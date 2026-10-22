using Ultros.Client;
namespace Ultros.Lists.Smoke;

internal static class InventoryProgressChecks
{
    private static void Require(bool condition,string message){if(!condition)throw new Exception(message);}
    private static readonly CartRow[] Rows=[new(8,"hq",3,0,null),new(8,"nq",2,0,null),new(8,"any",5,0,null)];
    private static readonly InventoryGain[] Gains=[new(8,true,4),new(8,false,3)];
    internal static void Run()
    {
        var attempts=new List<int>();
        var batch=new InventoryBatch([new(8,"nq",2),new(9,"any",3),new(10,"hq",4)]);
        var saves=0;
        void Save(){if(++saves==2)throw new IOException("Fixture disk failure after the second mutation.");}
        try{batch.Resume(delta=>attempts.Add(delta.ItemId),Save,()=>true);throw new Exception("Save failure ignored.");}
        catch(IOException){}
        Require(attempts.SequenceEqual(new[]{8,9})&&batch.Applied==5&&!batch.Complete,"Partial batch did not retain its progress.");
        batch.Resume(delta=>attempts.Add(delta.ItemId),Save,()=>true);
        Require(attempts.SequenceEqual(new[]{8,9,10})&&batch.Applied==9&&batch.Complete,"Retry lost an unattempted delta or replayed an already-mutated delta.");
        var resumedMutations=new List<AcquiredDelta>();
        var staleTail=new InventoryBatch([new(8,"nq",2),new(9,"any",3),new(10,"hq",4),new(11,"nq",5),new(12,"any",2)]);
        var currentRows=new CartRow[]{new(8,"nq",10,0,null),new(9,"any",10,0,null),new(10,"hq",10,0,null),new(11,"nq",10,0,null),new(12,"any",10,0,null)};
        var failSave=true;
        void SaveTail(){if(failSave)throw new IOException("Fixture first checkpoint failure.");}
        AcquiredDelta? Revalidate(AcquiredDelta delta)=>InventoryProgress.ClampToRemaining(currentRows,delta);
        try{staleTail.Resume(resumedMutations.Add,SaveTail,()=>true,Revalidate);throw new Exception("Tail fixture did not pause.");}
        catch(IOException){}
        // A sync saves the first mutation, deletes item 9, completes item 10,
        // and leaves only one unit outstanding on item 11 before retry.
        currentRows=[new(8,"nq",10,2,null),new(10,"hq",4,4,null),new(11,"nq",6,5,null),new(12,"any",10,0,null)];
        failSave=false;
        staleTail.Resume(resumedMutations.Add,SaveTail,()=>true,Revalidate);
        Require(resumedMutations.Select(delta=>delta.ItemId).SequenceEqual(new[]{8,11,12}),"Retry replayed the saved mutation or attempted a deleted/completed tail row.");
        Require(resumedMutations.Single(delta=>delta.ItemId==11).Quantity==1,"Retry exceeded a tail row's current remaining need.");
        Require(staleTail.Complete&&staleTail.Applied==5,"Skipped stale tail rows left the batch reserved or counted unapplied units.");
        var refused=new InventoryBatch([new(8,"nq",2),new(9,"any",3)]);
        var mutationAttempts=0;
        try {
            refused.Resume(_=>{mutationAttempts++;throw new InvalidOperationException("Fixture admission refusal before mutation.");},()=>{},()=>true);
            throw new Exception("Admission refusal ignored.");
        }
        catch(InvalidOperationException){}
        Require(refused.Applied==0&&!refused.AwaitingPersist&&!refused.Complete,"Rejected native mutation advanced the retained cursor.");
        refused.Resume(_=>mutationAttempts++,()=>{},()=>true);
        Require(mutationAttempts==3&&refused.Applied==5&&refused.Complete,"Resuming after admission recovery lost an accepted inventory delta.");
        var cancelled=new InventoryBatch([new(8,"nq",2),new(9,"any",3)]);
        var valid=true;
        cancelled.Resume(_=>valid=false,()=>{},()=>valid);
        Require(cancelled.Applied==2&&!cancelled.Complete,"Context cancellation applied the rest of the inventory batch.");
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
