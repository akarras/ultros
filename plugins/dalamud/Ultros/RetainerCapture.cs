using System.Security.Cryptography;
using System.Text.Json;
using Dalamud.Game.ClientState.Conditions;
using Dalamud.Game.Gui;
using Dalamud.Game.Addon.Lifecycle;
using Dalamud.Game.Addon.Lifecycle.AddonArgTypes;
using Dalamud.Plugin.Services;
using FFXIVClientStructs.FFXIV.Client.Game;
using Ultros.Client;

namespace UltrosPlugin;

/// <summary>Read-only copies on the framework thread; pointers never leave this class.</summary>
internal sealed unsafe class RetainerCapture : IDisposable
{
    private readonly ClientWorker worker;
    private readonly Configuration config;
    private readonly CaptureGate gate = new();
    private readonly CaptureFreshness freshness = new();
    private CaptureUpload? upload;
    private DateTimeOffset nextRead;
    internal ObservedCharacter? Character { get; private set; }
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
        Plugin.Log.Information("Ultros retainer reader: {Compatibility}; game {Game}; plugin {Path}",CompatibilityStatus,GameBuild??"unknown",Plugin.Interface.AssemblyLocation.FullName);
        Plugin.GameGui.AgentUpdate+=OnAgentUpdate;
        Plugin.AddonLifecycle.RegisterListener(AddonEvent.PostSetup,"RetainerSellList",OnSellingListUpdate);
        Plugin.AddonLifecycle.RegisterListener(AddonEvent.PostRequestedUpdate,"RetainerSellList",OnSellingListUpdate);
        Plugin.Framework.Update+=Update;
    }
    private void OnAgentUpdate(AgentUpdateFlag flag)
    {
        if((flag&AgentUpdateFlag.RetainerMarketInventoryUpdate)!=0)SignalFreshness();
    }
    private void OnSellingListUpdate(AddonEvent type,AddonArgs args)=>SignalFreshness();
    private void SignalFreshness()
    {
        if(!Compatible||!config.CaptureEnabled||!Plugin.Player.IsLoaded||!Plugin.Condition[ConditionFlag.OccupiedSummoningBell])return;
        var manager=RetainerManager.Instance();
        if(manager==null||!manager->IsReady)return;
        var active=manager->GetActiveRetainer();
        if(active==null||active->RetainerId==0||active->RetainerId!=manager->LastSelectedRetainerId||manager->RetainerObjectId==0)return;
        freshness.Signal($"{Plugin.Player.ContentId}/{active->RetainerId}/{manager->RetainerObjectId}",DateTimeOffset.UtcNow);
    }
    private void Reset(string status,bool awaitingWindow=false){gate.Reset();upload=null;if(!awaitingWindow)freshness.Reset();worker.CancelQueuedCaptures();Status=status;}
    private void Update(IFramework framework)
    {
        if(DateTimeOffset.UtcNow<nextRead)return;
        nextRead=DateTimeOffset.UtcNow.AddMilliseconds(250);
        if(!Compatible){Character=null;Reset($"Capture disabled: {CompatibilityStatus}");return;}
        if(GameBuild is null){Character=null;Reset("Capture disabled: ffxivgame.ver could not be read beside the game executable. Check the game installation and reload the plugin.");return;}
        if(!Plugin.Player.IsLoaded||Plugin.Player.ContentId==0||Plugin.Condition[ConditionFlag.BetweenAreas])
        {Character=null;Reset("Log in and visit a summoning bell.");return;}
        try
        {
            var player=Plugin.Player.ContentId;
            var manager=RetainerManager.Instance();
            if(manager==null||!manager->IsReady){Character=null;Reset("Visit a summoning bell to load your retainer roster.");return;}
            var roster=new List<ObservedRetainer>();
            foreach(var retainer in manager->Retainers)
            {
                if(retainer.RetainerId==0)continue;
                var name=retainer.NameString;
                if(name.Length is <1 or >64)throw new InvalidOperationException();
                roster.Add(new(retainer.RetainerId.ToString(),name,(int)retainer.Town==0?null:(int)retainer.Town));
            }
            if(roster.Count>10||Plugin.Player.ContentId!=player)throw new InvalidOperationException();
            Character=new(player.ToString(),Plugin.Player.CharacterName,(int)Plugin.Player.HomeWorld.RowId,roster.ToArray());
            if(!config.CaptureEnabled){Reset("Listing capture is off on this game client. Click Enable listing uploader to send listings from Markets.");return;}
            if(config.CaptureGameBuild!=GameBuild){Reset("Game build changed. Enable the uploader for the current build in Settings.");return;}
            var view=worker.View;
            if(!view.Connected||!view.Scopes.Contains("retainers:upload")){Reset("Connect and approve retainer uploads first.");return;}
            var matches=view.Characters.Where(c=>c.Name==Character.Name&&c.HomeWorld==Character.HomeWorld&&c.Registered&&c.UploadEnabled).ToArray();
            if(matches.Length!=1){Reset("Register this character and enable its uploads first.");return;}
            if(!Plugin.Condition[ConditionFlag.OccupiedSummoningBell]){Reset("Visit a summoning bell and open a retainer's selling list.");return;}
            var addon=Plugin.GameGui.GetAddonByName("RetainerSellList",1);
            if(addon.IsNull||!addon.IsReady||!addon.IsVisible){Reset("Open a retainer's selling list.",true);return;}
            var selected=manager->LastSelectedRetainerId;
            var active=manager->GetActiveRetainer();
            if(active==null||selected==0||active->RetainerId!=selected||manager->RetainerObjectId==0){Reset("Waiting for a coherent retainer context.");return;}
            var privateMatches=matches[0].Retainers.Where(r=>r.Registered&&!r.GroupingConflict&&r.Name==active->NameString).ToArray();
            if(privateMatches.Length!=1){Reset("Register the active retainer to this character first.");return;}
            var privateRetainer=privateMatches[0];
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
    public void Dispose(){Plugin.Framework.Update-=Update;Plugin.GameGui.AgentUpdate-=OnAgentUpdate;Plugin.AddonLifecycle.UnregisterListener(OnSellingListUpdate);Character=null;gate.Reset();freshness.Reset();worker.CancelQueuedCaptures();}
}
