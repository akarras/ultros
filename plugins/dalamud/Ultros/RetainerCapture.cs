using System.Security.Cryptography;
using System.Text.Json;
using System.Numerics;
using Dalamud.Game.ClientState.Conditions;
using Dalamud.Game.Gui;
using Dalamud.Game.Addon.Lifecycle;
using Dalamud.Game.Addon.Lifecycle.AddonArgTypes;
using Dalamud.Plugin.Services;
using FFXIVClientStructs.FFXIV.Client.Game;
using FFXIVClientStructs.FFXIV.Component.GUI;
using Ultros.Client;

namespace UltrosPlugin;

internal sealed record RetainerBell(ObservedCharacter Character,Vector2 Position,float Scale);

/// <summary>Read-only copies on the framework thread; pointers never leave this class.</summary>
internal sealed unsafe class RetainerCapture : IDisposable
{
    private readonly ClientWorker worker;
    private readonly Configuration config;
    private readonly CaptureGate gate = new();
    private readonly CaptureFreshness freshness = new();
    private CaptureUpload? upload;
    private DateTimeOffset nextRead;
    private DateTimeOffset nextRegistration;
    private string? registrationContext;
    private string? editorVisit;
    internal ObservedCharacter? Character { get; private set; }
    internal RetainerBell? Bell { get; private set; }
    private string status = "Uploader is off.";
    internal string Status
    {
        get=>status;
        private set
        {
            if(status==value)return;
            status=value;
            // Static reasons and listing counts only; never game IDs, credentials or raw exceptions.
            Plugin.Log.Information("Ultros retainer capture: {Status}",value);
        }
    }
    internal string? GameBuild { get; }
    internal bool Compatible { get; }
    internal string CompatibilityStatus { get; } = "Package verification files could not be read. Keep capture-compatibility.json beside the plugin DLL and reload.";
    private sealed record Compatibility(string ClientStructsSha256);
    internal RetainerCapture(ClientWorker worker,Configuration config)
    {
        this.worker=worker;this.config=config;
        try
        {
            using var process=System.Diagnostics.Process.GetCurrentProcess();
            var directory=Path.GetDirectoryName(process.MainModule!.FileName)!;
            var versionFile=Path.Combine(directory,"ffxivgame.ver");
            if(File.Exists(versionFile)&&new FileInfo(versionFile).Length<=128)
            {
                var value=File.ReadAllText(versionFile).Trim();
                if(value.Length is >0 and <=64&&value.All(c=>char.IsAsciiDigit(c)||c=='.'))GameBuild=value;
            }
        }
        catch { }
        try
        {
            var file=Path.Combine(Plugin.Interface.AssemblyLocation.Directory!.FullName,"capture-compatibility.json");
            if(new FileInfo(file).Length>1024)throw new JsonException();
            var expected=JsonSerializer.Deserialize<Compatibility>(File.ReadAllBytes(file));
            if(expected?.ClientStructsSha256 is not { Length: 64 } hash||!hash.All(Uri.IsHexDigit))throw new JsonException();
            var actual=Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(typeof(RetainerManager).Assembly.Location)));
            Compatible=string.Equals(hash,actual,StringComparison.OrdinalIgnoreCase);
            CompatibilityStatus=Compatible
                ? "Matches the loaded Dalamud client structs."
                : $"Dalamud client structs changed (package {hash[..8]}, loaded {actual[..8]}). Rebuild Ultros against the installed Dalamud version and reload.";
        }
        catch(JsonException){CompatibilityStatus="capture-compatibility.json is invalid. Rebuild or extract the complete plugin package and reload.";}
        catch {Compatible=false;}
        // No build pin means the uploader has never been configured, including
        // old default-off settings. Explicit pauses always save their build pin.
        if(Compatible&&GameBuild is not null&&config.CaptureGameBuild is null) {
            config.CaptureEnabled=true;
            config.CaptureGameBuild=GameBuild;
            Plugin.Interface.SavePluginConfig(config);
        }
        Plugin.Log.Information("Ultros retainer reader: {Compatibility}; game {Game}; plugin {Path}",CompatibilityStatus,GameBuild??"unknown",Plugin.Interface.AssemblyLocation.FullName);
        Plugin.GameGui.AgentUpdate+=OnAgentUpdate;
        Plugin.AddonLifecycle.RegisterListener(AddonEvent.PostSetup,"RetainerSellList",OnSellingListUpdate);
        Plugin.AddonLifecycle.RegisterListener(AddonEvent.PostRequestedUpdate,"RetainerSellList",OnSellingListUpdate);
        // A price editor can close without changing inventory quantities, or
        // with the same price. Re-read the complete inventory after it closes;
        // never send the editor's unconfirmed numeric input.
        Plugin.AddonLifecycle.RegisterListener(AddonEvent.PostSetup,"RetainerSell",OnPriceEditorOpened);
        Plugin.AddonLifecycle.RegisterListener(AddonEvent.PostOpen,"RetainerSell",OnPriceEditorOpened);
        Plugin.AddonLifecycle.RegisterListener(AddonEvent.PreFinalize,"RetainerSell",OnPriceEditorClosed);
        Plugin.AddonLifecycle.RegisterListener(AddonEvent.PostClose,"RetainerSell",OnPriceEditorClosed);
        Plugin.Framework.Update+=Update;
    }
    private void OnAgentUpdate(AgentUpdateFlag flag)
    {
        if((flag&AgentUpdateFlag.RetainerMarketInventoryUpdate)!=0)SignalFreshness();
    }
    private void OnSellingListUpdate(AddonEvent type,AddonArgs args)=>SignalFreshness();
    private string? CurrentVisit()
    {
        if(!Compatible||!config.CaptureEnabled||!Plugin.Player.IsLoaded||Plugin.Player.ContentId==0||!Plugin.Condition[ConditionFlag.OccupiedSummoningBell])return null;
        var manager=RetainerManager.Instance();
        if(manager==null||!manager->IsReady)return null;
        var active=manager->GetActiveRetainer();
        if(active==null||active->RetainerId==0||active->RetainerId!=manager->LastSelectedRetainerId||manager->RetainerObjectId==0)return null;
        return $"{Plugin.Player.ContentId}/{active->RetainerId}/{manager->RetainerObjectId}";
    }
    private void SignalFreshness(){if(CurrentVisit() is { } visit)freshness.Signal(visit,DateTimeOffset.UtcNow);}
    private void OnPriceEditorOpened(AddonEvent type,AddonArgs args)=>editorVisit=CurrentVisit();
    private void OnPriceEditorClosed(AddonEvent type,AddonArgs args)
    {
        var visit=editorVisit;editorVisit=null;
        // An old editor closing during a retainer switch must not authorize a
        // capture from the next retainer's not-yet-refreshed inventory.
        if(visit is not null&&visit==CurrentVisit())freshness.Signal(visit,DateTimeOffset.UtcNow);
    }
    private void Reset(string status,bool awaitingWindow=false){gate.Reset();upload=null;if(!awaitingWindow){freshness.Reset();editorVisit=null;}worker.CancelQueuedCaptures();Status=status;}
    private void Update(IFramework framework)
    {
        UpdateBell();
        if(DateTimeOffset.UtcNow<nextRead)return;
        nextRead=DateTimeOffset.UtcNow.AddMilliseconds(250);
        if(!Compatible){Character=null;Reset($"Capture disabled: {CompatibilityStatus}");return;}
        if(GameBuild is null){Character=null;Reset("Capture disabled: ffxivgame.ver could not be read beside the game executable. Check the game installation and reload the plugin.");return;}
        if(config.CaptureGameBuild!=GameBuild){Character=null;Reset("Game build changed. Enable the uploader for the current build in Settings.");return;}
        if(!Plugin.Player.IsLoaded||Plugin.Player.ContentId==0||Plugin.Condition[ConditionFlag.BetweenAreas])
        {Character=null;Reset("Log in and visit a summoning bell.");return;}
        try
        {
            var player=Plugin.Player.ContentId;
            var manager=RetainerManager.Instance();
            if(manager==null||!manager->IsReady){Character=null;Reset("Visit a summoning bell to load your retainer roster.");return;}
            var roster=new List<ObservedRetainer>();
            var bellList=Plugin.GameGui.GetAddonByName("RetainerList",1);
            var bellRosterVisible=!bellList.IsNull&&bellList.IsReady&&bellList.IsVisible;
            var entries=manager->Retainers.ToArray();
            var display=RetainerRoster.DisplaySlots(entries.Select(r=>r.RetainerId).ToArray(),manager->DisplayOrder.ToArray());
            var slots=display??Enumerable.Range(0,10).Where(i=>entries[i].RetainerId!=0).ToArray();
            foreach(var slot in slots)
            {
                var retainer=entries[slot];
                if(retainer.RetainerId==0)continue;
                var name=retainer.NameString;
                if(name.Length is <1 or >64)throw new InvalidOperationException();
                roster.Add(new(retainer.RetainerId.ToString(),name,(int)retainer.Town==0?null:(int)retainer.Town,display is null?null:roster.Count,bellRosterVisible?retainer.Available:null));
            }
            if(roster.Count>10||Plugin.Player.ContentId!=player)throw new InvalidOperationException();
            Character=new(player.ToString(),Plugin.Player.CharacterName,(int)Plugin.Player.HomeWorld.RowId,roster.ToArray());
            var view=worker.View;
            var registrationVisit=$"{view.Session?.Id}/{Character.ContentId}";
            if(registrationContext!=registrationVisit){registrationContext=registrationVisit;nextRegistration=DateTimeOffset.MinValue;}
            if(Plugin.Condition[ConditionFlag.OccupiedSummoningBell]&&view.Connected&&view.Scopes.Contains("retainers:register")&&DateTimeOffset.UtcNow>=nextRegistration)
            {
                // Metadata changes must not re-claim a retainer the user
                // deliberately removed on the website. The bell action is an
                // explicit refresh; automatic known-roster updates only observe.
                var offered=ClientWorker.NeedsRegistration(view.Characters,Character)?worker.Register(Character)
                    :view.RetainerPreferences&&RetainerRoster.Changed(view.Characters,Character)&&worker.Observe(Character);
                if(offered)nextRegistration=DateTimeOffset.UtcNow.AddSeconds(15);
            }
            if(!config.CaptureEnabled){Reset("Listing capture is off on this game client. Click Enable listing uploader to send listings from Markets.");return;}
            if(config.CaptureGameBuild!=GameBuild){Reset("Game build changed. Enable the uploader for the current build in Settings.");return;}
            if(!view.Connected||!view.Scopes.Contains("retainers:upload")){Reset("Connect and approve retainer uploads first.");return;}
            var matches=view.Characters.Where(c=>c.Name==Character.Name&&c.HomeWorld==Character.HomeWorld&&c.Registered&&c.UploadEnabled).ToArray();
            if(matches.Length!=1){Reset(view.Registering?"Saving this character's retainers…":"Check this character's registration and paused uploads in Retainers.",true);return;}
            if(!Plugin.Condition[ConditionFlag.OccupiedSummoningBell]){Reset("Visit a summoning bell and open a retainer's selling list.");return;}
            var addon=Plugin.GameGui.GetAddonByName("RetainerSellList",1);
            if(addon.IsNull||!addon.IsReady||!addon.IsVisible){Reset("Open a retainer's selling list.",true);return;}
            var selected=manager->LastSelectedRetainerId;
            var active=manager->GetActiveRetainer();
            if(active==null||selected==0||active->RetainerId!=selected||manager->RetainerObjectId==0){Reset("Waiting for a coherent retainer context.");return;}
            var privateMatches=matches[0].Retainers.Where(r=>r.Registered&&!r.GroupingConflict&&r.Name==active->NameString).ToArray();
            if(privateMatches.Length!=1){Reset("Register the active retainer to this character first.");return;}
            var privateRetainer=privateMatches[0];
            if(privateRetainer.Disabled){Reset("This retainer is disabled. Enable it in Manage uploads when you want to use it again.");return;}
            var inventory=InventoryManager.Instance();
            if(inventory==null){Reset("Retainer inventory unavailable.");return;}
            var container=inventory->GetInventoryContainer(InventoryType.RetainerMarket);
            if(container==null||!container->IsLoaded||container->Items==null||container->Size!=20){Reset("Waiting for the full retainer market inventory.",true);return;}
            var rows=new List<PrivateListing>();
            for(var slot=0;slot<20;slot++)
            {
                var item=container->Items[slot];
                if(item.ItemId==0)continue;
                if(item.IsSymbolic||item.Slot!=slot||!Plugin.Data.GetExcelSheet<Lumina.Excel.Sheets.Item>().HasRow(item.ItemId))throw new InvalidOperationException();
                var price=inventory->RetainerMarketPrices[slot];
                rows.Add(new((byte)slot,(int)item.ItemId,(item.Flags&InventoryItem.ItemFlags.HighQuality)!=0,item.Quantity,checked((long)price)));
            }
            if(Plugin.Player.ContentId!=player||manager->LastSelectedRetainerId!=selected||!container->IsLoaded){Reset("Retainer changed while capturing. Revisit its selling list.");return;}
            var visit=$"{player}/{selected}/{manager->RetainerObjectId}";
            var now=DateTimeOffset.UtcNow;
            var context=$"{visit}/{view.Session?.Id}/{privateRetainer.Id}/{matches[0].CaptureEpoch}";
            if(upload is not null&&upload.Context!=context){upload=null;worker.CancelQueuedCaptures();}
            if(upload is not null) {
                var finished=upload.Advance(now,capture=>worker.Upload(privateRetainer.Id,capture));
                Status=upload.Status;
                if(finished) {
                    if(upload.Result?.Saved==true)gate.Acknowledged(upload.Capture.Listings);
                    else{gate.Reset();freshness.Reset();}
                    upload=null;
                }
                return;
            }
            var complete=gate.Observe(context,freshness.Take(visit,now),true,active->MarketItemCount,rows.ToArray(),now);
            if(complete is null){Status=gate.Status;return;}
            var capture=new ListingCapture(Guid.NewGuid().ToString("N"),privateRetainer.Revision,matches[0].CaptureEpoch,DateTimeOffset.UtcNow,true,complete);
            upload=new(context,capture);
            upload.Advance(now,candidate=>worker.Upload(privateRetainer.Id,candidate));Status=upload.Status;
        }
        catch {Character=null;Reset("Capture stopped because the game data was inconsistent. Revisit the retainer.");}
    }
    private void UpdateBell()
    {
        Bell=null;
        if(!Compatible||GameBuild is null||config.CaptureGameBuild!=GameBuild||!Plugin.Player.IsLoaded
            ||!Plugin.Condition[ConditionFlag.OccupiedSummoningBell]||Character is not { } current
            ||current.ContentId!=Plugin.Player.ContentId.ToString())return;
        var addon=Plugin.GameGui.GetAddonByName("RetainerList",1);
        if(addon.IsNull||!addon.IsReady||!addon.IsVisible)return;
        var unit=(AtkUnitBase*)addon.Address;
        if(unit->RootNode==null||unit->RootNode->Width<300||!float.IsFinite(unit->Scale)||unit->Scale is <0.5f or >4)return;
        Bell=new(current,new(unit->X+180*unit->Scale,unit->Y+5*unit->Scale),unit->Scale);
    }
    public void Dispose(){Plugin.Framework.Update-=Update;Plugin.GameGui.AgentUpdate-=OnAgentUpdate;Plugin.AddonLifecycle.UnregisterListener(OnSellingListUpdate);Plugin.AddonLifecycle.UnregisterListener(OnPriceEditorOpened);Plugin.AddonLifecycle.UnregisterListener(OnPriceEditorClosed);Character=null;Bell=null;editorVisit=null;gate.Reset();freshness.Reset();worker.CancelQueuedCaptures();}
}
