using Ultros.Lists;
namespace Ultros.Client;

public sealed record CartProgress(long TotalNeed, long TotalAcquired, int CompletedRows, int TotalRows)
{
    public long RemainingUnits => Math.Max(0, TotalNeed - TotalAcquired);
    public float Fraction => TotalNeed <= 0 ? (TotalRows > 0 ? 1f : 0f) : Math.Clamp((float)TotalAcquired / TotalNeed, 0f, 1f);
}

public sealed record ShoppingStopItem(
    int ItemId,
    string RowQuality,
    bool ListingHq,
    int UnitsToBuy,
    long MinUnitPrice,
    long MaxUnitPrice,
    long Subtotal,
    long RowNeed,
    long RowAcquired);

public sealed record ShoppingWorldStop(
    int WorldId,
    long TotalGil,
    long TotalUnits,
    ShoppingStopItem[] Items);

public sealed record ShoppingRoutePlan(
    ShoppingWorldStop[] Stops,
    CartRow[] UnpricedRows,
    long TotalGil,
    long TotalUnits);

public static class ShoppingRoute
{
    public static CartProgress Progress(CartRow[] rows)
    {
        long totalNeed = 0, totalAcquired = 0;
        var completed = 0;
        foreach (var row in rows)
        {
            var need = Math.Max(1, row.Need);
            var acquired = Math.Clamp(row.Acquired, 0, need);
            totalNeed += need;
            totalAcquired += acquired;
            if (acquired >= need) completed++;
        }
        return new(totalNeed, totalAcquired, completed, rows.Length);
    }

    public static bool? QualityToHq(string quality) => quality switch
    {
        "hq" => true,
        "nq" => false,
        _ => null,
    };

    public static EstimateLine? FindLine(CartEstimate? estimate, CartRow row)
    {
        if (estimate is null) return null;
        var hq = QualityToHq(row.Quality);
        return estimate.Lines.FirstOrDefault(l => l.ItemId == row.ItemId && l.Hq == hq);
    }

    public static ShoppingRoutePlan Build(CartRow[] rows, CartEstimate? estimate, int currentWorldId = 0)
    {
        if (estimate is null || rows.Length == 0)
            return new([], rows.Where(r => r.Acquired < Math.Max(1, r.Need)).ToArray(), 0, 0);

        var byWorld = new Dictionary<int, Dictionary<(int ItemId, string RowQuality, bool ListingHq), (int Units, long Min, long Max, long Subtotal, CartRow Row)>>();
        var unpriced = new List<CartRow>();

        foreach (var row in rows)
        {
            var need = Math.Max(1, row.Need);
            if (row.Acquired >= need) continue;
            var line = FindLine(estimate, row);
            if (line is null || line.Allocations is not { Length: > 0 })
            {
                unpriced.Add(row);
                continue;
            }
            if (line.Status is "PartialSupply" or "NoSupply" or "NotRequested")
                unpriced.Add(row);

            foreach (var alloc in line.Allocations)
            {
                if (alloc.Units <= 0 || alloc.WorldId <= 0) continue;
                if (!byWorld.TryGetValue(alloc.WorldId, out var items))
                    byWorld[alloc.WorldId] = items = [];
                var key = (row.ItemId, row.Quality, alloc.Hq);
                var cost = alloc.PricePerUnit * alloc.Units;
                if (items.TryGetValue(key, out var existing))
                {
                    items[key] = (
                        existing.Units + alloc.Units,
                        Math.Min(existing.Min, alloc.PricePerUnit),
                        Math.Max(existing.Max, alloc.PricePerUnit),
                        existing.Subtotal + cost,
                        row);
                }
                else
                {
                    items[key] = (alloc.Units, alloc.PricePerUnit, alloc.PricePerUnit, cost, row);
                }
            }
        }

        var stops = new List<ShoppingWorldStop>();
        foreach (var (worldId, items) in byWorld)
        {
            var stopItems = items
                .Select(kv => new ShoppingStopItem(
                    kv.Key.ItemId,
                    kv.Key.RowQuality,
                    kv.Key.ListingHq,
                    kv.Value.Units,
                    kv.Value.Min,
                    kv.Value.Max,
                    kv.Value.Subtotal,
                    kv.Value.Row.Need,
                    kv.Value.Row.Acquired))
                .OrderByDescending(i => i.Subtotal)
                .ThenBy(i => i.ItemId)
                .ToArray();
            long gil = 0, units = 0;
            foreach (var item in stopItems) { gil += item.Subtotal; units += item.UnitsToBuy; }
            stops.Add(new(worldId, gil, units, stopItems));
        }

        var worldRank = (estimate.WorldTotals ?? [])
            .Select((w, idx) => (w.WorldId, idx))
            .DistinctBy(x => x.WorldId)
            .ToDictionary(x => x.WorldId, x => x.idx);

        var orderedStops = stops
            .OrderByDescending(s => currentWorldId > 0 && s.WorldId == currentWorldId)
            .ThenBy(s => worldRank.TryGetValue(s.WorldId, out var rank) ? rank : int.MaxValue)
            .ThenByDescending(s => s.TotalGil)
            .ThenBy(s => s.WorldId)
            .ToArray();

        long totalGil = 0, totalUnits = 0;
        foreach (var stop in orderedStops) { totalGil += stop.TotalGil; totalUnits += stop.TotalUnits; }

        return new(orderedStops, unpriced.ToArray(), totalGil, totalUnits);
    }
}
