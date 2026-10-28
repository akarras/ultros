using System.Numerics;
using Dalamud.Bindings.ImGui;
using Dalamud.Interface.Windowing;
using Ultros.Client;
using Ultros.Lists;
using UltrosPlugin.Ui;
using Item=Lumina.Excel.Sheets.Item;

namespace UltrosPlugin;

internal sealed class MainWindow : Window
{
    private readonly ClientWorker worker;
    private readonly RetainerCapture capture;
    private readonly InventoryAcquisition inventoryProgress;
    private readonly Configuration config;
    private readonly AccountPanel account;
    private readonly NotificationPanel notifications;
    private readonly RetainerListingsPanel retainerListings;
    private Page page=Page.Account;
    private bool firstDraw=true;
    private bool localhost;
    private string localOrigin,profile;
    private string search="",scope="";
    private bool automaticScope=true,openItem,openLists,openRetainers,manageRetainers,pendingMarket;
    private int itemId=8,quantity=1,quality;
    private (uint Id,string Name)[] matches=[];
    private readonly Dictionary<string,(int Need,int Acquired)> drafts=[];
    private static readonly string[] Qualities=["any","nq","hq"];
    private string uiStatus="";
    internal MainWindow(ClientWorker worker,RetainerCapture capture,InventoryAcquisition inventoryProgress,Configuration config):base($"Ultros v{ClientBuild.Version.ToString(4)}###UltrosConnected")
    {
        this.worker=worker;this.capture=capture;this.inventoryProgress=inventoryProgress;this.config=config;
        account=new AccountPanel{
            Connect=o=>Submit(worker.Pair(o.Label,o.EditLists,o.Register,o.Upload,o.ReadRetainers)),
            Disconnect=()=>Submit(worker.Disconnect()),CancelPairing=()=>Submit(worker.CancelPairing()),
            Refresh=()=>Submit(worker.Refresh()),Open=Open};
        notifications=new NotificationPanel{
            Test=()=>Submit(worker.TestNotification()),Refresh=()=>Submit(worker.Refresh()),
            MarkRead=id=>Submit(worker.MarkRead(id)),Delete=(session,ids)=>Submit(worker.DeleteNotifications(session,ids)),
            OpenItem=id=>ShowItem(id),OpenList=id=>{Submit(worker.SelectList(id));openLists=true;}};
        retainerListings=new(){ItemName=ItemName,WorldName=WorldName,Marketable=Marketable,OpenItem=id=>ShowItem(id),SetEnabled=(id,enabled)=>Submit(worker.SetRetainerEnabled(id,enabled))};
        localhost=config.Localhost;localOrigin=config.LocalOrigin;profile=config.Profile;
        Size=new Vector2(940,700);SizeCondition=ImGuiCond.FirstUseEver;
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
            Interlocked.Exchange(ref browserError,"Could not open your browser. Open the website manually or retry this button.");
    }
    private static string ItemName(int id)=>Plugin.Data.GetExcelSheet<Item>().TryGetRow((uint)id,out var item)?item.Name.ToString():$"Item #{id}";
    private static bool Marketable(int id)=>id>0&&Plugin.Data.GetExcelSheet<Item>().TryGetRow((uint)id,out var item)&&item.ItemSearchCategory.RowId!=0;
    private static string WorldName(int id)=>Plugin.Data.GetExcelSheet<Lumina.Excel.Sheets.World>().TryGetRow((uint)id,out var world)?world.Name.ToString():$"World #{id}";
    internal void ShowItem(int id,string? priceWorld=null){itemId=id;openItem=true;pendingMarket=true;IsOpen=true;if(priceWorld is not null){scope=priceWorld;automaticScope=false;}}
    internal void ShowRetainers(){openRetainers=true;manageRetainers=true;IsOpen=true;}
    internal void AddToList(int id,ListEditTarget? destination)
    {
        itemId=id;quality=0;quantity=1;openItem=false;pendingMarket=false;openLists=true;IsOpen=true;
        uiStatus=destination is not null&&worker.AddItem(destination,id)
            ? "" : "Choose a writable list, then add the selected item. Enable list editing in Account if needed.";
    }
    public override void OnClose()=>account.Reset();
    public override void OnOpen()=>account.Reset();
    public override void PreDraw()=>ImGui.SetNextWindowBgAlpha(1);
    public override void Draw()
    {
        if(Interlocked.Exchange(ref browserError,null) is { } browserFailure)uiStatus=browserFailure;
        var v=worker.View;
        var activatePage=firstDraw||openItem||openLists||openRetainers;
        if(automaticScope)scope=v.PriceRegion;
        if(pendingMarket&&scope.Length>0){pendingMarket=false;Submit(worker.FetchItem(itemId,scope));}
        if(firstDraw){page=v.Connected?Page.Market:Page.Account;firstDraw=false;}
        if(openItem){page=Page.Market;openItem=false;}
        if(openLists){page=Page.Lists;openLists=false;}
        if(openRetainers){page=Page.Retainers;openRetainers=false;}
        UiKit.BeginStyle();
        try
        {
            page=UiKit.Navigation(page,v.Connected,v.Inbox.Length,activatePage);
            account.Track(v,page==Page.Account);
            UiKit.BeginContent();
            try
            {
                if(config.Localhost)UiKit.Muted("LOCALHOST DEVELOPMENT");
                switch(page)
                {
                    case Page.Account:account.Draw(v);break;
                    case Page.Market:UiKit.Heading("Market","Find an item and compare prices across worlds.");Items(v);break;
                    case Page.Lists:UiKit.Heading("Lists","Plan purchases and track what you've acquired.");Cart(v);break;
                    case Page.Retainers:UiKit.Heading("Retainers","Your uploaded listings, grouped by character and retainer.");Characters(v);break;
                    case Page.Notifications:notifications.Draw(v);break;
                    case Page.Settings:UiKit.Heading("Settings");Settings();break;
                }
                if(uiStatus.Length>0){UiKit.Section("Notice");ImGui.TextWrapped(uiStatus);if(ImGui.SmallButton("Dismiss"))uiStatus="";}
            }finally{UiKit.EndContent(uiStatus.Length>0?uiStatus:v.Status);}
        }
        finally{UiKit.EndStyle();}
    }
    private void Search()
    {
        ImGui.SetNextItemWidth(Math.Min(280*UiKit.Scale,ImGui.GetContentRegionAvail().X-80*UiKit.Scale));
        var find=ImGui.InputTextWithHint("##search","Item name",ref search,100,ImGuiInputTextFlags.EnterReturnsTrue);
        ImGui.SameLine();find|=ImGui.Button("Find");
        if(find)
        {
            var term=search.Trim();
            matches=term.Length<2?[]:Plugin.Data.GetExcelSheet<Item>().Where(i=>i.ItemSearchCategory.RowId!=0).Select(i=>(i.RowId,i.Name.ToString()))
                .Where(i=>i.Item2.Contains(term,StringComparison.OrdinalIgnoreCase)).Take(25).ToArray();
        }
        if(matches.Length>0&&ImGui.BeginCombo("Matches",ItemName(itemId)))
        {
            try{foreach(var m in matches)if(ImGui.Selectable($"{m.Name} (#{m.Id})"))itemId=checked((int)m.Id);}
            finally{ImGui.EndCombo();}
        }
        if(ImGui.CollapsingHeader("Find by item ID")){ImGui.SetNextItemWidth(130*UiKit.Scale);ImGui.InputInt("Item ID",ref itemId);}
    }
    private void Items(ClientView v)
    {
        Search();
        UiKit.Hint("You can also right-click an item in game to look it up here.");
        ImGui.BeginDisabled(itemId<=0||!Plugin.Data.GetExcelSheet<Item>().HasRow((uint)itemId));
        if(ImGui.Button("Add item to active list"))AddToList(itemId,ListEditTarget.Capture(v));
        ImGui.EndDisabled();
        UiKit.Section("Compare prices");
        ImGui.SetNextItemWidth(220*UiKit.Scale);if(ImGui.InputTextWithHint("##price-zone","Region, datacenter or world",ref scope,64))automaticScope=false;
        UiKit.Inline();if(ImGui.Button("Use current region")){automaticScope=true;scope=v.PriceRegion;}
        ImGui.BeginDisabled(scope.Length==0||!Marketable(itemId));
        if(ImGui.Button("Load market & recent sales"))Submit(worker.FetchItem(itemId,scope));
        ImGui.EndDisabled();
        if(v.Market is not { } m){UiKit.Empty("Choose an item to get started","Search by name above, then load its listings and recent sales.");return;}
        ImGui.Separator();ImGui.TextUnformatted($"{ItemName(m.ItemId)} · {m.Scope} · {m.Status}");
        if(ImGui.BeginTable("market",5,ImGuiTableFlags.RowBg|ImGuiTableFlags.Borders|ImGuiTableFlags.ScrollX))
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
        ImGui.SetNextItemWidth(Math.Max(120*UiKit.Scale,ImGui.GetContentRegionAvail().X-100*UiKit.Scale));
        if(ImGui.BeginCombo("Active list",v.Lists.FirstOrDefault(l=>l.List.Id==v.SelectedList)?.List.Name??"Cart"))
        {
            try{foreach(var list in v.Lists)if(ImGui.Selectable($"{list.List.Name} · {list.Permission}##{list.List.Id}",list.List.Id==v.SelectedList)){drafts.Clear();Submit(worker.SelectList(list.List.Id));}}
            finally{ImGui.EndCombo();}
        }
        if(!v.CanEdit)ImGui.TextWrapped("Choose a writable online list and enable list editing in Account to add items.");
        if(v.Cart is not { } cart){ImGui.TextWrapped("Your online Cart is selected automatically when available. Create an online list named Cart on Ultros, or choose another list here.");return;}
        ImGui.TextUnformatted($"{cart.Name} · {(cart.AwaitingPersist?"UNSAVED — retry save":cart.RecoveryReview?"Recovery paused for review":"Saved locally")}");
        UiKit.Hint(v.ListTransport);
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
            ImGui.TextUnformatted($"Selected item: {ItemName(itemId)}");
            ImGui.SetNextItemWidth(100);ImGui.InputInt("Add item ID",ref itemId);
            UiKit.Inline();ImGui.SetNextItemWidth(80*UiKit.Scale);ImGui.Combo("Quality",ref quality,Qualities,3);
            UiKit.Inline();ImGui.SetNextItemWidth(85*UiKit.Scale);ImGui.InputInt("Need",ref quantity);
            if(ImGui.Button("Add to list")) {
                if(itemId<=0||!Plugin.Data.GetExcelSheet<Item>().HasRow((uint)itemId)||quantity is <1 or >1_000_000)
                    uiStatus="Choose an item and a quantity between 1 and 1,000,000.";
                else if(ListEditTarget.Capture(v) is { } destination)Submit(worker.AddItem(destination,itemId,Qualities[quality],quantity));
            }
            ImGui.SameLine();if(ImGui.Button("Undo"))Submit(worker.Edit(editTarget,new{kind="undo"}));
            ImGui.SameLine();if(ImGui.Button("Redo"))Submit(worker.Edit(editTarget,new{kind="redo"}));
        }finally{ImGui.EndDisabled();}
        if(ImGui.BeginTable("cart-v2",6,ImGuiTableFlags.RowBg|ImGuiTableFlags.Borders|ImGuiTableFlags.SizingStretchProp|ImGuiTableFlags.ScrollX))
        {
            try
            {
                ImGui.TableSetupColumn("Item",ImGuiTableColumnFlags.WidthFixed,230*UiKit.Scale);
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
            UiKit.Hint("Estimates allow partial stacks. Prices may change before you buy.");
        }
    }
    private void Characters(ClientView v)
    {
        ImGui.BeginDisabled(!v.Connected||!v.Scopes.Contains("retainers:read")||v.RefreshingRetainers);
        if(ImGui.Button(v.RefreshingRetainers?"Refreshing…":"Refresh listings"))Submit(worker.RefreshRetainers());
        ImGui.EndDisabled();
        UiKit.Inline(240);if(ImGui.Button("View listings on Ultros"))Open(new Uri(new Uri(v.VerificationUrl),"/retainers/listings").AbsoluteUri);
        UiKit.Hint(capture.Status);
        if(capture.Bell is { } bell)
        {
            var saved=ClientWorker.LatestObservation(v.Characters,bell.Character);
            ImGui.BeginDisabled(!v.RetainerPreferences||!v.Scopes.Contains("retainers:register")||saved is null||RetainerRoster.GameOrder(saved,bell.Character) is null);
            if(ImGui.Button("Use in-game order"))Submit(worker.UseGameRetainerOrder(bell.Character));
            ImGui.EndDisabled();
        }
        UiKit.Hint("Retainers follow your saved Ultros order. Use in-game order at a summoning bell to update both views.");
        retainerListings.Draw(v);
        ImGui.Spacing();ImGui.Spacing();
        if(manageRetainers){ImGui.SetNextItemOpen(true,ImGuiCond.Always);manageRetainers=false;}
        if(!ImGui.CollapsingHeader("Manage uploads"))return;
        UploaderControls();
        if(v.RosterStatus.Length>0)ImGui.TextWrapped(v.RosterStatus);
        if(capture.Character is { } current)
        {
            ImGui.TextUnformatted($"{current.Name} · {WorldName(current.HomeWorld)} · {current.Retainers.Length} retainers found");
            ImGui.BeginDisabled(!v.Connected||!v.Scopes.Contains("retainers:register")||v.Registering);
            try
            {
                if(ImGui.Button("Save character only"))Submit(worker.Observe(current));
                ImGui.SameLine();if(ImGui.Button(v.Registering?"Saving retainers…":"Register all observed retainers"))Submit(worker.Register(current));
            }finally{ImGui.EndDisabled();}
        }
        foreach(var character in v.Characters)
        {
            ImGui.PushID(character.Id);
            try
            {
                UiKit.Section(character.Name);
                UiKit.Hint($"{character.Retainers.Count(r=>r.Registered)} retainers saved · uploads {(character.UploadEnabled?"allowed":"paused")}");
                if(character.UploadEnabled&&(!config.CaptureEnabled||config.CaptureGameBuild!=capture.GameBuild))
                    ImGui.TextWrapped("This character allows uploads, but the uploader on this game client must be enabled above.");
                ImGui.BeginDisabled(!v.Scopes.Contains("retainers:upload")||!character.Registered);
                try
                {
                    if(ImGui.Button(character.UploadEnabled?"Pause this character's uploads":"Allow this character's uploads"))Submit(worker.UploadConsent(character.Id,!character.UploadEnabled));
                    UiKit.Inline(220);if(ImGui.Button("Delete saved listings..."))ImGui.OpenPopup("Delete saved listings?");
                    if(ImGui.BeginPopupModal("Delete saved listings?",ImGuiWindowFlags.AlwaysAutoResize))
                    {
                        ImGui.TextUnformatted($"Pause uploads and delete private snapshots for {character.Name}?");
                        if(ImGui.Button("Delete saved listings")){Submit(worker.UploadConsent(character.Id,false,true));ImGui.CloseCurrentPopup();}
                        ImGui.SameLine();if(ImGui.Button("Cancel"))ImGui.CloseCurrentPopup();
                        ImGui.EndPopup();
                    }
                }finally{ImGui.EndDisabled();}
                retainerListings.Manage(character,v.Connected&&v.RetainerPreferences&&v.Scopes.Contains("retainers:register"));
            }finally{ImGui.PopID();}
        }
    }
    private void UploaderControls()
    {
        var enabled=config.CaptureEnabled&&capture.Compatible&&capture.GameBuild is not null&&config.CaptureGameBuild==capture.GameBuild;
        ImGui.TextUnformatted($"Listing sync: {(enabled?"on":"paused")}");
        UiKit.Hint("Selling prices you confirm in game sync privately to Ultros.");
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
        UiKit.Hint(capture.Status);
    }
    private void Settings()
    {
        ImGui.TextUnformatted("Alerts");
        var chat=Math.Max(0,Array.FindIndex(GameNotifications.ChatTypes,t=>(int)t==config.NotificationChatType));
        ImGui.SetNextItemWidth(220*UiKit.Scale);
        if(ImGui.Combo("Chat category",ref chat,GameNotifications.ChatLabels,GameNotifications.ChatLabels.Length)){config.NotificationChatType=(int)GameNotifications.ChatTypes[chat];Plugin.Interface.SavePluginConfig(config);}
        UiKit.Hint("Enable this category in your game chat tab's filters to see [Ultros] messages.");
        var popup=config.NotificationPopup;var playSound=config.NotificationSound;
        if(ImGui.Checkbox("Also show a popup",ref popup)){config.NotificationPopup=popup;Plugin.Interface.SavePluginConfig(config);}
        if(ImGui.Checkbox("Play notification sound",ref playSound)){config.NotificationSound=playSound;Plugin.Interface.SavePluginConfig(config);}
        if(playSound)
        {
            var sound=config.NotificationSoundId;
            ImGui.SetNextItemWidth(220*UiKit.Scale);
            if(ImGui.SliderInt("Sound effect",ref sound,1,16)){config.NotificationSoundId=sound;Plugin.Interface.SavePluginConfig(config);}
        }
        if(ImGui.Button("Test chat notification"))Plugin.TestNotification();
        UiKit.Section("Shortcuts");
        var itemShortcuts=config.ItemContextMenuEnabled;
        if(ImGui.Checkbox("Item right-click shortcuts",ref itemShortcuts)){config.ItemContextMenuEnabled=itemShortcuts;Plugin.Interface.SavePluginConfig(config);}
        UiKit.Hint("Show Ultros actions in item menus. Summoning-bell controls work independently.");
        UiKit.Section("List progress");
        var automatic=config.InventoryProgressEnabled;
        var canEnable=capture.Compatible&&capture.GameBuild is not null;
        ImGui.BeginDisabled(!canEnable&&!automatic);
        if(ImGui.Checkbox("Track new inventory items",ref automatic)) {
            config.InventoryProgressEnabled=automatic;config.InventoryProgressGameBuild=automatic?capture.GameBuild:null;
            worker.CancelQueuedInventory();Plugin.Interface.SavePluginConfig(config);
        }
        ImGui.EndDisabled();
        UiKit.Hint("Mark incoming items acquired on your active list.");
        if(automatic&&canEnable&&config.InventoryProgressGameBuild!=capture.GameBuild&&ImGui.Button("Enable inventory progress for this game build")) {
            config.InventoryProgressGameBuild=capture.GameBuild;worker.CancelQueuedInventory();Plugin.Interface.SavePluginConfig(config);
        }
        UiKit.Hint(inventoryProgress.Status);
        if(ImGui.CollapsingHeader("How inventory progress works"))
            ImGui.TextWrapped("New items entering your bags count toward the active writable list. Existing items are the baseline. Transfers from retainers and saddlebags count; moving items within bags and equipment does not. Consuming items does not undo progress.");
        UiKit.Section("Advanced");
        if(ImGui.CollapsingHeader("Connection profile"))
        {
            ImGui.InputText("Game-process profile",ref profile,32);
            if(UltrosEndpoint.LocalhostAvailable)ImGui.Checkbox("Target localhost development server",ref localhost);
            if(localhost)ImGui.InputText("Local origin",ref localOrigin,128);
            if(ImGui.Button("Save connection settings (reload plugin)"))
            {
                try{_=new UltrosEndpoint(localhost,localOrigin);if(profile.Length<1||profile.Any(c=>!char.IsAsciiLetterOrDigit(c)&&c is not '-' and not '_'))throw new ArgumentException();config.Localhost=localhost;config.LocalOrigin=localOrigin;config.Profile=profile;Plugin.Interface.SavePluginConfig(config);uiStatus="Connection settings saved. Reload Ultros in Dalamud to apply.";}
                catch{uiStatus="Use a loopback origin and a profile containing letters, numbers, '-' or '_'.";}
            }
        }
        if(ImGui.CollapsingHeader("Capture diagnostics"))
        {
            ImGui.TextWrapped($"Capture compatibility: {capture.CompatibilityStatus}");
            ImGui.TextWrapped($"Game build: {capture.GameBuild??"ffxivgame.ver unavailable"}");
            UploaderControls();
        }
        if(UltrosEndpoint.LocalhostAvailable&&ImGui.Button("Open local CRDT merge lab"))Plugin.OpenLab();
    }
}
