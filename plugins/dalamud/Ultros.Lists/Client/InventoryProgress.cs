using Ultros.Lists;
namespace Ultros.Client;

public readonly record struct InventoryKey(int ItemId, bool Hq);
public sealed record InventoryGain(int ItemId, bool Hq, long Quantity);
public sealed record AcquiredDelta(int ItemId, string Quality, long Quantity);

/// <summary>Compare complete aggregate snapshots, never individual slot events.</summary>
public sealed class InventoryProgress
{
    private ListEditTarget? destination;
    private ulong character;
    private Dictionary<InventoryKey,long>? previous;
    public void Reset() { destination=null; character=0; previous=null; }
    public InventoryGain[] Observe(ListEditTarget target, ulong characterId, IReadOnlyDictionary<InventoryKey,long> totals, bool complete)
    {
        if(!complete || characterId==0 || totals.Count>4096 || totals.Any(pair=>pair.Key.ItemId<=0 || pair.Value<0)) { Reset();return []; }
        var current=new Dictionary<InventoryKey,long>(totals);
        var baseline=previous;
        previous=current;
        if(baseline is null || destination!=target || character!=characterId) { destination=target;character=characterId;return []; }
        return current.Where(pair=>pair.Value>baseline.GetValueOrDefault(pair.Key))
            .Select(pair=>new InventoryGain(pair.Key.ItemId,pair.Key.Hq,pair.Value-baseline.GetValueOrDefault(pair.Key))).ToArray();
    }

    public static AcquiredDelta? ClampToRemaining(IEnumerable<CartRow> rows, AcquiredDelta delta)
    {
        var row=rows.FirstOrDefault(row=>row.ItemId==delta.ItemId&&row.Quality==delta.Quality);
        if(row is null||delta.Quantity<=0)return null;
        var need=Math.Max(0,row.Need);
        var quantity=Math.Min(delta.Quantity,need-Math.Clamp(row.Acquired,0,need));
        return quantity>0?delta with{Quantity=quantity}:null;
    }

    public static AcquiredDelta[] Allocate(IEnumerable<CartRow> rows, IEnumerable<InventoryGain> gains)
    {
        var remaining=new Dictionary<(int ItemId,string Quality),long>();
        foreach(var row in rows.Where(row=>row.ItemId>0 && row.Quality is "hq" or "nq" or "any")) {
            var need=Math.Max(0,row.Need);
            remaining[(row.ItemId,row.Quality)]=need-Math.Clamp(row.Acquired,0,need);
        }
        var deltas=new Dictionary<(int ItemId,string Quality),long>();
        foreach(var gain in gains.Where(gain=>gain.ItemId>0 && gain.Quantity>0)) {
            var available=gain.Quantity;
            foreach(var quality in new[]{gain.Hq?"hq":"nq","any"}) {
                var key=(gain.ItemId,quality);
                var quantity=Math.Min(available,remaining.GetValueOrDefault(key));
                if(quantity==0)continue;
                remaining[key]-=quantity;available-=quantity;
                deltas[key]=checked(deltas.GetValueOrDefault(key)+quantity);
            }
        }
        return deltas.Select(pair=>new AcquiredDelta(pair.Key.ItemId,pair.Key.Quality,pair.Value)).ToArray();
    }
}

/// <summary>Resume a bounded batch after persistence failure without replaying mutated deltas.</summary>
internal sealed class InventoryBatch(AcquiredDelta[] deltas)
{
    private int cursor;
    private bool needsSave;
    internal long Applied { get; private set; }
    internal bool Complete => cursor == deltas.Length && !needsSave;
    internal bool AwaitingPersist => needsSave;
    internal void Resume(Action<AcquiredDelta> mutate, Action save, Func<bool> valid,
        Func<AcquiredDelta,AcquiredDelta?>? revalidate=null)
    {
        if(needsSave){save();needsSave=false;}
        while(cursor<deltas.Length&&valid()) {
            var original=deltas[cursor];
            var delta=revalidate is null?original:revalidate(original);
            // Intervening synchronization can remove or finish an unattempted row.
            if(delta is null){cursor++;continue;}
            mutate(delta);
            // Mutation succeeded even if its following disk write fails.
            cursor++;Applied+=delta.Quantity;needsSave=true;
            save();needsSave=false;
        }
    }
}
