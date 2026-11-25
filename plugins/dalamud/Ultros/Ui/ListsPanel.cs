using System.Numerics;
using Dalamud.Bindings.ImGui;
using Ultros.Client;
using Ultros.Lists;

namespace UltrosPlugin.Ui;

internal sealed class ListsPanel
{
    private static readonly string[] Qualities = ["Any", "NQ", "HQ"];
    private static readonly string[] WireQualities = ["any", "nq", "hq"];

    private readonly Dictionary<string, (int Need, int Acquired)> drafts = [];
    private string addSearch = "";
    private string lastAddSearch = "";
    private (int Id, string Name)[] addMatches = [];
    private int addItemId = 8;
    private int addQuality;
    private int addQuantity = 1;
    private bool hideAcquired;
    private bool showRoute;

    internal Func<int, string> ItemName { get; init; } = id => $"Item #{id}";
    internal Func<int, string> WorldName { get; init; } = id => id > 0 ? $"World #{id}" : "—";
    internal Func<int, bool> HasItem { get; init; } = id => id > 0;
    internal Func<string, (int Id, string Name)[]> SearchAllItems { get; init; } = _ => [];
    internal Func<int, bool, ImTextureID?>? ItemIcon { get; init; }
    internal Action Refresh { get; init; } = () => { };
    internal Action<int> SelectList { get; init; } = _ => { };
    internal Action<CartCommandTarget?> RetrySave { get; init; } = _ => { };
    internal Action<CartCommandTarget?> RefreshEstimate { get; init; } = _ => { };
    internal Action<RecoveryChoiceTarget?> RetryRecovery { get; init; } = _ => { };
    internal Action<RecoveryChoiceTarget?> UseServerCopy { get; init; } = _ => { };
    internal Action<ListEditTarget, int, string, int> AddItem { get; init; } = (_, _, _, _) => { };
    internal Action<CartCommandTarget?, object> Edit { get; init; } = (_, _) => { };
    internal Action<int> OpenItem { get; init; } = _ => { };
    internal Action<string> Notify { get; init; } = _ => { };

    internal void Draw(ClientView view, (string Title, string Message, string ActionLabel, Action OnAction)? buildBanner = null)
    {
        UiKit.Heading("Lists", "Plan purchases, follow your shopping route by world, and track acquired quantities live.");

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

        // Progress bar + Estimate KPI cards
        var progress = ShoppingRoute.Progress(cart.Rows);
        var route = ShoppingRoute.Build(cart.Rows, view.Estimate);
        if (cart.Rows.Length > 0)
        {
            UiKit.ProgressBar(progress.Fraction, $"{progress.TotalAcquired:N0} / {progress.TotalNeed:N0} units · {progress.CompletedRows} of {progress.TotalRows} items complete");
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

        UiKit.KpiRow(
            new KpiCard("Estimated remaining", costValue, costSub, estimateCurrent ? UiKit.Accent : UiKit.Warning),
            new KpiCard("Shopping stops", routeValue, routeSub, route.Stops.Length > 0 ? UiKit.Success : null));

        // Sub-view switcher + Add Item bar
        DrawViewButton("Items", !showRoute, () => showRoute = false);
        ImGui.SameLine();
        var routeLabel = route.Stops.Length > 0 ? $"Shopping Route ({route.Stops.Length})" : "Shopping Route";
        DrawViewButton(routeLabel, showRoute, () => showRoute = true);
        if (!showRoute)
        {
            UiKit.Inline(150);
            ImGui.Checkbox("Hide acquired", ref hideAcquired);
        }

        if (view.CanEdit && !showRoute) DrawAddBar(view);

        if (showRoute) DrawRoute(view, cart, cartTarget, route);
        else DrawItems(view, cart, cartTarget);

        if (view.CanEdit) DrawAddByIdFallback(view);
    }

    private void DrawViewButton(string label, bool active, Action onClick)
    {
        if (active) ImGui.PushStyleColor(ImGuiCol.Button, UiKit.Accent);
        try { if (ImGui.Button(label)) onClick(); }
        finally { if (active) ImGui.PopStyleColor(); }
    }

    private void DrawAddBar(ClientView view)
    {
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

    private void DrawItems(ClientView view, CartView cart, CartCommandTarget? editTarget)
    {
        var visibleRows = cart.Rows
            .Where(row => !hideAcquired || row.Acquired < Math.Max(1, row.Need))
            .ToArray();

        if (visibleRows.Length == 0)
        {
            UiKit.Empty(cart.Rows.Length == 0 ? "This list is empty" : "Everything is acquired",
                cart.Rows.Length == 0 ? "Search for an item above or right-click items in game to add them." : "Uncheck Hide acquired to view completed rows.");
            return;
        }

        if (ImGui.GetContentRegionAvail().X < 740 * UiKit.Scale)
        {
            DrawItemsCompact(view, visibleRows, editTarget);
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
                            var (commit, needActive) = DrawNeedInput(key, row, ref draft, editTarget);

                            ImGui.TableNextColumn();
                            ImGui.SetNextItemWidth(-1);
                            var (acquired, acquiredActive) = DrawAcquiredInput(key, row, ref draft, editTarget);
                            if (!commit && !acquired && (needActive || acquiredActive)) drafts[key] = draft;
                            else drafts.Remove(key);

                            ImGui.TableNextColumn();
                            DrawRowEstimate(line, done);

                            ImGui.TableNextColumn();
                            DrawQuickButtons(key, row, editTarget);

                            ImGui.TableNextColumn();
                            DrawRemoveButton(key, row, editTarget);
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

    private void DrawItemsCompact(ClientView view, CartRow[] visibleRows, CartCommandTarget? editTarget)
    {
        foreach (var row in visibleRows)
        {
            var key = $"{view.SelectedList}/{row.ItemId}/{row.Quality}";
            (int Need, int Acquired) draft = drafts.GetValueOrDefault(key, ((int)Math.Clamp(row.Need, 0, int.MaxValue), (int)Math.Clamp(row.Acquired, 0, int.MaxValue)));
            var line = ShoppingRoute.FindLine(view.Estimate, row);
            var done = row.Acquired >= Math.Max(1, row.Need);

            ImGui.PushID(key);
            try
            {
                DrawItemName(row, done);

                UiKit.QualityBadge(row.Quality);
                ImGui.SameLine();
                DrawRowEstimate(line, done);

                ImGui.BeginDisabled(!view.CanEdit);
                try
                {
                    UiKit.Inline(108);
                    UiKit.Muted("Need");
                    ImGui.SameLine();
                    ImGui.SetNextItemWidth(56 * UiKit.Scale);
                    var (commit, needActive) = DrawNeedInput(key, row, ref draft, editTarget);

                    UiKit.Inline(108);
                    UiKit.Muted("Have");
                    ImGui.SameLine();
                    ImGui.SetNextItemWidth(56 * UiKit.Scale);
                    var (acquired, acquiredActive) = DrawAcquiredInput(key, row, ref draft, editTarget);
                    if (!commit && !acquired && (needActive || acquiredActive)) drafts[key] = draft;
                    else drafts.Remove(key);

                    UiKit.Inline(155);
                    DrawQuickButtons(key, row, editTarget);
                    ImGui.SameLine();
                    DrawRemoveButton(key, row, editTarget);
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

    private (bool Committed, bool Active) DrawAcquiredInput(string key, CartRow row, ref (int Need, int Acquired) draft, CartCommandTarget? editTarget)
    {
        var acquired = ImGui.InputInt("##acquired", ref draft.Acquired, 0, 0, "%d", ImGuiInputTextFlags.EnterReturnsTrue);
        var active = ImGui.IsItemActive();
        acquired |= ImGui.IsItemDeactivatedAfterEdit();
        if (acquired)
        {
            Edit(editTarget, new { kind = "set_acquired", item_id = row.ItemId, quality = row.Quality, value = draft.Acquired });
            drafts.Remove(key);
        }
        return (acquired, active);
    }

    private void DrawQuickButtons(string key, CartRow row, CartCommandTarget? editTarget)
    {
        if (ImGui.SmallButton("+1"))
        {
            drafts.Remove(key);
            Edit(editTarget, new { kind = "add_acquired", item_id = row.ItemId, quality = row.Quality, delta = 1 });
        }
        ImGui.SameLine();
        if (ImGui.SmallButton("+5"))
        {
            drafts.Remove(key);
            Edit(editTarget, new { kind = "add_acquired", item_id = row.ItemId, quality = row.Quality, delta = 5 });
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

    private void DrawRoute(ClientView view, CartView cart, CartCommandTarget? editTarget, ShoppingRoutePlan route)
    {
        if (view.Estimate is null)
        {
            UiKit.Empty(view.Estimating ? "Building shopping route…" : "No price estimate loaded yet",
                "Click Refresh prices above to group your remaining items by cheapest world.");
            return;
        }
        if (route.Stops.Length == 0 && route.UnpricedRows.Length == 0)
        {
            UiKit.Empty("Nothing left to buy", "Every item on this list is already marked acquired.");
            return;
        }

        if (view.Estimate.Version != cart.Version)
            UiKit.Hint("Prices are from before your latest edits. Click Refresh prices to update stops.");

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
                                    Edit(editTarget, new { kind = "add_acquired", item_id = item.ItemId, quality = item.RowQuality, delta = 1 });
                                if (item.UnitsToBuy > 1)
                                {
                                    ImGui.SameLine();
                                    if (ImGui.SmallButton($"+{item.UnitsToBuy}"))
                                        Edit(editTarget, new { kind = "add_acquired", item_id = item.ItemId, quality = item.RowQuality, delta = item.UnitsToBuy });
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
                                        Edit(editTarget, new { kind = "add_acquired", item_id = item.ItemId, quality = item.RowQuality, delta = 1 });
                                    if (item.UnitsToBuy > 1)
                                    {
                                        ImGui.SameLine();
                                        if (ImGui.SmallButton($"+{item.UnitsToBuy}"))
                                            Edit(editTarget, new { kind = "add_acquired", item_id = item.ItemId, quality = item.RowQuality, delta = item.UnitsToBuy });
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
