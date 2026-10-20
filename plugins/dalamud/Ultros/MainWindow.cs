using System.Numerics;
using Dalamud.Bindings.ImGui;
using Dalamud.Interface.Windowing;
using Ultros.Client;
using Ultros.Lists;
using Item=Lumina.Excel.Sheets.Item;

namespace UltrosPlugin;

internal sealed class MainWindow : Window
{
    private readonly ClientWorker worker;
    private readonly RetainerCapture capture;
    private readonly Configuration config;
    private string label="My game client";
    private bool editLists,register,upload;
    private bool permissionsInitialized;
    private bool localhost;
    private string localOrigin,profile;
    private string search="",scope="";
    private bool automaticScope=true,openItem,pendingMarket;
    private int itemId=8,quantity=1,quality;
    private (uint Id,string Name)[] matches=[];
    private readonly Dictionary<string,(int Need,int Acquired)> drafts=[];
    private static readonly string[] Qualities=["any","nq","hq"];
    private string uiStatus="";
    internal MainWindow(ClientWorker worker,RetainerCapture capture,Configuration config):base($"Ultros v{ClientBuild.Version.ToString(4)}###UltrosConnected")
    {
        this.worker=worker;this.capture=capture;this.config=config;
        localhost=config.Localhost;localOrigin=config.LocalOrigin;profile=config.Profile;
        Size=new Vector2(920,670);SizeCondition=ImGuiCond.FirstUseEver;
        SizeConstraints=new WindowSizeConstraints{MinimumSize=new Vector2(650,440),MaximumSize=new Vector2(float.MaxValue)};
    }
    private void Submit(bool accepted){uiStatus=accepted?"":"The worker queue is full; retry shortly.";}
    private string? browserError;
    private void Open(string url)=>_ = OpenBrowser(url);
    private async Task OpenBrowser(string url)
    {
        // Approval links are validated by Connection; other URLs are built from
        // the configured origin. Dalamud's helper logs the full URL on failure.
        if(!await BrowserLinks.Open(url).ConfigureAwait(false))
            Interlocked.Exchange(ref browserError,"Could not open your browser. Open the verification page on another device and enter the short code shown here.");
    }
    private static string ItemName(int id)=>Plugin.Data.GetExcelSheet<Item>().TryGetRow((uint)id,out var item)?item.Name.ToString():$"Item #{id}";
    internal void ShowItem(int id){itemId=id;openItem=true;pendingMarket=true;IsOpen=true;}
    public override void Draw()
    {
        if(Interlocked.Exchange(ref browserError,null) is { } browserFailure)uiStatus=browserFailure;
        var v=worker.View;
        if(automaticScope)scope=v.PriceRegion;
        if(pendingMarket&&scope.Length>0){pendingMarket=false;Submit(worker.FetchItem(itemId,scope));}
        if(!permissionsInitialized&&v.Session is not null){editLists=v.Scopes.Contains("lists:write");register=v.Scopes.Contains("retainers:register");upload=v.Scopes.Contains("retainers:upload");permissionsInitialized=true;}
        ImGui.TextUnformatted(v.Connected?$"Connected · {v.Session?.AccountName} · {v.Session?.Label}":"Connect this game client to Ultros");
        if(config.Localhost)ImGui.TextColored(new Vector4(1,.7f,.2f,1),"LOCALHOST DEVELOPMENT");
        ImGui.TextWrapped(v.Status);
        if(uiStatus.Length>0)ImGui.TextWrapped(uiStatus);
        ImGui.Separator();
        if(!ImGui.BeginTabBar("ultros-tabs"))return;
        try
        {
            if(ImGui.BeginTabItem("Account")){try{Account(v);}finally{ImGui.EndTabItem();}}
            if(ImGui.BeginTabItem("Item",openItem?ImGuiTabItemFlags.SetSelected:ImGuiTabItemFlags.None)){try{openItem=false;Items(v);}finally{ImGui.EndTabItem();}}
            var activeList=v.Cart?.Name??v.Lists.FirstOrDefault(l=>l.List.Id==v.SelectedList)?.List.Name??"Cart";
            if(ImGui.BeginTabItem($"Lists - {activeList.Replace('#','＃')}###ultros-lists")){try{Cart(v);}finally{ImGui.EndTabItem();}}
            if(ImGui.BeginTabItem("Characters & retainers")){try{Characters(v);}finally{ImGui.EndTabItem();}}
            if(ImGui.BeginTabItem("Notification history")){try{Notifications(v);}finally{ImGui.EndTabItem();}}
            if(ImGui.BeginTabItem("Settings")){try{Settings();}finally{ImGui.EndTabItem();}}
        }
        finally{ImGui.EndTabBar();}
    }
    private void Account(ClientView v)
    {
        ImGui.TextWrapped(v.StorageMode);
        ImGui.InputText("Client label",ref label,64);
        ImGui.TextWrapped("Approve permissions in your browser. Notification delivery and list reading are requested by default.");
        ImGui.Checkbox("Edit cart rows",ref editLists);
        ImGui.Checkbox("Observe and register retainers",ref register);
        ImGui.Checkbox("Upload private retainer listings",ref upload);
        if(ImGui.Button(v.Connected?"Review new permissions":"Connect to Ultros"))Submit(worker.Pair(label,editLists,register,upload));
        if(v.HumanCode is { } code)
        {
            ImGui.Separator();
            ImGui.TextUnformatted($"Game code: {code}");
            ImGui.TextWrapped("Open the approval page to review this request directly. Compare the short code and account before approving.");
            ImGui.TextWrapped($"On a phone or another device: {v.VerificationUrl} — enter the short code above.");
            if(ImGui.Button("Open approval page"))Open(v.ApprovalUrl??v.VerificationUrl);
            ImGui.SameLine();if(ImGui.Button("Copy game code"))ImGui.SetClipboardText(code);
            ImGui.SameLine();if(ImGui.Button("Cancel pairing"))Submit(worker.CancelPairing());
        }
        if(v.Session is { } session)
        {
            ImGui.Separator();ImGui.TextUnformatted($"Account: {session.AccountName} ({session.AccountId})");
            ImGui.TextWrapped("Approved: "+string.Join(", ",session.Scopes));
            if(ImGui.Button("Manage connected clients"))Open(new Uri(new Uri(v.VerificationUrl),"/plugin/devices").AbsoluteUri);
            ImGui.SameLine();if(ImGui.Button("Disconnect this client"))Submit(worker.Disconnect());
        }
        ImGui.TextWrapped("Lists and alerts connect to your Ultros account. Game actions, purchases, and price changes remain manual.");
    }
    private void Search()
    {
        ImGui.SetNextItemWidth(260);
        var find=ImGui.InputTextWithHint("##search","Item name",ref search,100,ImGuiInputTextFlags.EnterReturnsTrue);
        ImGui.SameLine();find|=ImGui.Button("Find");
        if(find)
        {
            var term=search.Trim();
            matches=term.Length<2?[]:Plugin.Data.GetExcelSheet<Item>().Select(i=>(i.RowId,i.Name.ToString()))
                .Where(i=>i.Item2.Contains(term,StringComparison.OrdinalIgnoreCase)).Take(25).ToArray();
        }
        if(matches.Length>0&&ImGui.BeginCombo("Matches",ItemName(itemId)))
        {
            try{foreach(var m in matches)if(ImGui.Selectable($"{m.Name} (#{m.Id})"))itemId=checked((int)m.Id);}
            finally{ImGui.EndCombo();}
        }
        ImGui.SetNextItemWidth(100);ImGui.InputInt("Item ID",ref itemId);
    }
    private void Items(ClientView v)
    {
        Search();
        ImGui.TextWrapped("Right-click an item or recipe in the game and choose Search Ultros to open its market here.");
        ImGui.SetNextItemWidth(200);if(ImGui.InputTextWithHint("Price zone","Region, datacenter or world",ref scope,64))automaticScope=false;
        ImGui.SameLine();if(ImGui.Button("Use current region")){automaticScope=true;scope=v.PriceRegion;}
        ImGui.BeginDisabled(scope.Length==0);
        if(ImGui.Button("Load market & recent sales"))Submit(worker.FetchItem(itemId,scope));
        ImGui.EndDisabled();
        if(v.Market is not { } m)return;
        ImGui.Separator();ImGui.TextUnformatted($"{ItemName(m.ItemId)} · {m.Scope} · {m.Status}");
        if(ImGui.BeginTable("market",5,ImGuiTableFlags.RowBg|ImGuiTableFlags.Borders))
        {
            try {
                foreach(var title in new[]{"World ID","Retainer","Quality","Qty","Unit gil"})ImGui.TableSetupColumn(title);ImGui.TableHeadersRow();
                foreach(var row in m.Listings){ImGui.TableNextRow();foreach(var value in new[]{row.World,row.Retainer,row.Hq?"HQ":"NQ",row.Quantity.ToString(),row.UnitPrice.ToString("N0")}){ImGui.TableNextColumn();ImGui.TextUnformatted(value);}}
            }finally{ImGui.EndTable();}
        }
        if(m.Listings.Length==0)ImGui.TextWrapped("No cached listings for this item and scope.");
        if(ImGui.CollapsingHeader("Recent sales"))
            foreach(var sale in v.Sales)ImGui.TextUnformatted($"{sale.SoldAt} · {(sale.Hq?"HQ":"NQ")} ×{sale.Quantity} · {sale.UnitPrice:N0} gil/unit");
    }
    private void Cart(ClientView v)
    {
        var editTarget=CartCommandTarget.Capture(v);
        var cartTarget=CartCommandTarget.Capture(v);
        var recoveryTarget=RecoveryChoiceTarget.Capture(v);
        if(ImGui.Button("Refresh available lists"))Submit(worker.Refresh());
        ImGui.SameLine();
        if(ImGui.BeginCombo("Active list",v.Lists.FirstOrDefault(l=>l.List.Id==v.SelectedList)?.List.Name??"Cart"))
        {
            try{foreach(var list in v.Lists)if(ImGui.Selectable($"{list.List.Name} · {list.Permission}##{list.List.Id}",list.List.Id==v.SelectedList)){drafts.Clear();Submit(worker.SelectList(list.List.Id));}}
            finally{ImGui.EndCombo();}
        }
        if(v.Cart is not { } cart){ImGui.TextWrapped("Your online Cart is selected automatically when available. Create an online list named Cart on Ultros, or choose another list here.");return;}
        ImGui.TextUnformatted($"{cart.Name} · {(cart.AwaitingPersist?"UNSAVED — retry save":cart.RecoveryReview?"Recovery paused for review":"Saved locally")}");
        if(ImGui.Button("Retry save / sync"))Submit(worker.RetrySave(cartTarget));
        ImGui.SameLine();if(ImGui.Button("Refresh market estimate"))Submit(worker.Estimate(cartTarget));
        if(cart.RecoveryReview)
        {
            ImGui.TextWrapped("Both documents are preserved. Retry after resolving the conflict in Ultros, or keep the server copy. A recovery backup preserves the original local document.");
            if(ImGui.Button("Retry recovery"))Submit(worker.RetryRecovery(recoveryTarget));
            if(cart.HasServerCopy){ImGui.SameLine();if(ImGui.Button("Keep server copy (back up local)"))Submit(worker.UseServerCopy(recoveryTarget));}
            return;
        }
        ImGui.BeginDisabled(!v.CanEdit);
        try
        {
            ImGui.SetNextItemWidth(100);ImGui.InputInt("Add item ID",ref itemId);
            ImGui.SameLine();ImGui.SetNextItemWidth(80);ImGui.Combo("Quality",ref quality,Qualities,3);
            ImGui.SameLine();ImGui.SetNextItemWidth(85);ImGui.InputInt("Need",ref quantity);
            if(ImGui.Button("Add to cart"))Submit(worker.Edit(editTarget,new{kind="add",item_id=itemId,quality=Qualities[quality],need=quantity}));
            ImGui.SameLine();if(ImGui.Button("Undo"))Submit(worker.Edit(editTarget,new{kind="undo"}));
            ImGui.SameLine();if(ImGui.Button("Redo"))Submit(worker.Edit(editTarget,new{kind="redo"}));
        }finally{ImGui.EndDisabled();}
        if(ImGui.BeginTable("cart-v2",6,ImGuiTableFlags.RowBg|ImGuiTableFlags.Borders|ImGuiTableFlags.SizingStretchProp))
        {
            try
            {
                ImGui.TableSetupColumn("Item",ImGuiTableColumnFlags.WidthStretch,1);
                ImGui.TableSetupColumn("Quality",ImGuiTableColumnFlags.WidthFixed,60);
                ImGui.TableSetupColumn("Need",ImGuiTableColumnFlags.WidthFixed,80);
                ImGui.TableSetupColumn("Acquired",ImGuiTableColumnFlags.WidthFixed,80);
                ImGui.TableSetupColumn("Purchase",ImGuiTableColumnFlags.WidthFixed,100);
                ImGui.TableSetupColumn("",ImGuiTableColumnFlags.WidthFixed,75);ImGui.TableHeadersRow();
                foreach(var row in cart.Rows)
                {
                    var key=$"{v.SelectedList}/{row.ItemId}/{row.Quality}";
                    (int Need,int Acquired) draft=drafts.GetValueOrDefault(key,((int)Math.Clamp(row.Need,0,int.MaxValue),(int)Math.Clamp(row.Acquired,0,int.MaxValue)));
                    ImGui.PushID(key);
                    try
                    {
                        ImGui.TableNextRow();ImGui.TableNextColumn();
                        if(ImGui.Selectable(ItemName(row.ItemId))){itemId=row.ItemId;uiStatus="Item selected; open the Item tab to load its market.";}
                        ImGui.TableNextColumn();ImGui.TextUnformatted(row.Quality);
                        ImGui.BeginDisabled(!v.CanEdit);
                        try
                        {
                            ImGui.TableNextColumn();ImGui.SetNextItemWidth(-1);
                            var commit=ImGui.InputInt("##need",ref draft.Need,0,0,"%d",ImGuiInputTextFlags.EnterReturnsTrue);
                            var needActive=ImGui.IsItemActive();
                            commit|=ImGui.IsItemDeactivatedAfterEdit();
                            if(commit){Submit(worker.Edit(editTarget,new{kind="set_need",item_id=row.ItemId,quality=row.Quality,need=draft.Need}));drafts.Remove(key);}
                            ImGui.TableNextColumn();ImGui.SetNextItemWidth(-1);
                            var acquired=ImGui.InputInt("##acquired",ref draft.Acquired,0,0,"%d",ImGuiInputTextFlags.EnterReturnsTrue);
                            var acquiredActive=ImGui.IsItemActive();
                            acquired|=ImGui.IsItemDeactivatedAfterEdit();
                            if(acquired){Submit(worker.Edit(editTarget,new{kind="set_acquired",item_id=row.ItemId,quality=row.Quality,value=draft.Acquired}));drafts.Remove(key);}
                            if(!commit&&!acquired&&(needActive||acquiredActive))drafts[key]=draft;
                            else drafts.Remove(key);
                            ImGui.TableNextColumn();if(ImGui.Button("+1")){drafts.Remove(key);Submit(worker.Edit(editTarget,new{kind="add_acquired",item_id=row.ItemId,quality=row.Quality,delta=1}));}
                            ImGui.SameLine();if(ImGui.Button("+5")){drafts.Remove(key);Submit(worker.Edit(editTarget,new{kind="add_acquired",item_id=row.ItemId,quality=row.Quality,delta=5}));}
                            ImGui.TableNextColumn();if(ImGui.Button("Remove")){drafts.Remove(key);Submit(worker.Edit(editTarget,new{kind="remove",item_id=row.ItemId,quality=row.Quality}));}
                        }finally{ImGui.EndDisabled();}
                    }finally{ImGui.PopID();}
                }
            }finally{ImGui.EndTable();}
        }
        if(v.Estimate is { } estimate)
        {
            var current=estimate.Version==cart.Version;
            ImGui.TextWrapped($"{(current?"Estimate":"STALE estimate")} · {estimate.Scope} · {estimate.Total:N0} gil for priced units · {estimate.UnpricedUnits} unpriced units · fetched {estimate.ObservedAt.ToLocalTime():t}");
            ImGui.TextWrapped("Uses Ultros's shared Rust cart calculator. Estimates allow partial stacks; nothing is reserved.");
        }
    }
    private void Characters(ClientView v)
    {
        UploaderControls();
        if(v.RosterStatus.Length>0)ImGui.TextWrapped(v.RosterStatus);
        if(capture.Character is { } current)
        {
            ImGui.TextUnformatted($"Observed: {current.Name} · home world #{current.HomeWorld} · {current.Retainers.Length} retainers");
            ImGui.BeginDisabled(!v.Connected||!v.Scopes.Contains("retainers:register")||v.Registering);
            try
            {
                if(ImGui.Button("Save character only"))Submit(worker.Observe(current));
                ImGui.SameLine();if(ImGui.Button(v.Registering?"Saving retainers…":"Register all observed retainers"))Submit(worker.Register(current));
            }finally{ImGui.EndDisabled();}
        }
        if(v.Connected&&ImGui.Button("Open my retainers on Ultros"))Open(new Uri(new Uri(v.VerificationUrl),"/retainers").AbsoluteUri);
        ImGui.TextWrapped("Saving adds retainers to your Ultros account on the website and in game. Characters observed in game are unverified. There is no Lodestone bio challenge to complete.");
        foreach(var character in v.Characters)
        {
            ImGui.PushID(character.Id);
            try
            {
                ImGui.Separator();ImGui.TextUnformatted($"{character.Name} · home world #{character.HomeWorld} · unverified");
                ImGui.TextUnformatted(character.Registered?$"{character.Retainers.Count(r=>r.Registered)} retainers saved to your Ultros account":"Character observed; retainers have not been saved to your account.");
                ImGui.TextUnformatted($"Character upload permission: {(character.UploadEnabled?"allowed":"paused")} · last observed {character.ObservedAt.ToLocalTime():g}");
                if(character.UploadEnabled&&(!config.CaptureEnabled||config.CaptureGameBuild!=capture.GameBuild))
                    ImGui.TextWrapped("This character allows uploads, but the uploader on this game client must be enabled above.");
                ImGui.BeginDisabled(!v.Scopes.Contains("retainers:upload")||!character.Registered);
                try
                {
                    if(ImGui.Button(character.UploadEnabled?"Pause this character's uploads":"Allow this character's uploads"))Submit(worker.UploadConsent(character.Id,!character.UploadEnabled));
                    ImGui.SameLine();if(ImGui.Button("Pause & delete snapshots"))Submit(worker.UploadConsent(character.Id,false,true));
                }finally{ImGui.EndDisabled();}
                foreach(var r in character.Retainers)ImGui.TextUnformatted($"  {r.Name} · {(r.GroupingConflict?"saved under another character":r.Registered?"saved":"observed")}{(r.PublicRetainerId is not null?" · market listings connected":"")}");
            }finally{ImGui.PopID();}
        }
    }
    private void UploaderControls()
    {
        var enabled=config.CaptureEnabled&&config.CaptureGameBuild==capture.GameBuild;
        ImGui.TextUnformatted($"Listing uploader on this game client: {(enabled?"enabled":"off")}");
        ImGui.TextWrapped("Reads selling lists you open for characters with upload permission below. Listings are private until Universalis confirms them.");
        ImGui.BeginDisabled(!capture.Compatible||capture.GameBuild is null);
        try
        {
            if(ImGui.Button(enabled?"Pause listing uploader":"Enable listing uploader"))
            {
                config.CaptureEnabled=!enabled;
                config.CaptureGameBuild=capture.GameBuild;
                Plugin.Interface.SavePluginConfig(config);
                uiStatus=enabled?"Listing uploader paused on this game client.":"Listing uploader enabled. Close and reopen the retainer's Markets window to capture its listings.";
            }
        }finally{ImGui.EndDisabled();}
        ImGui.TextWrapped(capture.Status);
    }
    private void Notifications(ClientView v)
    {
        ImGui.TextUnformatted(v.NotificationTransport);
        if(ImGui.Button("Send endpoint test"))Submit(worker.TestNotification());
        ImGui.SameLine();if(ImGui.Button("Poll now"))Submit(worker.Refresh());
        ImGui.TextWrapped("Notifications appear in game chat. Configure their chat category and optional sound in Settings. Mark read is separate from delivery.");
        foreach(var entry in v.Inbox.Reverse())
        {
            ImGui.PushID(entry.Id);try
            {
                ImGui.Separator();ImGui.TextUnformatted(entry.Title);ImGui.TextWrapped(entry.Body);
                if(ImGui.Button("Mark read"))Submit(worker.MarkRead(entry.Id));
                if(entry.Action.Kind=="open_item"&&entry.Action.ItemId is { } id){ImGui.SameLine();if(ImGui.Button("Select item"))itemId=id;}
                if(entry.Action.Kind=="open_list"&&entry.Action.ListId is { } list){ImGui.SameLine();if(ImGui.Button("Open cart"))Submit(worker.SelectList(list));}
            }finally{ImGui.PopID();}
        }
    }
    private void Settings()
    {
        ImGui.InputText("Game-process profile",ref profile,32);
        if(UltrosEndpoint.LocalhostAvailable)ImGui.Checkbox("Target localhost development server",ref localhost);
        if(localhost)ImGui.InputText("Local origin",ref localOrigin,128);
        if(ImGui.Button("Save connection settings (reload plugin)"))
        {
            try{_=new UltrosEndpoint(localhost,localOrigin);if(profile.Length<1||profile.Any(c=>!char.IsAsciiLetterOrDigit(c)&&c is not '-' and not '_'))throw new ArgumentException();config.Localhost=localhost;config.LocalOrigin=localOrigin;config.Profile=profile;Plugin.Interface.SavePluginConfig(config);uiStatus="Connection settings saved. Reload Ultros in Dalamud to apply.";}
            catch{uiStatus="Use a loopback origin and a profile containing letters, numbers, '-' or '_'.";}
        }
        ImGui.Separator();
        ImGui.TextUnformatted("Notifications in game chat");
        var chat=Math.Max(0,Array.FindIndex(GameNotifications.ChatTypes,t=>(int)t==config.NotificationChatType));
        if(ImGui.Combo("Chat category",ref chat,GameNotifications.ChatLabels,GameNotifications.ChatLabels.Length)){config.NotificationChatType=(int)GameNotifications.ChatTypes[chat];Plugin.Interface.SavePluginConfig(config);}
        ImGui.TextWrapped("Ultros uses a local chat category with an [Ultros] prefix. Enable that category in your preferred game chat tab's filters.");
        var popup=config.NotificationPopup;var playSound=config.NotificationSound;
        if(ImGui.Checkbox("Also show a popup",ref popup)){config.NotificationPopup=popup;Plugin.Interface.SavePluginConfig(config);}
        if(ImGui.Checkbox("Play notification sound",ref playSound)){config.NotificationSound=playSound;Plugin.Interface.SavePluginConfig(config);}
        var sound=config.NotificationSoundId;
        if(ImGui.SliderInt("Sound effect",ref sound,1,16)){config.NotificationSoundId=sound;Plugin.Interface.SavePluginConfig(config);}
        if(ImGui.Button("Test chat notification"))Plugin.TestNotification();
        ImGui.Separator();
        ImGui.TextWrapped($"Capture compatibility: {capture.CompatibilityStatus}");
        ImGui.TextWrapped($"Game build: {capture.GameBuild??"ffxivgame.ver unavailable"}");
        UploaderControls();
        if(UltrosEndpoint.LocalhostAvailable&&ImGui.Button("Open local CRDT merge lab"))Plugin.OpenLab();
    }
}
