using Ultros.Lists;
namespace Ultros.Client;

public readonly record struct InventoryKey(int ItemId, bool Hq);
public sealed record InventoryGain(int ItemId, bool Hq, long Quantity);
public sealed record AcquiredDelta(int ItemId, string Quality, long Quantity);
public enum ProgressDeltaKind { Acquired, TargetCompleted, IntermediateCrafted }
public sealed record ProgressDelta(ProgressDeltaKind Kind, int ItemId, string Quality, int RecipeId, long Quantity);
public sealed record IntermediateOnHandUpdate(int ItemId, long OnHand);
public sealed record RowAcquiredUpdate(int ItemId, string Quality, long Acquired);

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

    public static ProgressDelta? ClampProjectDelta(CartView cart, CraftProjectPlan? plan, ProgressDelta delta)
    {
        if (delta.Quantity <= 0) return null;
        switch (delta.Kind)
        {
            case ProgressDeltaKind.TargetCompleted:
                var target = cart.Targets.FirstOrDefault(t => t.RecipeId == delta.RecipeId);
                if (target is null) return null;
                var need = Math.Max(0, target.Count);
                var remTarget = need - Math.Clamp(target.Completed, 0, need);
                var qtyTarget = Math.Min(delta.Quantity, remTarget);
                return qtyTarget > 0 ? delta with { Quantity = qtyTarget } : null;

            case ProgressDeltaKind.IntermediateCrafted:
                if (plan is { HasTargets: true })
                {
                    var step = plan.Steps.FirstOrDefault(s => s.ItemId == delta.ItemId && s.Mode == "craft");
                    if (step is null || step.Crafts <= 0) return null;
                    var remStep = step.Crafts - Math.Clamp(step.Crafted, 0, step.Crafts);
                    var qtyStep = Math.Min(delta.Quantity, remStep);
                    return qtyStep > 0 ? delta with { Quantity = qtyStep } : null;
                }
                return null;

            case ProgressDeltaKind.Acquired:
                var rows = plan is { HasTargets: true } ? plan.ConsolidatedRows : cart.Rows;
                var clamped = ClampToRemaining(rows, new(delta.ItemId, delta.Quality, delta.Quantity));
                return clamped is null ? null : delta with { Quantity = clamped.Quantity };

            default:
                return null;
        }
    }

    public static ProgressDelta[] AllocateProject(CartView cart, CraftProjectPlan? plan, IEnumerable<InventoryGain> gains)
    {
        if (plan is not { HasTargets: true })
            return Allocate(cart.Rows, gains)
                .Select(d => new ProgressDelta(ProgressDeltaKind.Acquired, d.ItemId, d.Quality, 0, d.Quantity))
                .ToArray();

        var remainingTargets = new Dictionary<int, long>();
        foreach (var t in plan.Targets)
        {
            var rem = Math.Max(0, t.Count - Math.Max(0, t.Completed));
            if (rem > 0) remainingTargets[t.RecipeId] = rem;
        }

        var remainingStepCrafts = new Dictionary<int, (long Crafts, long Yield)>();
        foreach (var s in plan.Steps)
        {
            if (s.Mode != "craft" || s.Crafts <= 0) continue;
            var rem = Math.Max(0, s.Crafts - Math.Max(0, s.Crafted));
            if (rem > 0) remainingStepCrafts[s.ItemId] = (rem, Math.Max(1, s.YieldAmount));
        }

        var targetDeltas = new Dictionary<(int ItemId, int RecipeId), long>();
        var stepDeltas = new Dictionary<int, long>();
        var rowGains = new List<InventoryGain>();

        foreach (var gain in gains.Where(g => g.ItemId > 0 && g.Quantity > 0))
        {
            var available = gain.Quantity;

            foreach (var t in plan.Targets)
            {
                if (available <= 0) break;
                if (t.ItemId != gain.ItemId) continue;
                var rem = remainingTargets.GetValueOrDefault(t.RecipeId);
                var qty = Math.Min(available, rem);
                if (qty <= 0) continue;
                remainingTargets[t.RecipeId] = rem - qty;
                available -= qty;
                var key = (t.ItemId, t.RecipeId);
                targetDeltas[key] = checked(targetDeltas.GetValueOrDefault(key) + qty);
            }

            if (available > 0 && remainingStepCrafts.TryGetValue(gain.ItemId, out var stepInfo) && stepInfo.Crafts > 0)
            {
                var craftsGained = Math.Min(stepInfo.Crafts, available / stepInfo.Yield);
                if (craftsGained > 0)
                {
                    remainingStepCrafts[gain.ItemId] = (stepInfo.Crafts - craftsGained, stepInfo.Yield);
                    available -= craftsGained * stepInfo.Yield;
                    stepDeltas[gain.ItemId] = checked(stepDeltas.GetValueOrDefault(gain.ItemId) + craftsGained);
                }
            }

            if (available > 0)
                rowGains.Add(gain with { Quantity = available });
        }

        var output = new List<ProgressDelta>(targetDeltas.Count + stepDeltas.Count + rowGains.Count);
        foreach (var (key, qty) in targetDeltas.OrderBy(kv => kv.Key.RecipeId))
            output.Add(new(ProgressDeltaKind.TargetCompleted, key.ItemId, "", key.RecipeId, qty));
        foreach (var (itemId, crafts) in stepDeltas.OrderBy(kv => kv.Key))
            output.Add(new(ProgressDeltaKind.IntermediateCrafted, itemId, "", 0, crafts));
        foreach (var acq in Allocate(plan.ConsolidatedRows, rowGains))
            output.Add(new(ProgressDeltaKind.Acquired, acq.ItemId, acq.Quality, 0, acq.Quantity));
        return output.ToArray();
    }

    public static IntermediateOnHandUpdate[] PlanBagStockPhase1Intermediates(
        CraftProjectPlan plan,
        IReadOnlyDictionary<InventoryKey, long> bagTotals)
    {
        if (!plan.HasTargets) return [];
        var bagByItem = new Dictionary<int, long>();
        foreach (var (key, qty) in bagTotals)
        {
            if (key.ItemId > 0 && qty > 0)
                bagByItem[key.ItemId] = checked(bagByItem.GetValueOrDefault(key.ItemId) + qty);
        }

        var updates = new List<IntermediateOnHandUpdate>();
        foreach (var step in plan.Steps)
        {
            if (step.Mode != "craft" || step.Needed <= 0) continue;
            var inBag = bagByItem.GetValueOrDefault(step.ItemId);
            var consumed = plan.ConsumedByItem.GetValueOrDefault(step.ItemId);
            var craftedYield = Math.Max(0, step.Crafted) * Math.Max(1, step.YieldAmount);
            var targetOnHand = Math.Clamp(consumed + inBag - craftedYield, 0, step.Needed);
            if (targetOnHand > step.OnHand)
                updates.Add(new(step.ItemId, targetOnHand));
        }
        return updates.ToArray();
    }

    public static RowAcquiredUpdate[] PlanBagStockPhase2Rows(
        CartRow[] rows,
        IReadOnlyDictionary<int, long> consumedByItem,
        IReadOnlyDictionary<InventoryKey, long> bagTotals)
    {
        var availNq = new Dictionary<int, long>();
        var availHq = new Dictionary<int, long>();
        foreach (var (key, qty) in bagTotals)
        {
            if (key.ItemId <= 0 || qty <= 0) continue;
            if (key.Hq) availHq[key.ItemId] = checked(availHq.GetValueOrDefault(key.ItemId) + qty);
            else availNq[key.ItemId] = checked(availNq.GetValueOrDefault(key.ItemId) + qty);
        }
        var availConsumed = new Dictionary<int, long>(consumedByItem);

        int Priority(string q) => q switch { "hq" => 0, "nq" => 1, _ => 2 };
        var updates = new List<RowAcquiredUpdate>();
        foreach (var group in rows.Where(r => r.ItemId > 0 && r.Need > 0).GroupBy(r => r.ItemId).OrderBy(g => g.Key))
        {
            var itemId = group.Key;
            foreach (var row in group.OrderBy(r => Priority(r.Quality)))
            {
                var need = Math.Max(0, row.Need);
                var hq = availHq.GetValueOrDefault(itemId);
                var nq = availNq.GetValueOrDefault(itemId);
                var cons = availConsumed.GetValueOrDefault(itemId);

                long accountTotal = row.Quality switch
                {
                    "hq" => Math.Min(need, hq),
                    "nq" => Math.Min(need, nq),
                    _ => Math.Min(need, hq + nq + cons),
                };

                if (row.Acquired < need && accountTotal > row.Acquired)
                {
                    updates.Add(new(itemId, row.Quality, accountTotal));
                }

                // Reserve the stock attributable to this row (`max(existing, accountTotal)`)
                // so later "any" rows for the same item never double-count HQ/NQ stacks.
                var claimed = Math.Min(need, Math.Max(row.Acquired, accountTotal));
                if (row.Quality == "hq")
                {
                    var take = Math.Min(hq, claimed);
                    availHq[itemId] = hq - take;
                }
                else if (row.Quality == "nq")
                {
                    var take = Math.Min(nq, claimed);
                    availNq[itemId] = nq - take;
                }
                else
                {
                    var rem = claimed;
                    var takeCons = Math.Min(cons, rem);
                    availConsumed[itemId] = cons - takeCons;
                    rem -= takeCons;
                    var takeNq = Math.Min(nq, rem);
                    availNq[itemId] = nq - takeNq;
                    rem -= takeNq;
                    var takeHq = Math.Min(hq, rem);
                    availHq[itemId] = hq - takeHq;
                }
            }
        }
        return updates.ToArray();
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

/// <summary>Resume a bounded Craft + Shop progress batch after persistence failure without replay.</summary>
internal sealed class ProjectProgressBatch(ProgressDelta[] deltas)
{
    private int cursor;
    private bool needsSave;
    internal long Applied { get; private set; }
    internal int AppliedCraftSteps { get; private set; }
    internal int AppliedUnits { get; private set; }
    internal bool Complete => cursor == deltas.Length && !needsSave;
    internal bool AwaitingPersist => needsSave;
    internal void Resume(Action<ProgressDelta> mutate, Action save, Func<bool> valid,
        Func<ProgressDelta, ProgressDelta?>? revalidate = null)
    {
        if (needsSave) { save(); needsSave = false; }
        while (cursor < deltas.Length && valid())
        {
            var original = deltas[cursor];
            var delta = revalidate is null ? original : revalidate(original);
            if (delta is null) { cursor++; continue; }
            mutate(delta);
            cursor++;
            Applied += delta.Quantity;
            if (delta.Kind == ProgressDeltaKind.Acquired) AppliedUnits += (int)Math.Min(int.MaxValue, delta.Quantity);
            else AppliedCraftSteps++;
            needsSave = true;
            save(); needsSave = false;
        }
    }
}
