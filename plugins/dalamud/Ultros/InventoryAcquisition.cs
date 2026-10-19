using Dalamud.Game.ClientState.Conditions;
using Dalamud.Game.Inventory;
using Dalamud.Game.Inventory.InventoryEventArgTypes;
using Dalamud.Plugin.Services;
using FFXIVClientStructs.FFXIV.Client.Game;
using Ultros.Client;
namespace UltrosPlugin;

/// <summary>Copy loaded player inventory on the framework thread; queue only list deltas.</summary>
internal sealed unsafe class InventoryAcquisition : IDisposable
{
    private static readonly GameInventoryType[] Containers=[
        GameInventoryType.Inventory1,GameInventoryType.Inventory2,GameInventoryType.Inventory3,GameInventoryType.Inventory4,
        GameInventoryType.Crystals,GameInventoryType.EquippedItems,GameInventoryType.ArmoryMainHand,
        GameInventoryType.ArmoryOffHand,GameInventoryType.ArmoryHead,GameInventoryType.ArmoryBody,
        GameInventoryType.ArmoryHands,GameInventoryType.ArmoryLegs,GameInventoryType.ArmoryFeets,
        GameInventoryType.ArmoryEar,GameInventoryType.ArmoryNeck,GameInventoryType.ArmoryWrist,
        GameInventoryType.ArmoryRings,GameInventoryType.ArmorySoulCrystal,
    ];
    private readonly ClientWorker worker;
    private readonly RetainerCapture reader;
    private readonly Configuration config;
    private readonly InventoryProgress progress=new();
    private ListEditTarget? target;
    private ulong character;
    private int generation;
    private bool dirty;
    private long notBefore;
    private Dictionary<InventoryKey,long>? candidate;
    private InventoryGain[] pending=[];
    internal string Status { get; private set; }="Automatic inventory progress is off.";

    internal InventoryAcquisition(ClientWorker worker,RetainerCapture reader,Configuration config)
    {
        this.worker=worker;this.reader=reader;this.config=config;
        Plugin.Inventory.InventoryChanged+=Changed;
        Plugin.Framework.Update+=Update;
    }
    private void Changed(IReadOnlyCollection<InventoryEventArgs> changes)
    {
        dirty=true;candidate=null;notBefore=Environment.TickCount64+500;
    }
    private void Reset(string status)
    {
        if(target is not null)worker.CancelQueuedInventory();
        target=null;character=0;progress.Reset();candidate=null;pending=[];dirty=false;Status=status;
    }
    private void Update(IFramework framework)
    {
        if(!config.InventoryProgressEnabled){Reset("Automatic inventory progress is off.");return;}
        if(!reader.Compatible||reader.GameBuild is null){Reset("Inventory reader compatibility could not be verified. Install a matching package.");return;}
        if(config.InventoryProgressGameBuild!=reader.GameBuild){Reset("Re-enable inventory progress for the current game build in Settings.");return;}
        var destination=ListEditTarget.Capture(worker.View);
        if(destination is null||!Plugin.Player.IsLoaded||Plugin.Player.ContentId==0||Plugin.Condition[ConditionFlag.BetweenAreas]){
            Reset("Waiting for a loaded character and a writable active list.");return;
        }
        var currentGeneration=worker.InventoryGeneration;
        if(target!=destination||character!=Plugin.Player.ContentId||generation!=currentGeneration){
            worker.CancelQueuedInventory();generation=worker.InventoryGeneration;
            target=destination;character=Plugin.Player.ContentId;progress.Reset();candidate=null;pending=[];
            dirty=true;notBefore=Environment.TickCount64+500;Status="Establishing an inventory baseline…";
        }
        if(pending.Length>0){
            if(!worker.ApplyInventoryGains(target,generation,pending)){Status="Waiting for the list worker…";return;}
            pending=[];Status="Watching inventory gains for the active list.";
        }
        if(!dirty||Environment.TickCount64<notBefore)return;
        if(!TryTotals(out var totals)||Plugin.Player.ContentId!=character){
            progress.Reset();candidate=null;notBefore=Environment.TickCount64+500;
            Status="Waiting for complete player inventory.";return;
        }
        if(candidate is null||candidate.Count!=totals.Count||candidate.Any(pair=>totals.GetValueOrDefault(pair.Key)!=pair.Value)){
            candidate=totals;notBefore=Environment.TickCount64+250;return;
        }
        pending=progress.Observe(target,character,totals,true);
        candidate=null;dirty=false;Status="Watching inventory gains for the active list.";
    }
    private static bool TryTotals(out Dictionary<InventoryKey,long> totals)
    {
        totals=[];
        var manager=InventoryManager.Instance();
        if(manager==null)return false;
        foreach(var type in Containers){
            // The SDK's span API alone does not check InventoryContainer.IsLoaded.
            // Match the packaged client structs and check completeness before copying.
            var container=manager->GetInventoryContainer((InventoryType)type);
            if(container==null||!container->IsLoaded||container->Items==null)return false;
            var items=Plugin.Inventory.GetInventoryItems(type);
            if(items.Length==0||items.Length>512||items.Length!=container->Size)return false;
            foreach(ref readonly var item in items){
                if(item.IsEmpty||item.IsCollectable)continue;
                var id=item.BaseItemId;var quantity=item.Quantity;
                if(id is 0 or >int.MaxValue||quantity<0)return false;
                if(quantity==0)continue;
                var key=new InventoryKey((int)id,item.IsHq);
                totals[key]=checked(totals.GetValueOrDefault(key)+quantity);
            }
        }
        return true;
    }
    public void Dispose(){Plugin.Inventory.InventoryChanged-=Changed;Plugin.Framework.Update-=Update;Reset("Automatic inventory progress is off.");}
}
