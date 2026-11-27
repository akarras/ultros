using System.Numerics;
using Dalamud.Bindings.ImGui;
using Ultros.Client;
using Ultros.Lists;

namespace UltrosPlugin.Ui;

internal sealed class ListsPanel
{
    internal enum ListsSubView { Items, Craft, Route }

    private static readonly string[] Qualities = ["Any", "NQ", "HQ"];
    private static readonly string[] WireQualities = ["any", "nq", "hq"];
    private static readonly string[] JobSortOrder = ["CRP", "BSM", "ARM", "GSM", "LTW", "WVR", "ALC", "CUL"];

    private readonly Dictionary<string, (int Need, int Acquired)> drafts = [];
    private readonly Dictionary<string, int> craftDrafts = [];
    private readonly HashSet<string> expandedIngredients = [];
    private string addSearch = "";
    private string lastAddSearch = "";
    private (int Id, string Name)[] addMatches = [];
    private int addItemId = 8;
    private int addQuality;
    private int addQuantity = 1;
    private bool addModeRecipe;
    private string recipeSearch = "";
    private string lastRecipeSearch = "";
    private (int RecipeId, int ItemId, string Name, string JobAbbr, int Level)[] recipeMatches = [];
    private int selectedRecipeId;
    private int addRecipeCount = 1;
    private bool hideAcquired;
    private bool hideCompletedCraft;
    private bool groupCraftByJob;
    private bool showAddRecipeBar;
    private ListsSubView subView = ListsSubView.Items;
    private int? lastListId;
    private bool previewPinned;

    internal Func<int, string> ItemName { get; init; } = id => $"Item #{id}";
    internal Func<int, string> WorldName { get; init; } = id => id > 0 ? $"World #{id}" : "—";
    internal Func<int, bool> HasItem { get; init; } = id => id > 0;
    internal Func<string, (int Id, string Name)[]> SearchAllItems { get; init; } = _ => [];
    internal Func<string, (int RecipeId, int ItemId, string Name, string JobAbbr, int Level)[]>? SearchRecipes { get; init; }
    internal Func<int, bool, ImTextureID?>? ItemIcon { get; init; }
    internal ICraftRecipeSource? RecipeCatalog { get; init; }
    internal Func<bool> CanSyncBagStock { get; init; } = () => false;
    internal Action<CartCommandTarget?>? SyncBagStock { get; init; }
    internal Action Refresh { get; init; } = () => { };
    internal Action<int> SelectList { get; init; } = _ => { };
    internal Action<CartCommandTarget?> RetrySave { get; init; } = _ => { };
    internal Action<CartCommandTarget?> RefreshEstimate { get; init; } = _ => { };
    internal Action<RecoveryChoiceTarget?> RetryRecovery { get; init; } = _ => { };
    internal Action<RecoveryChoiceTarget?> UseServerCopy { get; init; } = _ => { };
    internal Action<ListEditTarget, int, string, int> AddItem { get; init; } = (_, _, _, _) => { };
    internal Action<ListEditTarget, int, int> AddTarget { get; init; } = (_, _, _) => { };
    internal Action<CartCommandTarget?, object> Edit { get; init; } = (_, _) => { };
    internal Action<int> OpenItem { get; init; } = _ => { };
    internal Action<string> Notify { get; init; } = _ => { };

    internal void SetPreviewState(ListsSubView view, bool byJob = false, int? pinnedListId = null, string? expandMatsKey = null)
    {
        subView = view;
        groupCraftByJob = byJob;
        if (pinnedListId is not null) lastListId = pinnedListId;
        if (expandMatsKey is not null) expandedIngredients.Add(expandMatsKey);
        previewPinned = true;
    }

    internal void Draw(ClientView view, (string Title, string Message, string ActionLabel, Action OnAction)? buildBanner = null)
    {
        UiKit.Heading("Lists", "Plan purchases, step through crafting checklists, and track acquired quantities live.");

        if (buildBanner is { } banner)
            UiKit.Banner("lists-build", banner.Title, banner.Message, banner.ActionLabel, banner.OnAction, warning: true);

        var cartTarget = CartCommandTarget.Capture(view);
        var recoveryTarget = RecoveryChoiceTarget.Capture(view);

        // Compact single-row (wrapping on narrow windows) list selector + sync controls
        var avail = ImGui.GetContentRegionAvail().X;
        ImGui.SetNextItemWidth(Math.Max(150 * UiKit.Scale, Math.Min(230 * UiKit.Scale, avail - 150 * UiKit.Scale)));
        var currentName = view.Lists.FirstOrDefault(l => l.List.Id == view.SelectedList)?.List.Name ?? "Cart";
        if (ImGui.BeginCombo("##active-list", currentName))
        {
            try
            {
                foreach (var list in view.Lists)
                {
                    if (ImGui.Selectable($"{list.List.Name.Replace('#', '＃')} · {list.Permission}##{list.List.Id}", list.List.Id == view.SelectedList))
                    {
                        drafts.Clear();
                        craftDrafts.Clear();
                        previewPinned = false;
                        SelectList(list.List.Id);
                    }
                }
            }
            finally { ImGui.EndCombo(); }
        }
        UiKit.Inline(120);
        if (ImGui.Button("Refresh lists")) Refresh();

        if (!view.Connected)
        {
            UiKit.Empty("Connect to sync lists", "Open Account to connect this game client to Ultros.");
            return;
        }
        if (view.Cart is not { } cart)
        {
            UiKit.Empty("No active list selected", "Your online Cart is selected automatically when available. Create a list on Ultros, or pick one above.");
            return;
        }

        // Inline sync badge + actions
        UiKit.Inline(110);
        if (cart.AwaitingPersist) UiKit.Pill("UNSAVED", UiKit.Danger);
        else if (cart.RecoveryReview) UiKit.Pill("RECOVERY", UiKit.Warning);
        else UiKit.Pill(view.ListTransport == "Live updates" ? "LIVE" : "POLLING", view.ListTransport == "Live updates" ? UiKit.Success : UiKit.Accent);

        if (cart.AwaitingPersist)
        {
            UiKit.Inline(140);
            if (ImGui.Button("Retry save / sync")) RetrySave(cartTarget);
        }
        else
        {
            UiKit.Inline(130);
            ImGui.BeginDisabled(view.Estimating);
            if (ImGui.Button(view.Estimating ? "Refreshing…" : "Refresh prices")) RefreshEstimate(cartTarget);
            ImGui.EndDisabled();
        }

        if (view.CanEdit)
        {
            UiKit.Inline(120);
            ImGui.BeginDisabled(!cart.CanUndo);
            if (ImGui.Button("Undo")) Edit(cartTarget, new { kind = "undo" });
            ImGui.EndDisabled();
            ImGui.SameLine();
            ImGui.BeginDisabled(!cart.CanRedo);
            if (ImGui.Button("Redo")) Edit(cartTarget, new { kind = "redo" });
            ImGui.EndDisabled();
        }
        else if (!cart.RecoveryReview)
        {
            UiKit.Hint("Read-only view. Choose a writable list and enable list editing in Account to make changes.");
        }

        if (cart.RecoveryReview)
        {
            UiKit.Banner("recovery", "List recovery paused for review",
                "Both documents are preserved. Retry recovery after resolving the conflict on Ultros, or keep the server copy (your local copy is backed up first).");
            if (ImGui.Button("Retry recovery")) RetryRecovery(recoveryTarget);
            if (cart.HasServerCopy)
            {
                ImGui.SameLine();
                if (ImGui.Button("Keep server copy (back up local)")) UseServerCopy(recoveryTarget);
            }
            return;
        }

        var craftPlan = CraftProject.Build(cart, RecipeCatalog);
        if (view.SelectedList != lastListId)
        {
            lastListId = view.SelectedList;
            if (!previewPinned)
                subView = craftPlan.HasTargets ? ListsSubView.Craft : ListsSubView.Items;
        }
        if (!craftPlan.HasTargets && subView == ListsSubView.Craft)
            subView = ListsSubView.Items;

        var effectiveRows = craftPlan.HasTargets ? craftPlan.ConsolidatedRows : cart.Rows;
        var progress = ShoppingRoute.Progress(effectiveRows);
        var route = ShoppingRoute.Build(effectiveRows, view.Estimate);

        if (craftPlan.HasTargets)
        {
            var craftUnitsDone = craftPlan.Targets.Sum(t => Math.Min(t.Count, Math.Max(0, t.Completed)))
                + craftPlan.Steps.Where(s => s.Mode != "buy").Sum(s => Math.Min(s.Crafts, Math.Max(0, s.Crafted)));
            var craftUnitsTotal = craftPlan.Targets.Sum(t => Math.Max(0, t.Count))
                + craftPlan.Steps.Where(s => s.Mode != "buy").Sum(s => Math.Max(0, s.Crafts));
            var totalWork = craftUnitsTotal + progress.TotalNeed;
            var doneWork = craftUnitsDone + progress.TotalAcquired;
            var fraction = totalWork <= 0 ? 1f : (float)doneWork / totalWork;
            UiKit.ProgressBar(fraction,
                $"{craftPlan.TargetsDone}/{craftPlan.TargetsTotal} targets · {craftPlan.StepsDone}/{craftPlan.StepsTotal} sub-crafts · {progress.TotalAcquired:N0}/{progress.TotalNeed:N0} mats");
        }
        else if (effectiveRows.Length > 0)
        {
            UiKit.ProgressBar(progress.Fraction,
                $"{progress.TotalAcquired:N0} / {progress.TotalNeed:N0} units · {progress.CompletedRows} of {progress.TotalRows} items complete");
        }

        var estimateCurrent = view.Estimate is { } est && est.Version == cart.Version;
        var costValue = view.Estimate is { } e ? $"{e.Total:N0} gil" : view.Estimating ? "Calculating…" : "—";
        var costSub = view.Estimate is { } e2
            ? (estimateCurrent
                ? $"{e2.Scope} · {TimestampFormat.FormatRelativeOrLocal(e2.ObservedAt)}"
                : $"Outdated · {e2.Scope}")
            : "Refresh prices to estimate";
        var routeValue = route.Stops.Length > 0 ? $"{route.Stops.Length} world{(route.Stops.Length == 1 ? "" : "s")}" : progress.RemainingUnits == 0 ? "Done" : "—";
        var routeSub = view.Estimate is { UnpricedUnits: > 0 } ep
            ? $"{ep.UnpricedUnits:N0} unpriced units"
            : progress.RemainingUnits > 0 ? $"{progress.RemainingUnits:N0} units left" : "All items acquired";

        if (craftPlan.HasTargets)
        {
            var readyCount = craftPlan.Targets.Count(t => t.Readiness.Kind == CraftStepReadinessKind.Ready)
                + craftPlan.Steps.Count(s => s.Readiness.Kind == CraftStepReadinessKind.Ready);
            var craftValue = craftPlan.TargetsDone == craftPlan.TargetsTotal && craftPlan.TargetsTotal > 0
                ? "All done"
                : $"{craftPlan.TargetsDone}/{craftPlan.TargetsTotal} targets";
            var craftSub = readyCount > 0
                ? $"{readyCount} ready now · {craftPlan.StepsDone}/{craftPlan.StepsTotal} sub-crafts"
                : $"{craftPlan.StepsDone}/{craftPlan.StepsTotal} sub-crafts complete";
            var craftAccent = craftPlan.TargetsDone == craftPlan.TargetsTotal && craftPlan.TargetsTotal > 0
                ? UiKit.Success
                : readyCount > 0 ? UiKit.Success : UiKit.Accent;
            UiKit.KpiRow(
                new KpiCard("Craft copilot", craftValue, craftSub, craftAccent),
                new KpiCard("Estimated remaining", costValue, costSub, estimateCurrent ? UiKit.Accent : UiKit.Warning),
                new KpiCard("Shopping stops", routeValue, routeSub, route.Stops.Length > 0 ? UiKit.Success : null));
        }
        else
        {
            UiKit.KpiRow(
                new KpiCard("Estimated remaining", costValue, costSub, estimateCurrent ? UiKit.Accent : UiKit.Warning),
                new KpiCard("Shopping stops", routeValue, routeSub, route.Stops.Length > 0 ? UiKit.Success : null));
        }

        // Sub-view switcher
        if (craftPlan.HasTargets)
        {
            var craftDone = craftPlan.TargetsDone + craftPlan.StepsDone;
            var craftTotal = craftPlan.TargetsTotal + craftPlan.StepsTotal;
            DrawViewButton($"Craft ({craftDone}/{craftTotal})", subView == ListsSubView.Craft, () => { subView = ListsSubView.Craft; previewPinned = false; });
            ImGui.SameLine();
            var routeLabel = route.Stops.Length > 0 ? $"Shopping Route ({route.Stops.Length})" : "Shopping Route";
            DrawViewButton(routeLabel, subView == ListsSubView.Route, () => { subView = ListsSubView.Route; previewPinned = false; });
            ImGui.SameLine();
            DrawViewButton($"Items ({effectiveRows.Length})", subView == ListsSubView.Items, () => { subView = ListsSubView.Items; previewPinned = false; });
        }
        else
        {
            DrawViewButton("Items", subView == ListsSubView.Items, () => { subView = ListsSubView.Items; previewPinned = false; });
            ImGui.SameLine();
            var routeLabel = route.Stops.Length > 0 ? $"Shopping Route ({route.Stops.Length})" : "Shopping Route";
            DrawViewButton(routeLabel, subView == ListsSubView.Route, () => { subView = ListsSubView.Route; previewPinned = false; });
        }

        if (subView == ListsSubView.Items)
        {
            UiKit.Inline(150);
            ImGui.Checkbox("Hide acquired", ref hideAcquired);
            if (view.CanEdit) DrawAddBar(view);
            DrawItems(view, cart, craftPlan, effectiveRows, cartTarget);
            if (view.CanEdit) DrawAddByIdFallback(view);
        }
        else if (subView == ListsSubView.Craft)
        {
            DrawCraft(view, cart, craftPlan, cartTarget);
        }
        else
        {
            DrawRoute(view, cart, effectiveRows.Length, cartTarget, route);
        }
    }

    private void DrawViewButton(string label, bool active, Action onClick)
    {
        if (active) ImGui.PushStyleColor(ImGuiCol.Button, UiKit.Accent);
        try { if (ImGui.Button(label)) onClick(); }
        finally { if (active) ImGui.PopStyleColor(); }
    }

    private void DrawAddBar(ClientView view)
    {
        if (SearchRecipes is not null)
        {
            DrawViewButton("Item", !addModeRecipe, () => addModeRecipe = false);
            ImGui.SameLine();
            DrawViewButton("Recipe", addModeRecipe, () => addModeRecipe = true);
            UiKit.Inline(260);
        }

        if (addModeRecipe && SearchRecipes is not null)
        {
            DrawRecipeAddControls(view, switchToCraftOnAdd: true);
            return;
        }

        var avail = ImGui.GetContentRegionAvail().X;
        ImGui.SetNextItemWidth(Math.Max(150 * UiKit.Scale, Math.Min(250 * UiKit.Scale, avail - 300 * UiKit.Scale)));
        var enter = ImGui.InputTextWithHint("##list-add-search", "Add item by name…", ref addSearch, 100, ImGuiInputTextFlags.EnterReturnsTrue);
        var trimmed = addSearch.Trim();
        if (enter || (trimmed.Length >= 2 && trimmed != lastAddSearch))
        {
            lastAddSearch = trimmed;
            addMatches = trimmed.Length >= 2 ? SearchAllItems(trimmed) : [];
            if (enter && addMatches.Length > 0)
            {
                addItemId = addMatches[0].Id;
                addSearch = addMatches[0].Name;
                lastAddSearch = addSearch;
                addMatches = [];
            }
        }
        else if (trimmed.Length < 2 && addMatches.Length > 0)
        {
            lastAddSearch = "";
            addMatches = [];
        }

        UiKit.Inline(85);
        ImGui.SetNextItemWidth(76 * UiKit.Scale);
        ImGui.Combo("##list-add-qual", ref addQuality, Qualities, Qualities.Length);
        ImGui.SameLine();
        ImGui.SetNextItemWidth(76 * UiKit.Scale);
        if (ImGui.InputInt("##list-add-qty", ref addQuantity, 0, 0)) addQuantity = Math.Clamp(addQuantity, 1, 1_000_000);
        ImGui.SameLine();
        if (ImGui.Button("Add##list-add"))
        {
            if (!HasItem(addItemId) || addQuantity is < 1 or > 1_000_000)
                Notify("Choose a valid item and a quantity between 1 and 1,000,000.");
            else if (ListEditTarget.Capture(view) is { } destination)
                AddItem(destination, addItemId, WireQualities[addQuality], addQuantity);
        }

        if (addMatches.Length > 0)
        {
            var listHeight = Math.Min(addMatches.Length, 5) * 26 * UiKit.Scale + 10 * UiKit.Scale;
            ImGui.PushStyleColor(ImGuiCol.ChildBg, UiKit.CardBg);
            try
            {
                ImGui.BeginChild("##list-add-matches", new Vector2(-1, listHeight), true);
                try
                {
                    foreach (var match in addMatches)
                    {
                        ImGui.PushID(match.Id);
                        try
                        {
                            UiKit.ItemIcon(ItemIcon, match.Id, false, 18f);
                            if (ImGui.Selectable($"{match.Name.Replace('#', '＃')}##pick-add", addItemId == match.Id))
                            {
                                addItemId = match.Id;
                                addSearch = match.Name;
                                lastAddSearch = match.Name;
                                addMatches = [];
                            }
                        }
                        finally { ImGui.PopID(); }
                    }
                }
                finally { ImGui.EndChild(); }
            }
            finally { ImGui.PopStyleColor(); }
        }
    }

    private void DrawRecipeAddControls(ClientView view, bool switchToCraftOnAdd)
    {
        if (SearchRecipes is null) return;
        var avail = ImGui.GetContentRegionAvail().X;
        ImGui.SetNextItemWidth(Math.Max(150 * UiKit.Scale, Math.Min(260 * UiKit.Scale, avail - 210 * UiKit.Scale)));
        var enter = ImGui.InputTextWithHint("##list-recipe-search", "Add recipe target by name…", ref recipeSearch, 100, ImGuiInputTextFlags.EnterReturnsTrue);
        var trimmed = recipeSearch.Trim();
        if (enter || (trimmed.Length >= 2 && trimmed != lastRecipeSearch))
        {
            lastRecipeSearch = trimmed;
            recipeMatches = trimmed.Length >= 2 ? SearchRecipes(trimmed) : [];
            if (enter && recipeMatches.Length > 0)
            {
                selectedRecipeId = recipeMatches[0].RecipeId;
                recipeSearch = recipeMatches[0].Name;
                lastRecipeSearch = recipeSearch;
                recipeMatches = [];
            }
        }
        else if (trimmed.Length < 2 && recipeMatches.Length > 0)
        {
            lastRecipeSearch = "";
            recipeMatches = [];
        }

        UiKit.Inline(175);
        ImGui.SetNextItemWidth(68 * UiKit.Scale);
        if (ImGui.InputInt("##recipe-add-qty", ref addRecipeCount, 0, 0))
            addRecipeCount = Math.Clamp(addRecipeCount, 1, 9999);
        ImGui.SameLine();
        ImGui.BeginDisabled(selectedRecipeId <= 0);
        if (ImGui.Button("Add recipe##recipe-add"))
        {
            if (ListEditTarget.Capture(view) is { } destination && selectedRecipeId > 0)
            {
                AddTarget(destination, selectedRecipeId, addRecipeCount);
                if (switchToCraftOnAdd)
                {
                    subView = ListsSubView.Craft;
                    previewPinned = false;
                }
            }
        }
        ImGui.EndDisabled();

        if (recipeMatches.Length > 0)
        {
            var listHeight = Math.Min(recipeMatches.Length, 5) * 26 * UiKit.Scale + 10 * UiKit.Scale;
            ImGui.PushStyleColor(ImGuiCol.ChildBg, UiKit.CardBg);
            try
            {
                ImGui.BeginChild("##list-recipe-matches", new Vector2(-1, listHeight), true);
                try
                {
                    foreach (var match in recipeMatches)
                    {
                        ImGui.PushID(match.RecipeId);
                        try
                        {
                            UiKit.ItemIcon(ItemIcon, match.ItemId, false, 18f);
                            var jobTag = match.JobAbbr.Length > 0
                                ? (match.Level > 0 ? $"[{match.JobAbbr} Lv.{match.Level}] " : $"[{match.JobAbbr}] ")
                                : "";
                            if (ImGui.Selectable($"{jobTag}{match.Name.Replace('#', '＃')}##pick-recipe", selectedRecipeId == match.RecipeId))
                            {
                                selectedRecipeId = match.RecipeId;
                                recipeSearch = match.Name;
                                lastRecipeSearch = match.Name;
                                recipeMatches = [];
                            }
                        }
                        finally { ImGui.PopID(); }
                    }
                }
                finally { ImGui.EndChild(); }
            }
            finally { ImGui.PopStyleColor(); }
        }
    }

    private void DrawCraft(ClientView view, CartView cart, CraftProjectPlan plan, CartCommandTarget? editTarget)
    {
        // Craft copilot toolbar
        DrawViewButton("Step order", !groupCraftByJob, () => groupCraftByJob = false);
        ImGui.SameLine();
        DrawViewButton("By job", groupCraftByJob, () => groupCraftByJob = true);
        UiKit.Inline(130);
        ImGui.Checkbox("Hide done", ref hideCompletedCraft);

        UiKit.Inline(155);
        ImGui.BeginDisabled(!view.CanEdit);
        var excludeCrystals = cart.ExcludeCrystals;
        if (ImGui.Checkbox("Exclude crystals", ref excludeCrystals))
            Edit(editTarget, new { kind = "set_craft_options", exclude_crystals = excludeCrystals, include_vendors = cart.IncludeVendors });
        ImGui.EndDisabled();

        if (SyncBagStock is not null)
        {
            UiKit.Inline(155);
            var canSync = view.CanEdit && CanSyncBagStock();
            ImGui.BeginDisabled(!canSync);
            if (ImGui.Button("Sync bag stock")) SyncBagStock(editTarget);
            ImGui.EndDisabled();
            if (ImGui.IsItemHovered(ImGuiHoveredFlags.AllowWhenDisabled))
            {
                ImGui.SetTooltip(canSync
                    ? "Populate On hand and acquired materials from your character's currently loaded inventory."
                    : "Load your character on a writable list to sync stock from your bags.");
            }
        }

        if (view.CanEdit && SearchRecipes is not null)
        {
            UiKit.Inline(115);
            DrawViewButton(showAddRecipeBar ? "Close add" : "+ Recipe", showAddRecipeBar, () => showAddRecipeBar = !showAddRecipeBar);
        }

        if (plan.Error is { } err)
        {
            ImGui.Spacing();
            UiKit.Banner("craft-error", "Crafting plan incomplete", err, warning: true);
        }

        if (view.CanEdit && SearchRecipes is not null && showAddRecipeBar)
        {
            ImGui.Spacing();
            DrawRecipeAddControls(view, switchToCraftOnAdd: false);
        }

        // Section 1: Finished targets
        UiKit.Section($"Finished targets ({plan.TargetsDone}/{plan.TargetsTotal})");
        var visibleTargets = plan.Targets
            .Where(t => !hideCompletedCraft || t.Readiness.Kind != CraftStepReadinessKind.Done)
            .ToArray();
        if (visibleTargets.Length == 0)
        {
            UiKit.Empty(plan.Targets.Length == 0 ? "No recipe targets yet" : "All targets complete",
                plan.Targets.Length == 0 ? "Add a recipe above or on the Ultros website to build your crafting plan." : "Uncheck Hide done to review completed targets.");
        }
        else
        {
            foreach (var target in visibleTargets)
                DrawCraftTarget(view, target, editTarget);
        }

        // Section 2: Intermediate sub-crafts
        UiKit.Section($"Sub-crafts ({plan.StepsDone}/{plan.StepsTotal})");
        var visibleSteps = plan.Steps
            .Where(s => !hideCompletedCraft || s.Readiness.Kind != CraftStepReadinessKind.Done)
            .ToArray();

        if (visibleSteps.Length == 0)
        {
            if (plan.Steps.Length == 0)
                UiKit.Hint("No intermediate sub-crafts required. All ingredients come directly from Shopping Route or on-hand stock.");
            else
                UiKit.Hint("All intermediate sub-crafts are complete. Uncheck Hide done to show them.");
            return;
        }

        if (groupCraftByJob)
        {
            var groups = visibleSteps
                .GroupBy(s => s.JobAbbr.Length > 0 ? s.JobAbbr : "Other")
                .OrderBy(g => JobGroupRank(g.Key))
                .ThenBy(g => g.Key, StringComparer.Ordinal);
            foreach (var group in groups)
            {
                var doneInGroup = group.Count(s => s.Readiness.Kind == CraftStepReadinessKind.Done);
                UiKit.Section($"{group.Key} · {doneInGroup}/{group.Count()} ready or done");
                foreach (var step in group)
                    DrawCraftStep(view, step, editTarget);
            }
        }
        else
        {
            foreach (var step in visibleSteps)
                DrawCraftStep(view, step, editTarget);
        }
    }

    private static int JobGroupRank(string jobAbbr)
    {
        var idx = Array.IndexOf(JobSortOrder, jobAbbr);
        return idx >= 0 ? idx : JobSortOrder.Length;
    }

    private void DrawCraftTarget(ClientView view, CraftTargetPlan target, CartCommandTarget? editTarget)
    {
        var key = $"{view.SelectedList}/target/{target.RecipeId}";
        var countDraft = craftDrafts.GetValueOrDefault(key, (int)Math.Clamp(target.Count, 1, 9999));
        var done = target.Readiness.Kind == CraftStepReadinessKind.Done;

        ImGui.PushID(key);
        try
        {
            DrawItemLink(target.ItemId, done);

            UiKit.Inline(260);
            if (target.JobAbbr.Length > 0)
            {
                UiKit.Pill(target.RecipeLevel > 0 ? $"{target.JobAbbr} {target.RecipeLevel}" : target.JobAbbr, UiKit.Accent);
                ImGui.SameLine();
            }
            DrawReadinessPill(target.Readiness);
            ImGui.SameLine();
            var surplus = target.Surplus > 0 ? $" · +{target.Surplus:N0} extra" : "";
            ImGui.TextUnformatted($"{target.Completed:N0}/{target.Count:N0} made{surplus}");

            ImGui.BeginDisabled(!view.CanEdit);
            try
            {
                UiKit.Muted("Count");
                ImGui.SameLine();
                ImGui.SetNextItemWidth(54 * UiKit.Scale);
                var commit = ImGui.InputInt("##target-count", ref countDraft, 0, 0, "%d", ImGuiInputTextFlags.EnterReturnsTrue);
                var active = ImGui.IsItemActive();
                commit |= ImGui.IsItemDeactivatedAfterEdit();
                if (commit)
                {
                    var clamped = Math.Clamp(countDraft, 1, 9999);
                    Edit(editTarget, new { kind = "set_target_count", recipe_id = target.RecipeId, count = clamped });
                    craftDrafts.Remove(key);
                }
                else if (active) craftDrafts[key] = countDraft;
                else craftDrafts.Remove(key);

                UiKit.Inline(215);
                if (target.Completed < target.Count)
                {
                    var delta = Math.Min(Math.Max(1, target.YieldAmount), target.Count - target.Completed);
                    if (ImGui.SmallButton($"+{delta}##target-inc"))
                        Edit(editTarget, new { kind = "add_target_completed", recipe_id = target.RecipeId, delta });
                    ImGui.SameLine();
                    if (ImGui.SmallButton("Done##target-done"))
                        Edit(editTarget, new { kind = "set_target_completed", recipe_id = target.RecipeId, value = target.Count });
                }
                else
                {
                    if (ImGui.SmallButton("Reset##target-reset"))
                        Edit(editTarget, new { kind = "set_target_completed", recipe_id = target.RecipeId, value = 0 });
                }

                ImGui.SameLine();
                DrawMatsToggle(key);

                ImGui.SameLine();
                if (ImGui.SmallButton("Remove##target-rm"))
                {
                    craftDrafts.Remove(key);
                    Edit(editTarget, new { kind = "remove_target", recipe_id = target.RecipeId });
                }
            }
            finally { ImGui.EndDisabled(); }

            if (expandedIngredients.Contains(key))
                DrawIngredientsBreakdown(target.Ingredients);

            ImGui.Separator();
        }
        finally { ImGui.PopID(); }
    }

    private void DrawCraftStep(ClientView view, CraftStepPlan step, CartCommandTarget? editTarget)
    {
        var key = $"{view.SelectedList}/step/{step.ItemId}";
        var onHandDraft = craftDrafts.GetValueOrDefault(key, (int)Math.Clamp(step.OnHand, 0, 1_000_000));
        var done = step.Readiness.Kind == CraftStepReadinessKind.Done;

        ImGui.PushID(key);
        try
        {
            UiKit.Muted($"#{step.StepNumber}");
            ImGui.SameLine();
            DrawItemLink(step.ItemId, done);

            UiKit.Inline(290);
            if (step.JobAbbr.Length > 0)
            {
                UiKit.Pill(step.RecipeLevel > 0 ? $"{step.JobAbbr} {step.RecipeLevel}" : step.JobAbbr, UiKit.Accent);
                ImGui.SameLine();
            }
            DrawReadinessPill(step.Readiness);
            ImGui.SameLine();
            var summary = step.Mode == "buy"
                ? $"Buy {step.Remaining:N0} (of {step.Needed:N0} needed)"
                : step.Crafts == 0
                    ? $"Covered by {step.OnHand:N0} on hand"
                    : step.Crafted >= step.Crafts
                        ? $"{step.Crafted:N0}/{step.Crafts:N0} crafts complete"
                        : $"{step.Crafted:N0}/{step.Crafts:N0} crafts · make {step.Remaining:N0}";
            ImGui.TextUnformatted(summary);

            if (step.Consumers.Length > 0)
            {
                var shown = step.Consumers.Take(3).Select(c => $"{ItemName(c.ItemId)} ×{c.UnitsUsed:N0}");
                var usedText = "Used by: " + string.Join(", ", shown)
                    + (step.Consumers.Length > 3 ? $" +{step.Consumers.Length - 3} more" : "");
                UiKit.Hint(usedText);
                if (step.Consumers.Length > 3 && ImGui.IsItemHovered())
                    ImGui.SetTooltip(string.Join("\n", step.Consumers.Select(c => $"{ItemName(c.ItemId)} ×{c.UnitsUsed:N0}")));
            }

            ImGui.BeginDisabled(!view.CanEdit);
            try
            {
                DrawViewButton("Craft##mode-craft", step.Mode != "buy", () =>
                {
                    if (step.Mode == "buy")
                        Edit(editTarget, new { kind = "set_craft_choice", item_id = step.ItemId, mode = "craft", recipe_id = step.RecipeId });
                });
                ImGui.SameLine();
                DrawViewButton("Buy##mode-buy", step.Mode == "buy", () =>
                {
                    if (step.Mode != "buy")
                        Edit(editTarget, new { kind = "set_craft_choice", item_id = step.ItemId, mode = "buy", recipe_id = step.RecipeId });
                });

                UiKit.Inline(140);
                UiKit.Muted("On hand");
                ImGui.SameLine();
                ImGui.SetNextItemWidth(54 * UiKit.Scale);
                var commit = ImGui.InputInt("##step-onhand", ref onHandDraft, 0, 0, "%d", ImGuiInputTextFlags.EnterReturnsTrue);
                var active = ImGui.IsItemActive();
                commit |= ImGui.IsItemDeactivatedAfterEdit();
                if (commit)
                {
                    var clamped = Math.Clamp(onHandDraft, 0, 1_000_000);
                    Edit(editTarget, new { kind = "set_intermediate_on_hand", item_id = step.ItemId, on_hand = clamped });
                    craftDrafts.Remove(key);
                }
                else if (active) craftDrafts[key] = onHandDraft;
                else craftDrafts.Remove(key);

                if (step.Mode != "buy" && step.Crafts > 0)
                {
                    UiKit.Inline(155);
                    if (step.Crafted < step.Crafts)
                    {
                        if (ImGui.SmallButton("+1##step-inc"))
                            Edit(editTarget, new { kind = "add_intermediate_crafted", item_id = step.ItemId, delta = 1 });
                        ImGui.SameLine();
                        if (ImGui.SmallButton("Done##step-done"))
                            Edit(editTarget, new { kind = "set_intermediate_crafted", item_id = step.ItemId, value = step.Crafts });
                    }
                    else
                    {
                        if (ImGui.SmallButton("Reset##step-reset"))
                            Edit(editTarget, new { kind = "set_intermediate_crafted", item_id = step.ItemId, value = 0 });
                    }
                }

                UiKit.Inline(80);
                DrawMatsToggle(key);
            }
            finally { ImGui.EndDisabled(); }

            if (expandedIngredients.Contains(key))
                DrawIngredientsBreakdown(step.Ingredients);

            ImGui.Separator();
        }
        finally { ImGui.PopID(); }
    }

    private void DrawMatsToggle(string key)
    {
        var open = expandedIngredients.Contains(key);
        if (ImGui.SmallButton($"{(open ? "Hide mats" : "Mats")}##mats-toggle"))
        {
            if (open) expandedIngredients.Remove(key);
            else expandedIngredients.Add(key);
        }
    }

    private void DrawIngredientsBreakdown(CraftStepIngredient[] ingredients)
    {
        ImGui.Indent(16 * UiKit.Scale);
        try
        {
            foreach (var ing in ingredients)
            {
                var name = ItemName(ing.ItemId).Replace('#', '＃');
                var tag = ing.IsCraftedStep ? " (sub-craft)" : ing.IsCrystal ? " (crystal)" : "";
                if (ing.NeededTotal == 0 && ing.IsCrystal)
                {
                    UiKit.Muted($"{name}{tag}: excluded");
                    continue;
                }
                var displayNeed = ing.NeededRemaining > 0 ? ing.NeededRemaining : ing.NeededTotal;
                var ok = ing.Available >= displayNeed;
                ImGui.TextUnformatted($"{name}{tag}:");
                ImGui.SameLine();
                ImGui.TextColored(ok ? UiKit.Success : UiKit.Warning, $"{ing.Available:N0} / {displayNeed:N0}");
            }
        }
        finally { ImGui.Unindent(16 * UiKit.Scale); }
    }

    private static void DrawReadinessPill(CraftStepReadiness readiness)
    {
        switch (readiness.Kind)
        {
            case CraftStepReadinessKind.Done:
                UiKit.Pill("DONE", UiKit.Success);
                break;
            case CraftStepReadinessKind.Ready:
                UiKit.Pill(readiness.CraftableNow > 1 ? $"READY ×{readiness.CraftableNow}" : "READY", UiKit.Success);
                break;
            case CraftStepReadinessKind.NeedMaterials:
                UiKit.Pill($"NEED {readiness.MissingKinds} MAT{(readiness.MissingKinds == 1 ? "" : "S")}", UiKit.Warning);
                break;
            case CraftStepReadinessKind.Buying:
                UiKit.Pill("BUYING", UiKit.Accent);
                break;
        }
    }

    private void DrawItemLink(int itemId, bool done)
    {
        UiKit.ItemIcon(ItemIcon, itemId, false, 20f);
        var displayName = ItemName(itemId).Replace('#', '＃');
        var width = Math.Min(ImGui.CalcTextSize(displayName).X + 6 * UiKit.Scale, Math.Max(120 * UiKit.Scale, ImGui.GetContentRegionAvail().X));
        if (done) ImGui.PushStyleColor(ImGuiCol.Text, ImGui.GetStyle().Colors[(int)ImGuiCol.TextDisabled]);
        try
        {
            if (ImGui.Selectable($"{displayName}##open", false, ImGuiSelectableFlags.None, new Vector2(width, 0))) OpenItem(itemId);
        }
        finally { if (done) ImGui.PopStyleColor(); }
        if (ImGui.IsItemHovered()) ImGui.SetTooltip("Click to open in Market");
    }

    private void DrawAddByIdFallback(ClientView view)
    {
        ImGui.Spacing();
        if (ImGui.CollapsingHeader("Add by item ID"))
        {
            ImGui.SetNextItemWidth(110 * UiKit.Scale);
            ImGui.InputInt("Item ID##list-id", ref addItemId);
            ImGui.SameLine();
            UiKit.Muted(HasItem(addItemId) ? ItemName(addItemId) : "Unknown item");
            ImGui.SameLine();
            if (ImGui.SmallButton("Add ID##list-add-id"))
            {
                if (!HasItem(addItemId) || addQuantity is < 1 or > 1_000_000)
                    Notify("Choose a valid item and a quantity between 1 and 1,000,000.");
                else if (ListEditTarget.Capture(view) is { } destination)
                    AddItem(destination, addItemId, WireQualities[addQuality], addQuantity);
            }
        }
    }

    private void DrawItems(ClientView view, CartView cart, CraftProjectPlan craftPlan, CartRow[] effectiveRows, CartCommandTarget? editTarget)
    {
        var visibleRows = effectiveRows
            .Where(row => !hideAcquired || row.Acquired < Math.Max(1, row.Need))
            .ToArray();

        if (visibleRows.Length == 0)
        {
            UiKit.Empty(effectiveRows.Length == 0 ? "This list is empty" : "Everything is acquired",
                effectiveRows.Length == 0 ? "Search for an item above or right-click items in game to add them." : "Uncheck Hide acquired to view completed rows.");
            return;
        }

        var useCreateAcquire = craftPlan.HasTargets;

        if (ImGui.GetContentRegionAvail().X < 740 * UiKit.Scale)
        {
            DrawItemsCompact(view, cart, visibleRows, useCreateAcquire, editTarget);
        }
        else if (ImGui.BeginTable("cart-v3", 7, ImGuiTableFlags.RowBg | ImGuiTableFlags.BordersInnerH | ImGuiTableFlags.SizingStretchProp))
        {
            try
            {
                ImGui.TableSetupColumn("Item", ImGuiTableColumnFlags.WidthStretch, 1f);
                ImGui.TableSetupColumn("Qual", ImGuiTableColumnFlags.WidthFixed, 46 * UiKit.Scale);
                ImGui.TableSetupColumn("Need", ImGuiTableColumnFlags.WidthFixed, 68 * UiKit.Scale);
                ImGui.TableSetupColumn("Have", ImGuiTableColumnFlags.WidthFixed, 68 * UiKit.Scale);
                ImGui.TableSetupColumn("Est. cost", ImGuiTableColumnFlags.WidthFixed, 108 * UiKit.Scale);
                ImGui.TableSetupColumn("Quick", ImGuiTableColumnFlags.WidthFixed, 96 * UiKit.Scale);
                ImGui.TableSetupColumn("", ImGuiTableColumnFlags.WidthFixed, 68 * UiKit.Scale);
                ImGui.TableHeadersRow();

                foreach (var row in visibleRows)
                {
                    var key = $"{view.SelectedList}/{row.ItemId}/{row.Quality}";
                    (int Need, int Acquired) draft = drafts.GetValueOrDefault(key, ((int)Math.Clamp(row.Need, 0, int.MaxValue), (int)Math.Clamp(row.Acquired, 0, int.MaxValue)));
                    var line = ShoppingRoute.FindLine(view.Estimate, row);
                    var done = row.Acquired >= Math.Max(1, row.Need);
                    var derivedOnly = CraftProject.IsDerivedOnlyRow(cart, row);

                    ImGui.PushID(key);
                    try
                    {
                        ImGui.TableNextRow();
                        ImGui.TableNextColumn();
                        DrawItemName(row, done);

                        ImGui.TableNextColumn();
                        UiKit.QualityBadge(row.Quality);

                        ImGui.BeginDisabled(!view.CanEdit);
                        try
                        {
                            ImGui.TableNextColumn();
                            ImGui.SetNextItemWidth(-1);
                            bool commit = false, needActive = false;
                            if (derivedOnly)
                            {
                                ImGui.BeginDisabled(true);
                                (commit, needActive) = DrawNeedInput(key, row, ref draft, editTarget);
                                ImGui.EndDisabled();
                                if (ImGui.IsItemHovered(ImGuiHoveredFlags.AllowWhenDisabled))
                                    ImGui.SetTooltip("Quantity derived from Craft targets.");
                            }
                            else
                            {
                                (commit, needActive) = DrawNeedInput(key, row, ref draft, editTarget);
                            }

                            ImGui.TableNextColumn();
                            ImGui.SetNextItemWidth(-1);
                            var (acquired, acquiredActive) = DrawAcquiredInput(key, row, useCreateAcquire, ref draft, editTarget);
                            if (!commit && !acquired && (needActive || acquiredActive)) drafts[key] = draft;
                            else drafts.Remove(key);

                            ImGui.TableNextColumn();
                            DrawRowEstimate(line, done);

                            ImGui.TableNextColumn();
                            DrawQuickButtons(key, row, useCreateAcquire, editTarget);

                            ImGui.TableNextColumn();
                            if (derivedOnly)
                            {
                                UiKit.Muted("Craft mat");
                            }
                            else
                            {
                                DrawRemoveButton(key, row, editTarget);
                            }
                        }
                        finally { ImGui.EndDisabled(); }
                    }
                    finally { ImGui.PopID(); }
                }
            }
            finally { ImGui.EndTable(); }
        }

        if (view.Estimate is not null)
            UiKit.Hint("Estimates allow partial stacks. Prices may change before you buy.");
    }

    private void DrawItemsCompact(ClientView view, CartView cart, CartRow[] visibleRows, bool useCreateAcquire, CartCommandTarget? editTarget)
    {
        foreach (var row in visibleRows)
        {
            var key = $"{view.SelectedList}/{row.ItemId}/{row.Quality}";
            (int Need, int Acquired) draft = drafts.GetValueOrDefault(key, ((int)Math.Clamp(row.Need, 0, int.MaxValue), (int)Math.Clamp(row.Acquired, 0, int.MaxValue)));
            var line = ShoppingRoute.FindLine(view.Estimate, row);
            var done = row.Acquired >= Math.Max(1, row.Need);
            var derivedOnly = CraftProject.IsDerivedOnlyRow(cart, row);

            ImGui.PushID(key);
            try
            {
                DrawItemName(row, done);

                UiKit.QualityBadge(row.Quality);
                if (derivedOnly)
                {
                    ImGui.SameLine();
                    UiKit.Pill("CRAFT MAT", UiKit.Accent);
                }
                ImGui.SameLine();
                DrawRowEstimate(line, done);

                ImGui.BeginDisabled(!view.CanEdit);
                try
                {
                    UiKit.Inline(108);
                    UiKit.Muted("Need");
                    ImGui.SameLine();
                    ImGui.SetNextItemWidth(56 * UiKit.Scale);
                    bool commit = false, needActive = false;
                    if (derivedOnly)
                    {
                        ImGui.BeginDisabled(true);
                        (commit, needActive) = DrawNeedInput(key, row, ref draft, editTarget);
                        ImGui.EndDisabled();
                    }
                    else
                    {
                        (commit, needActive) = DrawNeedInput(key, row, ref draft, editTarget);
                    }

                    UiKit.Inline(108);
                    UiKit.Muted("Have");
                    ImGui.SameLine();
                    ImGui.SetNextItemWidth(56 * UiKit.Scale);
                    var (acquired, acquiredActive) = DrawAcquiredInput(key, row, useCreateAcquire, ref draft, editTarget);
                    if (!commit && !acquired && (needActive || acquiredActive)) drafts[key] = draft;
                    else drafts.Remove(key);

                    UiKit.Inline(155);
                    DrawQuickButtons(key, row, useCreateAcquire, editTarget);
                    if (!derivedOnly)
                    {
                        ImGui.SameLine();
                        DrawRemoveButton(key, row, editTarget);
                    }
                }
                finally { ImGui.EndDisabled(); }

                ImGui.Separator();
            }
            finally { ImGui.PopID(); }
        }
    }

    private void DrawItemName(CartRow row, bool done)
    {
        UiKit.ItemIcon(ItemIcon, row.ItemId, row.Quality == "hq", 20f);
        var displayName = ItemName(row.ItemId).Replace('#', '＃');
        if (done) ImGui.PushStyleColor(ImGuiCol.Text, ImGui.GetStyle().Colors[(int)ImGuiCol.TextDisabled]);
        try
        {
            if (ImGui.Selectable($"{displayName}##open")) OpenItem(row.ItemId);
        }
        finally { if (done) ImGui.PopStyleColor(); }
        if (ImGui.IsItemHovered()) ImGui.SetTooltip("Click to open in Market");
    }

    private (bool Committed, bool Active) DrawNeedInput(string key, CartRow row, ref (int Need, int Acquired) draft, CartCommandTarget? editTarget)
    {
        var commit = ImGui.InputInt("##need", ref draft.Need, 0, 0, "%d", ImGuiInputTextFlags.EnterReturnsTrue);
        var active = ImGui.IsItemActive();
        commit |= ImGui.IsItemDeactivatedAfterEdit();
        if (commit)
        {
            Edit(editTarget, new { kind = "set_need", item_id = row.ItemId, quality = row.Quality, need = draft.Need });
            drafts.Remove(key);
        }
        return (commit, active);
    }

    private (bool Committed, bool Active) DrawAcquiredInput(string key, CartRow row, bool useCreateAcquire, ref (int Need, int Acquired) draft, CartCommandTarget? editTarget)
    {
        var acquired = ImGui.InputInt("##acquired", ref draft.Acquired, 0, 0, "%d", ImGuiInputTextFlags.EnterReturnsTrue);
        var active = ImGui.IsItemActive();
        acquired |= ImGui.IsItemDeactivatedAfterEdit();
        if (acquired)
        {
            var kind = useCreateAcquire ? "set_acquired_or_create" : "set_acquired";
            Edit(editTarget, new { kind, item_id = row.ItemId, quality = row.Quality, value = draft.Acquired });
            drafts.Remove(key);
        }
        return (acquired, active);
    }

    private void DrawQuickButtons(string key, CartRow row, bool useCreateAcquire, CartCommandTarget? editTarget)
    {
        var kind = useCreateAcquire ? "add_acquired_or_create" : "add_acquired";
        if (ImGui.SmallButton("+1"))
        {
            drafts.Remove(key);
            Edit(editTarget, new { kind, item_id = row.ItemId, quality = row.Quality, delta = 1 });
        }
        ImGui.SameLine();
        if (ImGui.SmallButton("+5"))
        {
            drafts.Remove(key);
            Edit(editTarget, new { kind, item_id = row.ItemId, quality = row.Quality, delta = 5 });
        }
    }

    private void DrawRemoveButton(string key, CartRow row, CartCommandTarget? editTarget)
    {
        if (ImGui.SmallButton("Remove"))
        {
            drafts.Remove(key);
            Edit(editTarget, new { kind = "remove", item_id = row.ItemId, quality = row.Quality });
        }
    }

    private static void DrawRowEstimate(EstimateLine? line, bool done)
    {
        if (done || line?.Status == "Acquired")
        {
            ImGui.TextColored(UiKit.Success, "Done");
            return;
        }
        if (line is null)
        {
            UiKit.Muted("—");
            return;
        }
        switch (line.Status)
        {
            case "Priced":
                ImGui.TextUnformatted($"{line.Total:N0} gil");
                if (ImGui.IsItemHovered() && line.UnitPrice is { } u)
                    ImGui.SetTooltip($"Cheapest unit: {u:N0} gil");
                break;
            case "PartialSupply":
                ImGui.TextColored(UiKit.Warning, $"{line.Total:N0}*");
                if (ImGui.IsItemHovered())
                    ImGui.SetTooltip($"Partial supply: {line.PricedUnits:N0} of {line.Remaining:N0} units priced.");
                break;
            case "NoSupply":
                ImGui.TextColored(UiKit.Danger, "No supply");
                break;
            default:
                UiKit.Muted("—");
                break;
        }
    }

    private void DrawRoute(ClientView view, CartView cart, int effectiveRowCount, CartCommandTarget? editTarget, ShoppingRoutePlan route)
    {
        if (view.Estimate is null)
        {
            UiKit.Empty(view.Estimating ? "Building shopping route…" : "No price estimate loaded yet",
                effectiveRowCount > 0 ? "Click Refresh prices above to group your remaining items by cheapest world." : "Add items or recipe targets first, then click Refresh prices.");
            return;
        }
        if (route.Stops.Length == 0 && route.UnpricedRows.Length == 0)
        {
            UiKit.Empty("Nothing left to buy", "Every item on this list is already marked acquired.");
            return;
        }

        if (view.Estimate.Version != cart.Version)
            UiKit.Hint("Prices are from before your latest edits. Click Refresh prices to update stops.");

        var acquireKind = cart.Targets.Length > 0 ? "add_acquired_or_create" : "add_acquired";
        var compactRoute = ImGui.GetContentRegionAvail().X < 560 * UiKit.Scale;
        foreach (var stop in route.Stops)
        {
            ImGui.PushID(stop.WorldId);
            try
            {
                UiKit.Section($"{WorldName(stop.WorldId)} · {stop.TotalGil:N0} gil ({stop.TotalUnits:N0} unit{(stop.TotalUnits == 1 ? "" : "s")})");
                if (compactRoute)
                {
                    foreach (var item in stop.Items)
                    {
                        ImGui.PushID($"{item.ItemId}-{item.RowQuality}-{item.ListingHq}");
                        try
                        {
                            UiKit.ItemIcon(ItemIcon, item.ItemId, item.ListingHq, 20f);
                            if (ImGui.Selectable($"{ItemName(item.ItemId).Replace('#', '＃')}##route-open"))
                                OpenItem(item.ItemId);

                            UiKit.QualityBadge(item.ListingHq);
                            ImGui.SameLine();
                            var priceText = item.MinUnitPrice == item.MaxUnitPrice
                                ? $"×{item.UnitsToBuy:N0} · {item.Subtotal:N0} ({item.MinUnitPrice:N0}/ea)"
                                : $"×{item.UnitsToBuy:N0} · {item.Subtotal:N0} ({item.MinUnitPrice:N0}–{item.MaxUnitPrice:N0})";
                            ImGui.TextUnformatted(priceText);

                            UiKit.Inline(110);
                            ImGui.BeginDisabled(!view.CanEdit);
                            try
                            {
                                if (ImGui.SmallButton("+1"))
                                    Edit(editTarget, new { kind = acquireKind, item_id = item.ItemId, quality = item.RowQuality, delta = 1 });
                                if (item.UnitsToBuy > 1)
                                {
                                    ImGui.SameLine();
                                    if (ImGui.SmallButton($"+{item.UnitsToBuy}"))
                                        Edit(editTarget, new { kind = acquireKind, item_id = item.ItemId, quality = item.RowQuality, delta = item.UnitsToBuy });
                                }
                            }
                            finally { ImGui.EndDisabled(); }
                            ImGui.Separator();
                        }
                        finally { ImGui.PopID(); }
                    }
                }
                else if (ImGui.BeginTable("route-stop", 5, ImGuiTableFlags.RowBg | ImGuiTableFlags.BordersInnerH | ImGuiTableFlags.SizingStretchProp))
                {
                    try
                    {
                        ImGui.TableSetupColumn("Item", ImGuiTableColumnFlags.WidthStretch, 1f);
                        ImGui.TableSetupColumn("Qual", ImGuiTableColumnFlags.WidthFixed, 46 * UiKit.Scale);
                        ImGui.TableSetupColumn("Buy", ImGuiTableColumnFlags.WidthFixed, 56 * UiKit.Scale);
                        ImGui.TableSetupColumn("Est. price", ImGuiTableColumnFlags.WidthFixed, 130 * UiKit.Scale);
                        ImGui.TableSetupColumn("Mark bought", ImGuiTableColumnFlags.WidthFixed, 118 * UiKit.Scale);
                        ImGui.TableHeadersRow();

                        foreach (var item in stop.Items)
                        {
                            ImGui.PushID($"{item.ItemId}-{item.RowQuality}-{item.ListingHq}");
                            try
                            {
                                ImGui.TableNextRow();
                                ImGui.TableNextColumn();
                                UiKit.ItemIcon(ItemIcon, item.ItemId, item.ListingHq, 20f);
                                if (ImGui.Selectable($"{ItemName(item.ItemId).Replace('#', '＃')}##route-open"))
                                    OpenItem(item.ItemId);

                                ImGui.TableNextColumn();
                                UiKit.QualityBadge(item.ListingHq);

                                ImGui.TableNextColumn();
                                ImGui.TextUnformatted($"×{item.UnitsToBuy:N0}");

                                ImGui.TableNextColumn();
                                var priceText = item.MinUnitPrice == item.MaxUnitPrice
                                    ? $"{item.Subtotal:N0} ({item.MinUnitPrice:N0}/ea)"
                                    : $"{item.Subtotal:N0} ({item.MinUnitPrice:N0}–{item.MaxUnitPrice:N0})";
                                ImGui.TextUnformatted(priceText);

                                ImGui.TableNextColumn();
                                ImGui.BeginDisabled(!view.CanEdit);
                                try
                                {
                                    if (ImGui.SmallButton("+1"))
                                        Edit(editTarget, new { kind = acquireKind, item_id = item.ItemId, quality = item.RowQuality, delta = 1 });
                                    if (item.UnitsToBuy > 1)
                                    {
                                        ImGui.SameLine();
                                        if (ImGui.SmallButton($"+{item.UnitsToBuy}"))
                                            Edit(editTarget, new { kind = acquireKind, item_id = item.ItemId, quality = item.RowQuality, delta = item.UnitsToBuy });
                                    }
                                }
                                finally { ImGui.EndDisabled(); }
                            }
                            finally { ImGui.PopID(); }
                        }
                    }
                    finally { ImGui.EndTable(); }
                }
            }
            finally { ImGui.PopID(); }
        }

        if (route.UnpricedRows.Length > 0)
        {
            UiKit.Section("Unpriced or short supply");
            UiKit.Hint("These items have no matching listings (or partial supply) in the list's price scope.");
            foreach (var row in route.UnpricedRows)
            {
                var remaining = Math.Max(0, Math.Max(1, row.Need) - Math.Max(0, row.Acquired));
                UiKit.ItemIcon(ItemIcon, row.ItemId, row.Quality == "hq", 18f);
                ImGui.TextUnformatted($"{ItemName(row.ItemId)} ({row.Quality.ToUpperInvariant()}) · {remaining:N0} remaining");
            }
        }
    }
}
