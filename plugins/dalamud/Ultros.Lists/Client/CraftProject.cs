using Ultros.Lists;

namespace Ultros.Client;

public sealed record RecipeIngredient(int ItemId, long Amount);

public sealed record RecipeInfo(
    int RecipeId,
    int OutputItemId,
    long YieldAmount,
    string JobAbbr,
    int RecipeLevel,
    RecipeIngredient[] Ingredients);

public interface ICraftRecipeSource
{
    RecipeInfo? GetRecipe(int recipeId);
    RecipeInfo? GetDefaultRecipeForItem(int itemId);
    bool IsCrystal(int itemId);
}

public enum CraftStepReadinessKind
{
    Done,
    Ready,
    NeedMaterials,
    Buying,
}

public readonly record struct CraftStepReadiness(
    CraftStepReadinessKind Kind,
    long CraftableNow = 0,
    int MissingKinds = 0);

public sealed record CraftStepIngredient(
    int ItemId,
    long PerCraft,
    long NeededTotal,
    long NeededRemaining,
    long Available,
    bool IsCrystal,
    bool IsCraftedStep);

public sealed record CraftConsumer(int RecipeId, int ItemId, long UnitsUsed);

public sealed record CraftTargetPlan(
    int RecipeId,
    int ItemId,
    string JobAbbr,
    int RecipeLevel,
    long Count,
    long Completed,
    long Crafts,
    long YieldAmount,
    long Surplus,
    long CompletedCrafts,
    CraftStepReadiness Readiness,
    CraftStepIngredient[] Ingredients);

public sealed record CraftStepPlan(
    int StepNumber,
    int ItemId,
    int RecipeId,
    string JobAbbr,
    int RecipeLevel,
    long Needed,
    long OnHand,
    long Remaining,
    long Crafts,
    long YieldAmount,
    long Surplus,
    long Crafted,
    string Mode,
    CraftStepReadiness Readiness,
    CraftConsumer[] Consumers,
    CraftStepIngredient[] Ingredients);

public sealed record CraftProjectPlan(
    bool HasTargets,
    string? Error,
    bool ExcludeCrystals,
    bool IncludeVendors,
    CraftTargetPlan[] Targets,
    CraftStepPlan[] Steps,
    CartRow[] ManualRows,
    CartRow[] ConsolidatedRows,
    int StepsDone,
    int StepsTotal,
    int TargetsDone,
    int TargetsTotal,
    long LeavesAcquired,
    long LeavesNeeded,
    IReadOnlyDictionary<int, long> ConsumedByItem);

public static class CraftProject
{
    private const int MaxDepth = 12;
    private const int MaxNodes = 512;
    private const int MaxTargets = 64;
    private const long MaxQuantity = 1_000_000_000;

    private sealed record ExpansionMaterial(
        int ItemId,
        long Needed,
        long Owned,
        long Crafts,
        long Surplus,
        int Depth,
        int? RecipeId)
    {
        public long Remaining => Needed - Owned;
    }

    public static bool IsDerivedOnlyRow(CartView cart, CartRow row) =>
        cart.Targets.Length > 0
        && !cart.Rows.Any(r => r.ItemId == row.ItemId && r.Quality == row.Quality && r.Need > 0);

    public static long CompletedCraftsForTarget(long completed, long needed, long crafts, long yieldAmount)
    {
        if (needed <= 0 || crafts <= 0) return Math.Max(0, crafts);
        if (completed >= needed) return crafts;
        return Math.Clamp(Math.Max(0, completed) / Math.Max(1, yieldAmount), 0, crafts);
    }

    public static CraftProjectPlan Build(CartView cart, ICraftRecipeSource? recipes)
    {
        if (cart.Targets.Length == 0)
        {
            var manualOnly = cart.Rows.Where(r => r.Need > 0).ToArray();
            return new(
                false, null, cart.ExcludeCrystals, cart.IncludeVendors,
                [], [], manualOnly, cart.Rows,
                0, 0, 0, 0, 0, 0,
                new Dictionary<int, long>());
        }

        if (recipes is null)
        {
            var manual = cart.Rows.Where(r => r.Need > 0).ToArray();
            var consolidated = cart.Rows.Where(r => r.Need > 0).ToArray();
            return new(
                true, "Game recipe data is not loaded.", cart.ExcludeCrystals, cart.IncludeVendors,
                [], [], manual, consolidated,
                0, 0, 0, cart.Targets.Length, 0, 0,
                new Dictionary<int, long>());
        }

        var choiceMap = cart.CraftChoices.ToDictionary(c => c.ItemId);
        var activeDemands = new SortedDictionary<int, long>();
        foreach (var target in cart.Targets)
        {
            if (target.Count <= 0) continue;
            activeDemands[target.RecipeId] = Math.Clamp(
                activeDemands.GetValueOrDefault(target.RecipeId) + target.Count, 1, 9999);
        }

        if (activeDemands.Count == 0)
        {
            var manual = cart.Rows.Where(r => r.Need > 0).ToArray();
            return new(
                false, null, cart.ExcludeCrystals, cart.IncludeVendors,
                [], [], manual, cart.Rows,
                0, 0, 0, 0, 0, 0,
                new Dictionary<int, long>());
        }

        if (activeDemands.Count > MaxTargets)
        {
            var (mRows, cRows) = ConsolidateRows(cart.Rows, new Dictionary<int, long>());
            return new(
                true, "This crafting project has too many targets. Remove some targets to simplify it.",
                cart.ExcludeCrystals, cart.IncludeVendors,
                [], [], mRows, cRows,
                0, 0, 0, activeDemands.Count, 0, 0,
                new Dictionary<int, long>());
        }

        var recipeCache = new Dictionary<int, RecipeInfo>();
        var queue = new Queue<int>();
        var visited = new HashSet<int>();

        foreach (var (recipeId, _) in activeDemands)
        {
            if (recipes.GetRecipe(recipeId) is { } r)
            {
                recipeCache[r.RecipeId] = r;
                foreach (var ing in r.Ingredients)
                {
                    if (ing.Amount > 0 && visited.Add(ing.ItemId))
                        queue.Enqueue(ing.ItemId);
                }
            }
        }

        var activeChoices = new Dictionary<int, int>();
        var allCraftChoices = new Dictionary<int, int>();
        var owned = new Dictionary<int, long>();
        var excluded = new HashSet<int>();

        while (queue.Count > 0)
        {
            var itemId = queue.Dequeue();
            if (cart.ExcludeCrystals && recipes.IsCrystal(itemId))
            {
                excluded.Add(itemId);
                continue;
            }

            choiceMap.TryGetValue(itemId, out var explicitChoice);
            if (explicitChoice is { OnHand: > 0 })
                owned[itemId] = explicitChoice.OnHand;

            RecipeInfo? chosenRecipe = explicitChoice?.RecipeId is { } rid ? recipes.GetRecipe(rid) : null;
            chosenRecipe ??= recipes.GetDefaultRecipeForItem(itemId);
            if (chosenRecipe is not null)
            {
                allCraftChoices[itemId] = chosenRecipe.RecipeId;
                if (explicitChoice?.Mode != "buy")
                    activeChoices[itemId] = chosenRecipe.RecipeId;

                recipeCache[chosenRecipe.RecipeId] = chosenRecipe;
                foreach (var ing in chosenRecipe.Ingredients)
                {
                    if (ing.Amount > 0 && visited.Add(ing.ItemId))
                        queue.Enqueue(ing.ItemId);
                }
            }
        }

        if (!TryExpand(activeDemands, recipeCache, activeChoices, owned, excluded,
                out var expandedTargets, out var expandedSteps, out var expandedMaterials, out var consumersByItem, out var expandError))
        {
            var (mRows, cRows) = ConsolidateRows(cart.Rows, new Dictionary<int, long>());
            return new(
                true, expandError, cart.ExcludeCrystals, cart.IncludeVendors,
                [], [], mRows, cRows,
                0, 0, 0, activeDemands.Count, 0, 0,
                new Dictionary<int, long>());
        }

        var leafDemands = new Dictionary<int, long>();
        foreach (var mat in expandedMaterials)
        {
            if (mat.Remaining > 0)
                leafDemands[mat.ItemId] = mat.Remaining;
        }

        var (manualRows, consolidatedRows) = ConsolidateRows(cart.Rows, leafDemands);

        var rawAcquired = new Dictionary<int, long>();
        foreach (var row in cart.Rows)
        {
            if (row.Acquired > 0 && row.Quality == "any")
                rawAcquired[row.ItemId] = rawAcquired.GetValueOrDefault(row.ItemId) + row.Acquired;
        }

        long leafAcquiredClamped = 0;
        long leafNeededTotal = 0;
        foreach (var (itemId, need) in leafDemands)
        {
            var boundedNeed = Math.Max(0, need);
            leafNeededTotal += boundedNeed;
            leafAcquiredClamped += Math.Clamp(rawAcquired.GetValueOrDefault(itemId), 0, boundedNeed);
        }

        var targetByRecipe = cart.Targets.ToDictionary(t => t.RecipeId);
        var consumed = new Dictionary<int, long>();

        foreach (var tMat in expandedTargets)
        {
            if (tMat.RecipeId is not { } rid || !targetByRecipe.TryGetValue(rid, out var snap)
                || !recipeCache.TryGetValue(rid, out var recipe))
                continue;
            var compCrafts = CompletedCraftsForTarget(snap.Completed, tMat.Needed, tMat.Crafts, recipe.YieldAmount);
            if (compCrafts <= 0) continue;
            foreach (var ing in recipe.Ingredients)
            {
                if (ing.Amount > 0 && !excluded.Contains(ing.ItemId))
                    consumed[ing.ItemId] = consumed.GetValueOrDefault(ing.ItemId) + compCrafts * ing.Amount;
            }
        }

        foreach (var sMat in expandedSteps)
        {
            if (sMat.RecipeId is not { } rid || !recipeCache.TryGetValue(rid, out var recipe)) continue;
            var crafted = Math.Clamp(choiceMap.TryGetValue(sMat.ItemId, out var c) ? c.Crafted : 0, 0, sMat.Crafts);
            if (crafted <= 0) continue;
            foreach (var ing in recipe.Ingredients)
            {
                if (ing.Amount > 0 && !excluded.Contains(ing.ItemId))
                    consumed[ing.ItemId] = consumed.GetValueOrDefault(ing.ItemId) + crafted * ing.Amount;
            }
        }

        var remainingAcquired = new Dictionary<int, long>();
        foreach (var (itemId, acq) in rawAcquired)
        {
            var rem = Math.Max(0, acq - consumed.GetValueOrDefault(itemId));
            if (rem > 0) remainingAcquired[itemId] = rem;
        }

        var remainingOnHand = new Dictionary<int, long>();
        foreach (var (itemId, onHandAmt) in owned)
            remainingOnHand[itemId] = Math.Max(0, onHandAmt - consumed.GetValueOrDefault(itemId));

        var remainingCraftedUnits = new Dictionary<int, long>();
        foreach (var sMat in expandedSteps)
        {
            if (sMat.RecipeId is not { } rid || !recipeCache.TryGetValue(rid, out var recipe)) continue;
            var yieldAmt = Math.Max(1, recipe.YieldAmount);
            var crafted = Math.Clamp(choiceMap.TryGetValue(sMat.ItemId, out var c) ? c.Crafted : 0, 0, sMat.Crafts);
            var craftedTotal = crafted * yieldAmt;
            var used = consumed.GetValueOrDefault(sMat.ItemId);
            var onHandAmt = owned.GetValueOrDefault(sMat.ItemId);
            var leftoverUsed = Math.Max(0, used - onHandAmt);
            var remCrafted = Math.Max(0, craftedTotal - leftoverUsed);
            if (remCrafted > 0) remainingCraftedUnits[sMat.ItemId] = remCrafted;
        }

        var activeCraftedSet = activeChoices.Keys.ToHashSet();

        CraftStepIngredient[] BuildIngredients(RecipeInfo recipe, long totalCrafts, long remainingCrafts)
        {
            var list = new List<CraftStepIngredient>(recipe.Ingredients.Length);
            foreach (var ing in recipe.Ingredients)
            {
                if (ing.Amount <= 0) continue;
                var isCrystal = recipes.IsCrystal(ing.ItemId);
                var isCraftedStep = activeCraftedSet.Contains(ing.ItemId);
                var neededTotal = cart.ExcludeCrystals && isCrystal ? 0 : totalCrafts * ing.Amount;
                var neededRem = cart.ExcludeCrystals && isCrystal ? 0 : Math.Max(0, remainingCrafts) * ing.Amount;
                var avail = isCraftedStep
                    ? remainingOnHand.GetValueOrDefault(ing.ItemId) + remainingCraftedUnits.GetValueOrDefault(ing.ItemId)
                    : remainingOnHand.GetValueOrDefault(ing.ItemId) + remainingAcquired.GetValueOrDefault(ing.ItemId);
                list.Add(new(ing.ItemId, ing.Amount, neededTotal, neededRem, avail, isCrystal, isCraftedStep));
            }
            return list
                .OrderBy(i => i.IsCrystal ? 1 : 0)
                .ThenBy(i => i.ItemId)
                .ToArray();
        }

        var steps = new List<CraftStepPlan>();
        var stepIndex = 0;

        void AppendStep(ExpansionMaterial mat, string mode)
        {
            var rid = mat.RecipeId ?? (allCraftChoices.TryGetValue(mat.ItemId, out var candId) ? candId : 0);
            if (rid <= 0 || !recipeCache.TryGetValue(rid, out var recipe)) return;
            stepIndex++;
            choiceMap.TryGetValue(mat.ItemId, out var choice);
            var onHandAmt = choice?.OnHand ?? mat.Owned;
            var remaining = Math.Max(0, mat.Remaining);
            var yieldAmt = Math.Max(1, recipe.YieldAmount);
            long crafts, surplus;
            if (mode == "buy")
            {
                crafts = remaining > 0 ? (remaining + yieldAmt - 1) / yieldAmt : 0;
                surplus = Math.Max(0, crafts * yieldAmt - remaining);
            }
            else
            {
                crafts = mat.Crafts;
                surplus = mat.Surplus;
            }
            var crafted = Math.Clamp(choice?.Crafted ?? 0, 0, crafts);
            var remainingCrafts = mode == "buy" ? 0 : Math.Max(0, crafts - crafted);
            var readiness = mode == "buy"
                ? new(CraftStepReadinessKind.Buying)
                : EvaluateReadiness(
                    remaining,
                    crafted * yieldAmt,
                    crafts,
                    yieldAmt,
                    recipe,
                    excluded,
                    activeCraftedSet,
                    remainingAcquired,
                    remainingCraftedUnits,
                    remainingOnHand);

            var consumers = consumersByItem.TryGetValue(mat.ItemId, out var cons) ? cons.ToArray() : [];
            var ings = BuildIngredients(recipe, crafts, remainingCrafts);
            steps.Add(new(
                stepIndex,
                mat.ItemId,
                rid,
                recipe.JobAbbr,
                recipe.RecipeLevel,
                mat.Needed,
                onHandAmt,
                remaining,
                crafts,
                yieldAmt,
                surplus,
                crafted,
                mode,
                readiness,
                consumers,
                ings));
        }

        foreach (var mat in expandedSteps)
            AppendStep(mat, "craft");
        foreach (var mat in expandedMaterials)
            if (mat.RecipeId is null && allCraftChoices.ContainsKey(mat.ItemId))
                AppendStep(mat, "buy");

        var targets = new List<CraftTargetPlan>(expandedTargets.Count);
        foreach (var tMat in expandedTargets)
        {
            if (tMat.RecipeId is not { } rid || !recipeCache.TryGetValue(rid, out var recipe)) continue;
            var completed = targetByRecipe.TryGetValue(rid, out var snap) ? snap.Completed : 0;
            var yieldAmt = Math.Max(1, recipe.YieldAmount);
            var compCrafts = CompletedCraftsForTarget(completed, tMat.Needed, tMat.Crafts, yieldAmt);
            var remCrafts = Math.Max(0, tMat.Crafts - compCrafts);
            var readiness = EvaluateReadiness(
                tMat.Needed,
                completed,
                tMat.Crafts,
                yieldAmt,
                recipe,
                excluded,
                activeCraftedSet,
                remainingAcquired,
                remainingCraftedUnits,
                remainingOnHand);
            targets.Add(new(
                rid,
                recipe.OutputItemId,
                recipe.JobAbbr,
                recipe.RecipeLevel,
                tMat.Needed,
                completed,
                tMat.Crafts,
                yieldAmt,
                tMat.Surplus,
                compCrafts,
                readiness,
                BuildIngredients(recipe, tMat.Crafts, remCrafts)));
        }

        var craftedStepsOnly = steps.Where(s => s.Mode != "buy").ToArray();
        var stepsDone = craftedStepsOnly.Count(s => s.Readiness.Kind == CraftStepReadinessKind.Done);
        var targetsDone = targets.Count(t => t.Readiness.Kind == CraftStepReadinessKind.Done);

        return new(
            true,
            null,
            cart.ExcludeCrystals,
            cart.IncludeVendors,
            targets.ToArray(),
            steps.ToArray(),
            manualRows,
            consolidatedRows,
            stepsDone,
            craftedStepsOnly.Length,
            targetsDone,
            targets.Count,
            leafAcquiredClamped,
            leafNeededTotal,
            consumed);
    }

    private static (CartRow[] ManualRows, CartRow[] ConsolidatedRows) ConsolidateRows(
        CartRow[] rawRows,
        IReadOnlyDictionary<int, long> leafDemands)
    {
        var remainingLeaves = new Dictionary<int, long>(leafDemands);
        var manual = new List<CartRow>(rawRows.Length);
        var merged = new List<CartRow>(rawRows.Length + leafDemands.Count);

        foreach (var row in rawRows)
        {
            if (row.Need > 0) manual.Add(row);
            long extra = 0;
            if (row.Quality == "any" && remainingLeaves.Remove(row.ItemId, out var leafNeed))
                extra = leafNeed;
            var combinedNeed = Math.Max(0, row.Need) + extra;
            if (combinedNeed > 0)
                merged.Add(row with { Need = combinedNeed });
        }

        foreach (var (itemId, need) in remainingLeaves)
        {
            if (need > 0)
                merged.Add(new(itemId, "any", need, 0, null));
        }

        merged.Sort((a, b) =>
        {
            var cmp = a.ItemId.CompareTo(b.ItemId);
            return cmp != 0 ? cmp : QualityOrder(a.Quality).CompareTo(QualityOrder(b.Quality));
        });

        return (manual.ToArray(), merged.ToArray());
    }

    private static int QualityOrder(string quality) => quality switch
    {
        "any" => 0,
        "hq" => 1,
        "nq" => 2,
        _ => 3,
    };

    private static CraftStepReadiness EvaluateReadiness(
        long needed,
        long completed,
        long crafts,
        long yieldAmount,
        RecipeInfo recipe,
        HashSet<int> excluded,
        HashSet<int> craftedChoices,
        Dictionary<int, long> remainingAcquired,
        Dictionary<int, long> remainingCraftedUnits,
        Dictionary<int, long> remainingOnHand)
    {
        if (needed <= 0 || completed >= needed || crafts <= 0)
            return new(CraftStepReadinessKind.Done);

        var yield = Math.Max(1, yieldAmount);
        var remainingCrafts = (Math.Max(0, needed - completed) + yield - 1) / yield;
        var craftableNow = remainingCrafts;
        var missingKinds = 0;

        foreach (var ing in recipe.Ingredients)
        {
            if (ing.Amount <= 0 || excluded.Contains(ing.ItemId)) continue;
            var avail = craftedChoices.Contains(ing.ItemId)
                ? Math.Max(0, remainingOnHand.GetValueOrDefault(ing.ItemId)) + Math.Max(0, remainingCraftedUnits.GetValueOrDefault(ing.ItemId))
                : Math.Max(0, remainingOnHand.GetValueOrDefault(ing.ItemId)) + Math.Max(0, remainingAcquired.GetValueOrDefault(ing.ItemId));
            var canCover = avail / ing.Amount;
            if (canCover == 0) missingKinds++;
            craftableNow = Math.Min(craftableNow, canCover);
        }

        return missingKinds > 0 || craftableNow == 0
            ? new(CraftStepReadinessKind.NeedMaterials, 0, missingKinds)
            : new(CraftStepReadinessKind.Ready, craftableNow, 0);
    }

    private static bool TryExpand(
        SortedDictionary<int, long> activeTargets,
        Dictionary<int, RecipeInfo> recipes,
        Dictionary<int, int> choices,
        Dictionary<int, long> owned,
        HashSet<int> excluded,
        out List<ExpansionMaterial> targets,
        out List<ExpansionMaterial> steps,
        out List<ExpansionMaterial> materials,
        out Dictionary<int, List<CraftConsumer>> consumers,
        out string? error)
    {
        targets = [];
        steps = [];
        materials = [];
        consumers = [];
        error = null;

        var rootRecipes = new List<(int RecipeId, RecipeInfo Recipe, long Count)>(activeTargets.Count);
        var rootDemands = new SortedDictionary<int, long>();
        var consumerParents = new Dictionary<int, SortedDictionary<(int RecipeId, int ItemId), long>>();

        foreach (var (recipeId, count) in activeTargets)
        {
            if (!recipes.TryGetValue(recipeId, out var recipe) || recipe.YieldAmount <= 0)
            {
                error = "Recipe unavailable for one of this list's targets.";
                return false;
            }
            rootRecipes.Add((recipeId, recipe, count));
        }

        foreach (var (recipeId, recipe, count) in rootRecipes)
        {
            var crafts = (count + recipe.YieldAmount - 1) / recipe.YieldAmount;
            var surplus = crafts * recipe.YieldAmount - count;
            targets.Add(new(recipe.OutputItemId, count, 0, crafts, surplus, 0, recipeId));

            foreach (var ing in recipe.Ingredients)
            {
                if (ing.Amount <= 0 || excluded.Contains(ing.ItemId)) continue;
                var added = crafts * ing.Amount;
                var next = rootDemands.GetValueOrDefault(ing.ItemId) + added;
                if (next > MaxQuantity) { error = "Craft quantity overflow."; return false; }
                rootDemands[ing.ItemId] = next;
                AddConsumer(consumerParents, ing.ItemId, recipeId, recipe.OutputItemId, added);
            }
        }

        var active = new HashSet<int>();
        var seen = new HashSet<int>();
        var order = new List<(int ItemId, int Depth)>();

        bool Visit(int item, int depth, out string? visitError)
        {
            if (active.Contains(item))
            {
                visitError = "These craft choices form a cycle. Choose Buy for one of the ingredients.";
                return false;
            }
            if (seen.Contains(item)) { visitError = null; return true; }
            if (depth > MaxDepth || seen.Count >= MaxNodes)
            {
                visitError = "This crafting plan is too large. Buy some intermediates to simplify it.";
                return false;
            }

            active.Add(item);
            if (choices.TryGetValue(item, out var cid))
            {
                if (!recipes.TryGetValue(cid, out var r) || r.OutputItemId != item || r.YieldAmount <= 0)
                {
                    visitError = "Invalid or missing intermediate recipe.";
                    return false;
                }
                foreach (var ing in r.Ingredients)
                {
                    if (ing.Amount > 0 && !excluded.Contains(ing.ItemId))
                        if (!Visit(ing.ItemId, depth + 1, out visitError))
                            return false;
                }
            }
            active.Remove(item);
            seen.Add(item);
            order.Add((item, depth));
            visitError = null;
            return true;
        }

        foreach (var child in rootDemands.Keys)
        {
            if (!Visit(child, 1, out error))
                return false;
        }

        var demands = new Dictionary<int, long>(rootDemands);
        var nodeMap = new SortedDictionary<int, ExpansionMaterial>();
        var postorder = order.Select(x => x.ItemId).ToArray();

        for (var i = order.Count - 1; i >= 0; i--)
        {
            var (item, depth) = order[i];
            var needed = demands.GetValueOrDefault(item);
            if (needed <= 0) continue;
            var ownedAmt = Math.Clamp(owned.GetValueOrDefault(item), 0, needed);
            if (choices.TryGetValue(item, out var cid))
            {
                var recipe = recipes[cid];
                var remaining = needed - ownedAmt;
                var crafts = remaining > 0 ? (remaining + recipe.YieldAmount - 1) / recipe.YieldAmount : 0;
                var surplus = crafts * recipe.YieldAmount - remaining;
                nodeMap[item] = new(item, needed, ownedAmt, crafts, surplus, depth, cid);

                foreach (var ing in recipe.Ingredients)
                {
                    if (ing.Amount <= 0 || excluded.Contains(ing.ItemId)) continue;
                    if (ing.ItemId == item)
                    {
                        error = "These craft choices form a cycle. Choose Buy for one of the ingredients.";
                        return false;
                    }
                    var added = crafts * ing.Amount;
                    if (added <= 0) continue;
                    var next = demands.GetValueOrDefault(ing.ItemId) + added;
                    if (next > MaxQuantity) { error = "Craft quantity overflow."; return false; }
                    demands[ing.ItemId] = next;
                    AddConsumer(consumerParents, ing.ItemId, cid, item, added);
                }
            }
            else
            {
                nodeMap[item] = new(item, needed, ownedAmt, 0, 0, depth, null);
            }
        }

        foreach (var item in postorder)
        {
            if (nodeMap.TryGetValue(item, out var line) && line.RecipeId is not null)
                steps.Add(line);
        }

        foreach (var line in nodeMap.Values)
        {
            if (line.RecipeId is null && line.Remaining > 0)
                materials.Add(line);
        }

        foreach (var (item, parents) in consumerParents)
        {
            consumers[item] = parents
                .Select(kv => new CraftConsumer(kv.Key.RecipeId, kv.Key.ItemId, kv.Value))
                .ToList();
        }

        return true;
    }

    private static void AddConsumer(
        Dictionary<int, SortedDictionary<(int RecipeId, int ItemId), long>> map,
        int childItemId,
        int parentRecipeId,
        int parentItemId,
        long unitsUsed)
    {
        if (!map.TryGetValue(childItemId, out var inner))
            map[childItemId] = inner = [];
        var key = (parentRecipeId, parentItemId);
        inner[key] = inner.GetValueOrDefault(key) + unitsUsed;
    }
}
