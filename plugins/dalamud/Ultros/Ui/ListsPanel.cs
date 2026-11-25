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
    private int addItemId;
    private int addQuality;
    private int addQuantity = 1;
    private bool hideAcquired;
    private bool showRoute;

    internal Func<int, string> ItemName { get; init; } = id => $"Item #{id}";
    internal Func<int, string> WorldName { get; init; } = id => id > 0 ? $"World #{id}" : "—";
    internal Func<int, bool> HasItem { get; init; } = id => id > 0;
    internal Func<string, (int Id, string Name)[]> SearchAllItems { get; init; } = _ => [];
    internal Func<int, bool, ImTextureID?>? ItemIcon { get; init; }
    internal Action<CartCommandTarget?> RefreshAll { get; init; } = _ => { };
    internal Action<int> SelectList { get; init; } = _ => { };
    internal Action<CartCommandTarget?> RetrySave { get; init; } = _ => { };
    internal Action<RecoveryChoiceTarget?> RetryRecovery { get; init; } = _ => { };
    internal Action<RecoveryChoiceTarget?> UseServerCopy { get; init; } = _ => { };
    internal Action<ListEditTarget, int, string, int> AddItem { get; init; } = (_, _, _, _) => { };
    internal Action<CartCommandTarget?, object> Edit { get; init; } = (_, _) => { };
    internal Action<CartCommandTarget?, object[]> EditBatch { get; init; } = (_, _) => { };
    internal Action<int> OpenItem { get; init; } = _ => { };
    internal Action<string> Notify { get; init; } = _ => { };

    internal void Draw(ClientView view, (string Title, string Message, string ActionLabel, Action OnAction)? buildBanner = null)
    {
        if (buildBanner is { } banner)
            UiKit.Banner("lists-build", banner.Title, banner.Message, banner.ActionLabel, banner.OnAction, warning: true);

        var cartTarget = CartCommandTarget.Capture(view);
        var recoveryTarget = RecoveryChoiceTarget.Capture(view);

        // Single streamlined toolbar: List selector + Status pill + unified Refresh + Undo/Redo
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

        if (!view.Connected)
        {
            UiKit.Empty("Connect to sync lists", "Open Account to connect this game client to Ultros.");
            return;
        }
        if (view.Cart is not { } cart)
        {
            UiKit.Inline(100);
            if (ImGui.Button("Refresh##lists-refresh")) RefreshAll(null);
            UiKit.Empty("No active list selected", "Your online Cart is selected automatically when available. Create a list on Ultros, or pick one above.");
            return;
        }

        UiKit.Inline(90);
        if (cart.AwaitingPersist) UiKit.Pill("UNSAVED", UiKit.Danger);
        else if (cart.RecoveryReview) UiKit.Pill("RECOVERY", UiKit.Warning);
        else UiKit.Pill(view.ListTransport == "Live updates" ? "LIVE" : "POLLING", view.ListTransport == "Live updates" ? UiKit.Success : UiKit.Accent);

        UiKit.Inline(110);
        if (cart.AwaitingPersist)
        {
            if (UiKit.PrimaryButton("Retry save")) RetrySave(cartTarget);
        }
        else
        {
            ImGui.BeginDisabled(view.Estimating);
            if (ImGui.Button(view.Estimating ? "Refreshing…" : "Refresh##lists-refresh")) RefreshAll(cartTarget);
            ImGui.EndDisabled();
            if (ImGui.IsItemHovered()) ImGui.SetTooltip("Sync lists and refresh market estimates");
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
            if (UiKit.PrimaryButton("Retry recovery")) RetryRecovery(recoveryTarget);
            if (cart.HasServerCopy)
            {
                ImGui.SameLine();
                if (ImGui.Button("Keep server copy (back up local)")) UseServerCopy(recoveryTarget);
            }
            return;
        }

        // Combined 3-card Summary Row (Progress + Cost + Stops)
        var progress = ShoppingRoute.Progress(cart.Rows);
        var route = ShoppingRoute.Build(cart.Rows, view.Estimate, view.CurrentWorldId);

        var estimateCurrent = view.Estimate is { } est && est.Version == cart.Version;
        var costValue = view.Estimate is { } e ? $"{e.Total:N0} gil" : view.Estimating ? "Calculating…" : "—";
        var costSub = view.Estimate is { } e2
            ? (estimateCurrent
                ? $"{e2.Scope} · {TimestampFormat.FormatRelativeOrLocal(e2.ObservedAt)}"
                : $"Outdated · {e2.Scope}")
            : "Refresh to estimate";
        var routeValue = route.Stops.Length > 0 ? $"{route.Stops.Length} world{(route.Stops.Length == 1 ? "" : "s")}" : progress.RemainingUnits == 0 ? "Done" : "—";
        var routeSub = view.Estimate is { UnpricedUnits: > 0 } ep
            ? $"{ep.UnpricedUnits:N0} unpriced units"
            : progress.RemainingUnits > 0 ? $"{progress.RemainingUnits:N0} units left" : "All items acquired";

        UiKit.KpiRow(
            new KpiCard("Progress", $"{progress.TotalAcquired:N0} / {progress.TotalNeed:N0}", $"{progress.CompletedRows} of {progress.TotalRows} items", null, progress.Fraction),
            new KpiCard("Estimated remaining", costValue, costSub, estimateCurrent ? UiKit.Accent : UiKit.Warning),
            new KpiCard("Shopping stops", routeValue, routeSub, route.Stops.Length > 0 ? UiKit.Success : null));

        // Sub-view switcher + Hide acquired
        if (UiKit.ToggleChip($"Items ({cart.Rows.Length})##lv-items", !showRoute)) showRoute = false;
        ImGui.SameLine();
        var routeLabel = route.Stops.Length > 0 ? $"Shopping Route ({route.Stops.Length})##lv-route" : "Shopping Route##lv-route";
        if (UiKit.ToggleChip(routeLabel, showRoute)) showRoute = true;
        if (!showRoute)
        {
            UiKit.Inline(150);
            ImGui.Checkbox("Hide acquired", ref hideAcquired);
        }

        if (view.CanEdit && !showRoute) DrawAddBar(view);

        if (showRoute) DrawRoute(view, cart, cartTarget, route);
        else DrawItems(view, cart, cartTarget);
    }

    private void DrawAddBar(ClientView view)
    {
        var avail = ImGui.GetContentRegionAvail().X;
        ImGui.SetNextItemWidth(Math.Max(150 * UiKit.Scale, Math.Min(260 * UiKit.Scale, avail - 230 * UiKit.Scale)));
        var enter = ImGui.InputTextWithHint("##list-add-search", "Add item by name or ID…", ref addSearch, 100, ImGuiInputTextFlags.EnterReturnsTrue);
        var trimmed = addSearch.Trim();
        if (enter || trimmed != lastAddSearch)
        {
            lastAddSearch = trimmed;
            addMatches = ResolveAddMatches(trimmed);
            if (enter && addMatches.Length > 0)
            {
                addItemId = addMatches[0].Id;
                addSearch = ItemName(addItemId);
                lastAddSearch = addSearch;
                addMatches = [];
            }
        }

        UiKit.Inline(215);
        ImGui.SetNextItemWidth(68 * UiKit.Scale);
        ImGui.Combo("##list-add-qual", ref addQuality, Qualities, Qualities.Length);
        ImGui.SameLine();
        ImGui.SetNextItemWidth(62 * UiKit.Scale);
        if (ImGui.InputInt("##list-add-qty", ref addQuantity, 0, 0)) addQuantity = Math.Clamp(addQuantity, 1, 1_000_000);
        ImGui.SameLine();
        if (UiKit.PrimaryButton("Add##list-add"))
        {
            if (addItemId <= 0 && int.TryParse(addSearch.Trim(), out var parsedId) && HasItem(parsedId))
                addItemId = parsedId;
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
                                addSearch = ItemName(match.Id);
                                lastAddSearch = addSearch;
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

    private (int Id, string Name)[] ResolveAddMatches(string trimmed)
    {
        if (trimmed.Length == 0) return [];
        if (int.TryParse(trimmed, out var parsedId) && HasItem(parsedId))
            return [(parsedId, $"{ItemName(parsedId)} (#{parsedId})")];
        return trimmed.Length >= 2 ? SearchAllItems(trimmed) : [];
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

        if (ImGui.GetContentRegionAvail().X < 700 * UiKit.Scale)
        {
            DrawItemsCompact(view, visibleRows, editTarget);
        }
        else if (ImGui.BeginTable("cart-v3", 7, ImGuiTableFlags.RowBg | ImGuiTableFlags.BordersInnerH | ImGuiTableFlags.SizingStretchProp))
        {
            try
            {
                ImGui.TableSetupColumn("Item", ImGuiTableColumnFlags.WidthStretch, 1f);
                ImGui.TableSetupColumn("Qual", ImGuiTableColumnFlags.WidthFixed, 42 * UiKit.Scale);
                ImGui.TableSetupColumn("Need", ImGuiTableColumnFlags.WidthFixed, 62 * UiKit.Scale);
                ImGui.TableSetupColumn("Have", ImGuiTableColumnFlags.WidthFixed, 62 * UiKit.Scale);
                ImGui.TableSetupColumn("Est. cost", ImGuiTableColumnFlags.WidthFixed, 102 * UiKit.Scale);
                ImGui.TableSetupColumn("Quick", ImGuiTableColumnFlags.WidthFixed, 84 * UiKit.Scale);
                ImGui.TableSetupColumn("", ImGuiTableColumnFlags.WidthFixed, 30 * UiKit.Scale);
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
                            DrawQuickButtons(key, row, done, editTarget);

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
                    UiKit.Inline(104);
                    UiKit.Muted("Need");
                    ImGui.SameLine();
                    ImGui.SetNextItemWidth(54 * UiKit.Scale);
                    var (commit, needActive) = DrawNeedInput(key, row, ref draft, editTarget);

                    UiKit.Inline(104);
                    UiKit.Muted("Have");
                    ImGui.SameLine();
                    ImGui.SetNextItemWidth(54 * UiKit.Scale);
                    var (acquired, acquiredActive) = DrawAcquiredInput(key, row, ref draft, editTarget);
                    if (!commit && !acquired && (needActive || acquiredActive)) drafts[key] = draft;
                    else drafts.Remove(key);

                    UiKit.Inline(120);
                    DrawQuickButtons(key, row, done, editTarget);
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
        if (ImGui.IsItemHovered()) ImGui.SetTooltip("Click to inspect in Market");
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

    private void DrawQuickButtons(string key, CartRow row, bool done, CartCommandTarget? editTarget)
    {
        if (done)
        {
            if (ImGui.SmallButton("-1"))
            {
                drafts.Remove(key);
                Edit(editTarget, new { kind = "add_acquired", item_id = row.ItemId, quality = row.Quality, delta = -1 });
            }
            if (ImGui.IsItemHovered()) ImGui.SetTooltip("Subtract 1 acquired");
            return;
        }
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
        if (UiKit.DangerButton("×##rm", small: true))
        {
            drafts.Remove(key);
            Edit(editTarget, new { kind = "remove", item_id = row.ItemId, quality = row.Quality });
        }
        if (ImGui.IsItemHovered()) ImGui.SetTooltip("Remove from list");
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
                "Click Refresh above to group your remaining items by cheapest world.");
            return;
        }
        if (route.Stops.Length == 0 && route.UnpricedRows.Length == 0)
        {
            UiKit.Empty("Nothing left to buy", "Every item on this list is already marked acquired.");
            return;
        }

        if (view.Estimate.Version != cart.Version)
            UiKit.Hint("Prices are from before your latest edits. Click Refresh to update stops.");

        var compactRoute = ImGui.GetContentRegionAvail().X < 560 * UiKit.Scale;
        foreach (var stop in route.Stops)
        {
            var isCurrentWorld = view.CurrentWorldId > 0 && stop.WorldId == view.CurrentWorldId;
            ImGui.PushID(stop.WorldId);
            try
            {
                ImGui.Spacing();
                ImGui.Separator();
                ImGui.SetWindowFontScale(1.12f);
                ImGui.TextUnformatted($"{WorldName(stop.WorldId)} · {stop.TotalGil:N0} gil ({stop.TotalUnits:N0} unit{(stop.TotalUnits == 1 ? "" : "s")})");
                ImGui.SetWindowFontScale(1f);
                if (isCurrentWorld)
                {
                    ImGui.SameLine();
                    UiKit.Pill("YOU ARE HERE", UiKit.Success);
                }
                if (view.CanEdit && stop.Items.Length > 0)
                {
                    UiKit.Inline(140);
                    if (ImGui.SmallButton("Mark stop bought"))
                    {
                        var edits = stop.Items
                            .Select(i => (object)new { kind = "add_acquired", item_id = i.ItemId, quality = i.RowQuality, delta = i.UnitsToBuy })
                            .ToArray();
                        EditBatch(editTarget, edits);
                    }
                    if (ImGui.IsItemHovered()) ImGui.SetTooltip($"Mark all {stop.TotalUnits:N0} units on {WorldName(stop.WorldId)} acquired");
                }

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
                        ImGui.TableSetupColumn("Qual", ImGuiTableColumnFlags.WidthFixed, 42 * UiKit.Scale);
                        ImGui.TableSetupColumn("Buy", ImGuiTableColumnFlags.WidthFixed, 52 * UiKit.Scale);
                        ImGui.TableSetupColumn("Est. price", ImGuiTableColumnFlags.WidthFixed, 134 * UiKit.Scale);
                        ImGui.TableSetupColumn("Bought", ImGuiTableColumnFlags.WidthFixed, 96 * UiKit.Scale);
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
