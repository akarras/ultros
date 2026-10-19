using Ultros.Client;
namespace Ultros.Lists.Smoke;

internal static class InventoryProgressChecks
{
    private static void Require(bool condition,string message){if(!condition)throw new Exception(message);}
    private static readonly CartRow[] Rows=[new(8,"hq",3,0,null),new(8,"nq",2,0,null),new(8,"any",5,0,null)];
    private static readonly InventoryGain[] Gains=[new(8,true,4),new(8,false,3)];
    internal static void Run()
    {
        var target=new ListEditTarget("account","device",1);
        var progress=new InventoryProgress();
        static Dictionary<InventoryKey,long> Totals(long nq,long hq=0)=>new(){[new(8,false)]=nq,[new(8,true)]=hq};
        Require(progress.Observe(target,1,Totals(10),true).Length==0,"Initial inventory counted.");
        Require(progress.Observe(target,1,Totals(10),true).Length==0,"Moving/sorting stacks counted.");
        Require(progress.Observe(target,1,Totals(7),true).Length==0,"Consumption changed acquired progress.");
        Require(progress.Observe(target,1,Totals(9),true).Single().Quantity==2,"Positive net gain incorrect.");
        Require(progress.Observe(target with{ListId=2},1,Totals(99),true).Length==0,"List switch counted existing inventory.");
        Require(progress.Observe(target,2,Totals(999),true).Length==0,"Character switch counted existing inventory.");
        Require(progress.Observe(target,2,Totals(0),false).Length==0,"Unloaded inventory was compared.");
        Require(progress.Observe(target,2,Totals(1000),true).Length==0,"Reload counted existing inventory.");
        progress.Reset();
        Require(progress.Observe(target,2,Totals(1001),true).Length==0,"Re-enable counted existing inventory.");
        var deltas=InventoryProgress.Allocate(Rows,Gains);
        Require(deltas.Single(row=>row.Quality=="hq").Quantity==3,"HQ row was not filled first.");
        Require(deltas.Single(row=>row.Quality=="nq").Quantity==2,"NQ row was not filled first.");
        Require(deltas.Single(row=>row.Quality=="any").Quantity==2,"Any-quality row double counted gains.");
        Require(InventoryProgress.Allocate([new(8,"hq",1,1,null),new(8,"any",2,1,null)],[new(8,true,100)]).Single().Quantity==1,"Completed/remaining amounts ignored.");
        Require(InventoryProgress.Allocate([new(8,"nq",4,0,null)],[new(8,true,4)]).Length==0,"HQ filled an explicitly NQ row.");
    }
    internal static void NativeDeltas()
    {
        using var document=Document.Open();
        foreach(var row in Rows)document.Edit(new{kind="add",item_id=row.ItemId,quality=row.Quality,need=row.Need});
        using var concurrent=Document.Open(document.Export());
        concurrent.Edit(new{kind="add_acquired",item_id=8,quality="hq",delta=1});
        foreach(var delta in InventoryProgress.Allocate(Rows,Gains))
            document.Edit(new{kind="add_acquired",item_id=delta.ItemId,quality=delta.Quality,delta=delta.Quantity});
        document.Import(concurrent.Export());document.Import(concurrent.Export());
        Require(document.View.Rows.Single(row=>row.Quality=="hq").Acquired==4,"Inventory progress overwrote or replayed the other peer's acquisition.");
    }
}
