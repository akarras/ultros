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
