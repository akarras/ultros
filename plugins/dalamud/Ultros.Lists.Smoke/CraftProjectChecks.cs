using Ultros.Client;
using Ultros.Lists;

namespace Ultros.Lists.Smoke;

internal static class CraftProjectChecks
{
    private static void Require(bool condition, string message)
    {
        if (!condition) throw new Exception(message);
    }

    private sealed class TestRecipeSource : ICraftRecipeSource
    {
        private readonly Dictionary<int, RecipeInfo> recipes = new()
        {
            // Target A (recipe 100): 1x Item 5056 -> 2x Item 200 (Ingot) + 1x Item 201 (Lumber) + 3x Item 8 (Crystal)
            [100] = new(100, 5056, 1, "BSM", 100, [new(200, 2), new(201, 1), new(8, 3)]),
            // Target B (recipe 101): 1x Item 5057 -> 3x Item 200 (Ingot) + 2x Item 8 (Crystal)
            [101] = new(101, 5057, 1, "ARM", 100, [new(200, 3), new(8, 2)]),
            // Shared intermediate 200 (recipe 200, yield 2!): 2x Item 300 (Ore)
            [200] = new(200, 200, 2, "BSM", 98, [new(300, 2)]),
            // Intermediate 201 (recipe 201, yield 1): 1x Item 202 (Sub-step) + 1x Item 301 (Raw log)
            [201] = new(201, 201, 1, "CRP", 98, [new(202, 1), new(301, 1)]),
            // Deep intermediate 202 (recipe 202, yield 1): 2x Item 302 (Twig)
            [202] = new(202, 202, 1, "CRP", 94, [new(302, 2)]),
            // Multi-yield target (recipe 102, yield 3): 1x Item 301
            [102] = new(102, 5058, 3, "ALC", 96, [new(301, 1)]),
        };

        public RecipeInfo? GetRecipe(int recipeId) => recipes.GetValueOrDefault(recipeId);
        public RecipeInfo? GetDefaultRecipeForItem(int itemId) =>
            recipes.Values.Where(r => r.OutputItemId == itemId).OrderBy(r => r.RecipeId).FirstOrDefault();
        public bool IsCrystal(int itemId) => itemId == 8;
    }

    internal static void Run()
    {
        var source = new TestRecipeSource();

        // 1. Multi-recipe consolidation, batch yield rounding, on_hand once across targets, bottom-up order, Used by.
        var cart = new CartView("Raid Gear", "peer-1", false, false,
        [
            new(300, "any", 0, 1, null), // 1/2 per craft -> NeedMaterials; with 2 -> Ready x1 (even though 4 needed total)
            new(302, "any", 0, 2, null),
        ])
        {
            Targets =
            [
                new(100, 1, 0),
                new(101, 1, 0),
            ],
            CraftChoices =
            [
                // Item 200 total needed = 2 + 3 = 5. With 1 on_hand -> 4 remaining -> yield 2 -> 2 crafts -> 4x Item 300 needed.
                new(200, "craft", 200, 1, 0),
            ],
            ExcludeCrystals = true,
        };

        var plan = CraftProject.Build(cart, source);
        Require(plan.HasTargets && plan.Error is null, "CraftProject failed to expand valid multi-recipe plan.");
        Require(plan.Targets.Length == 2, "Expected 2 targets.");

        // Bottom-up order: deepest dependency 202 must come before 201!
        var stepIds = plan.Steps.Select(s => s.ItemId).ToArray();
        Require(Array.IndexOf(stepIds, 202) < Array.IndexOf(stepIds, 201), "Bottom-up step order placed child after parent.");

        var step200 = plan.Steps.Single(s => s.ItemId == 200);
        Require(step200.Needed == 5 && step200.OnHand == 1 && step200.Remaining == 4 && step200.Crafts == 2,
            "Shared intermediate did not deduplicate yield rounding across targets.");
        Require(step200.Consumers.Length == 2, "Shared intermediate missed Used by attribution for both targets.");
        Require(step200.Readiness.Kind == CraftStepReadinessKind.NeedMaterials, "Step 200 should need 1 more ore for even 1 craft.");

        // Partial materials covering 1 craft (2/4 ore) -> Ready (CraftableNow == 1).
        var partialPlan = CraftProject.Build(cart with { Rows = [new(300, "any", 0, 2, null), new(302, "any", 0, 2, null)] }, source);
        var partialStep200 = partialPlan.Steps.Single(s => s.ItemId == 200);
        Require(partialStep200.Readiness.Kind == CraftStepReadinessKind.Ready && partialStep200.Readiness.CraftableNow == 1,
            "Step 200 should report Ready x1 once materials cover at least 1 craft.");

        var step202 = plan.Steps.Single(s => s.ItemId == 202);
        Require(step202.Readiness.Kind == CraftStepReadinessKind.Ready && step202.Readiness.CraftableNow == 1,
            "Step 202 should be ready with 2/2 item 302 acquired.");

        // Crystal item 8 excluded; consolidated leaves: 300 (need 4, acq 1), 301 (need 1), 302 (need 2, acq 2).
        Require(!plan.ConsolidatedRows.Any(r => r.ItemId == 8), "ExcludeCrystals failed to filter crystals.");
        var row300 = plan.ConsolidatedRows.Single(r => r.ItemId == 300);
        Require(row300.Need == 4 && row300.Acquired == 1, "Consolidated leaf missed existing acquired count.");

        // 2. Flipping an intermediate to Buy prunes its child (202) and puts 201 in Buying + ConsolidatedRows.
        var boughtCart = cart with
        {
            CraftChoices = [.. cart.CraftChoices, new(201, "buy", 201, 0, 0)],
        };
        var boughtPlan = CraftProject.Build(boughtCart, source);
        Require(!boughtPlan.Steps.Any(s => s.ItemId == 202), "Buy toggle did not prune sub-craft 202.");
        Require(boughtPlan.Steps.Any(s => s.ItemId == 201 && s.Mode == "buy" && s.Readiness.Kind == CraftStepReadinessKind.Buying),
            "Bought intermediate missing from Steps with Buying readiness.");
        Require(boughtPlan.ConsolidatedRows.Any(r => r.ItemId == 201 && r.Need == 1),
            "Bought intermediate missing from ConsolidatedRows.");

        // 3. Completing child step (202crafted = 1) unblocks parent (201) to Ready and consumes item 302!
        var progressedCart = cart with
        {
            CraftChoices =
            [
                new(200, "craft", 200, 1, 0),
                new(202, "craft", 202, 0, 1),
            ],
            Rows = [new(300, "any", 0, 4, null), new(301, "any", 0, 1, null), new(302, "any", 0, 2, null)],
        };
        var progressedPlan = CraftProject.Build(progressedCart, source);
        Require(progressedPlan.Steps.Single(s => s.ItemId == 202).Readiness.Kind == CraftStepReadinessKind.Done,
            "Completed step 202 did not report Done.");
        Require(progressedPlan.Steps.Single(s => s.ItemId == 201).Readiness.Kind == CraftStepReadinessKind.Ready,
            "Completing child step 202 + buying log 301 did not unlock parent 201.");
        Require(progressedPlan.Steps.Single(s => s.ItemId == 200).Readiness.Kind == CraftStepReadinessKind.Ready,
            "Buying 4x ore 300 did not make step 200 Ready.");

        // 4. Live auto-progress allocates TargetCompleted, IntermediateCrafted (yield > 1!), and leaf Acquired.
        var gainCart = new CartView("Gains", "p1", false, false, [])
        {
            Targets = [new(100, 1, 0)],
            ExcludeCrystals = true,
        };
        var gainPlan = CraftProject.Build(gainCart, source);
        // Crafted: 1x target 5056 (+1 target_completed), 2x yield-2 ingot 200 (+1 intermediate_crafted), and 3x item 301 (only 1 needed -> 1 acquired).
        var deltas = InventoryProgress.AllocateProject(gainCart, gainPlan, [new(5056, true, 1), new(200, false, 2), new(301, false, 3)]);
        Require(deltas.Single(d => d.Kind == ProgressDeltaKind.TargetCompleted && d.RecipeId == 100).Quantity == 1,
            "Inventory gain did not credit completed target recipe.");
        Require(deltas.Single(d => d.Kind == ProgressDeltaKind.IntermediateCrafted && d.ItemId == 200).Quantity == 1,
            "Inventory gain did not convert yield-2 units into 1 intermediate craft.");
        Require(deltas.Single(d => d.Kind == ProgressDeltaKind.Acquired && d.ItemId == 301).Quantity == 1,
            "Inventory gain exceeded consolidated leaf demand or failed to credit leaf.");

        // ClampProjectDelta skips already-completed steps.
        var doneCart = gainCart with { Targets = [new(100, 1, 1)] };
        var donePlan = CraftProject.Build(doneCart, source);
        Require(InventoryProgress.ClampProjectDelta(doneCart, donePlan, new(ProgressDeltaKind.TargetCompleted, 5056, "", 100, 1)) is null,
            "ClampProjectDelta did not stop at target count.");

        // 5. Bag-stock sync Phase 1 + Phase 2 handles existing HQ manual rows without double-counting into consolidated "any" leaves, and is idempotent!
        var bagCart = new CartView("Bag Sync", "p1", false, false, [new(300, "hq", 2, 0, null)])
        {
            Targets = [new(100, 1, 0)], // needs 2x item 200 -> 1 craft -> needs 2x item 300 ("any")
            ExcludeCrystals = true,
        };
        var bagPlan = CraftProject.Build(bagCart, source);
        var bagTotals = new Dictionary<InventoryKey, long>
        {
            [new(200, false)] = 0,
            [new(300, true)] = 2, // should fill manual HQ row first, leaving 0 HQ for "any" leaf
            [new(300, false)] = 1, // should fill 1 of 2 for "any" leaf
        };
        var phase1 = InventoryProgress.PlanBagStockPhase1Intermediates(bagPlan, bagTotals);
        Require(phase1.Length == 0, "Zero bag stock unexpectedly changed intermediate on_hand.");
        var phase2 = InventoryProgress.PlanBagStockPhase2Rows(bagPlan.ConsolidatedRows, bagPlan.ConsumedByItem, bagTotals);
        Require(phase2.Single(u => u.Quality == "hq").Acquired == 2 && phase2.Single(u => u.Quality == "any").Acquired == 1,
            "Bag-stock sync double-counted HQ items between manual HQ row and consolidated leaf.");

        // Repeat with Phase 2 already applied -> must be idempotent (0 updates)!
        var syncedCart = bagCart with
        {
            Rows = [new(300, "hq", 2, 2, null), new(300, "any", 0, 1, null)],
        };
        var syncedPlan = CraftProject.Build(syncedCart, source);
        Require(InventoryProgress.PlanBagStockPhase2Rows(syncedPlan.ConsolidatedRows, syncedPlan.ConsumedByItem, bagTotals).Length == 0,
            "Bag-stock sync was not idempotent.");

        Console.WriteLine("PASS: multi-recipe CraftProject DAG consolidation, Buy pruning, StepReadiness, yield-aware live synth auto-progress, and idempotent bag-stock sync.");
    }

    internal static void NativeSchemaV2()
    {
        using var a = Document.Open();
        a.Edit(new { kind = "add_target", recipe_id = 100, count = 2 });
        a.Edit(new { kind = "set_craft_choice", item_id = 200, mode = "craft", recipe_id = 200 });
        a.Edit(new { kind = "set_intermediate_on_hand", item_id = 200, on_hand = 1 });
        a.Edit(new { kind = "set_craft_options", exclude_crystals = true, include_vendors = false });
        a.Edit(new { kind = "add_acquired_or_create", item_id = 300, quality = "any", delta = 2 });

        Require(a.View.Targets.Single() == new CartTarget(100, 2, 0), "Schema v2 add_target view mismatch.");
        Require(a.View.CraftChoices.Single() == new CartCraftChoice(200, "craft", 200, 1, 0), "Schema v2 craft choice view mismatch.");
        Require(a.View.ExcludeCrystals && !a.View.IncludeVendors, "Schema v2 craft options view mismatch.");

        using var b = Document.Open(a.Export());
        a.Edit(new { kind = "add_target_completed", recipe_id = 100, delta = 1 });
        b.Edit(new { kind = "add_target_completed", recipe_id = 100, delta = 1 });
        a.Edit(new { kind = "add_intermediate_crafted", item_id = 200, delta = 1 });
        b.Edit(new { kind = "add_intermediate_crafted", item_id = 200, delta = 2 });
        a.Edit(new { kind = "add_acquired_or_create", item_id = 300, quality = "any", delta = 1 });
        b.Edit(new { kind = "add_acquired_or_create", item_id = 300, quality = "any", delta = 2 });

        var fromB = b.Export();
        a.Import(fromB);
        a.Import(fromB);
        Require(a.View.Targets.Single().Completed == 2, "Concurrent target completion counters did not converge.");
        Require(a.View.CraftChoices.Single().Crafted == 3, "Concurrent intermediate crafted counters did not converge.");
        Require(a.View.Rows.Single(r => r.ItemId == 300).Acquired == 5, "Concurrent dormant leaf counters did not converge.");

        a.Edit(new { kind = "set_target_count", recipe_id = 100, count = 4 });
        a.Edit(new { kind = "set_acquired_or_create", item_id = 301, quality = "any", value = 6 });
        Require(a.View.Targets.Single().Count == 4 && a.View.Rows.Single(r => r.ItemId == 301).Acquired == 6,
            "Schema v2 setters failed.");

        a.Edit(new { kind = "remove_target", recipe_id = 100 });
        Require(a.View.Targets.Length == 0, "remove_target failed.");
        a.Edit(new { kind = "undo" });
        Require(a.View.Targets.Length == 1, "Undo of remove_target failed.");

        using var restored = Document.Open(a.Export());
        Require(restored.View.Targets.SequenceEqual(a.View.Targets) && restored.View.CraftChoices.SequenceEqual(a.View.CraftChoices),
            "Schema v2 snapshot round trip failed.");

        Console.WriteLine("PASS: WASM-to-IL Schema v2 craft edits, concurrent CRDT counters, undo, and durable snapshot round trip.");
    }
}
