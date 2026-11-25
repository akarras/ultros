using System.Numerics;
using Dalamud.Bindings.ImGui;
using Ultros.Client;

namespace UltrosPlugin.Ui;

internal sealed class MarketPanel
{
    private static readonly string[] Qualities = ["Any", "NQ", "HQ"];
    private static readonly string[] WireQualities = ["any", "nq", "hq"];
    private static readonly string[] FilterLabels = ["All", "HQ", "NQ"];

    private string search = "";
    private string lastSearch = "";
    private string scope = "";
    private string scopeFilter = "";
    private bool automaticScope = true;
    private bool pendingMarket;
    private int itemId;
    private int addQuality;
    private int addQuantity = 1;
    private int qualityFilter;
    private bool showSales;
    private int sortColumn = 4;
    private bool sortAscending = true;
    private (int Id, string Name)[] matches = [];

    internal Func<int, string> ItemName { get; init; } = id => $"Item #{id}";
    internal Func<int, string> WorldName { get; init; } = id => id > 0 ? $"World #{id}" : "—";
    internal Func<int, bool> Marketable { get; init; } = _ => false;
    internal Func<int, bool> HasItem { get; init; } = id => id > 0;
    internal Func<string, (int Id, string Name)[]> SearchItems { get; init; } = _ => [];
    internal Func<int, bool, ImTextureID?>? ItemIcon { get; init; }
    internal Action<int, string> FetchItem { get; init; } = (_, _) => { };
    internal Action<int, string, int> AddToList { get; init; } = (_, _, _) => { };
    internal Action<int, string> OpenWebsite { get; init; } = (_, _) => { };
    internal Action<string> Notify { get; init; } = _ => { };

    internal int SelectedItemId => itemId;

    internal void SelectItem(int id, string? priceWorld = null)
    {
        itemId = id;
        pendingMarket = true;
        if (priceWorld is not null)
        {
            scope = priceWorld;
            automaticScope = false;
        }
    }

    internal void SyncScope(ClientView view)
    {
        if (automaticScope && view.PriceRegion.Length > 0) scope = view.PriceRegion;
        if (pendingMarket && scope.Length > 0 && Marketable(itemId))
        {
            pendingMarket = false;
            FetchItem(itemId, scope);
        }
    }

    internal void Draw(ClientView view)
    {
        SyncScope(view);
        var activeItemId = view.Market?.ItemId ?? itemId;

        // Clean 3-control toolbar: Search (name or numeric ID) + Scope picker + Refresh
        var avail = ImGui.GetContentRegionAvail().X;
        var scopeWidth = 155 * UiKit.Scale;
        var refreshWidth = 84 * UiKit.Scale;
        var searchWidth = avail > 540 * UiKit.Scale
            ? Math.Max(180 * UiKit.Scale, avail - scopeWidth - refreshWidth - 20 * UiKit.Scale)
            : Math.Max(160 * UiKit.Scale, avail);

        ImGui.SetNextItemWidth(searchWidth);
        var submitted = ImGui.InputTextWithHint("##market-search", "Search item name or ID…", ref search, 100, ImGuiInputTextFlags.EnterReturnsTrue);
        var trimmed = search.Trim();
        if (submitted || trimmed != lastSearch)
        {
            lastSearch = trimmed;
            matches = ResolveMatches(trimmed);
            if (submitted && matches.Length > 0)
            {
                itemId = matches[0].Id;
                matches = [];
                if (scope.Length > 0) FetchItem(itemId, scope);
            }
        }

        UiKit.Inline(250);
        DrawScopePicker(view, activeItemId, scopeWidth);

        ImGui.SameLine();
        ImGui.BeginDisabled(scope.Length == 0 || activeItemId <= 0 || !Marketable(activeItemId));
        if (ImGui.Button("Refresh##market-refresh")) FetchItem(activeItemId, scope);
        ImGui.EndDisabled();

        if (matches.Length > 0)
        {
            var listHeight = Math.Min(matches.Length, 6) * 26 * UiKit.Scale + 10 * UiKit.Scale;
            ImGui.PushStyleColor(ImGuiCol.ChildBg, UiKit.CardBg);
            try
            {
                ImGui.BeginChild("##market-matches", new Vector2(-1, listHeight), true);
                try
                {
                    foreach (var match in matches)
                    {
                        ImGui.PushID(match.Id);
                        try
                        {
                            UiKit.ItemIcon(ItemIcon, match.Id, false, 18f);
                            var label = $"{match.Name.Replace('#', '＃')}##pick";
                            if (ImGui.Selectable(label, activeItemId == match.Id))
                            {
                                itemId = match.Id;
                                matches = [];
                                search = match.Name;
                                lastSearch = match.Name;
                                if (scope.Length > 0) FetchItem(itemId, scope);
                            }
                        }
                        finally { ImGui.PopID(); }
                    }
                }
                finally { ImGui.EndChild(); }
            }
            finally { ImGui.PopStyleColor(); }
        }

        if (view.Market is not { } market)
        {
            UiKit.Empty("Search for an item to inspect prices",
                "Type an item name or ID above, or right-click any marketable item in game to load listings and recent sales.");
            return;
        }

        // Selected Item Header + Open on Ultros
        ImGui.Spacing();
        ImGui.Separator();
        UiKit.ItemIcon(ItemIcon, market.ItemId, qualityFilter == 1, 26f);
        ImGui.SetWindowFontScale(1.18f);
        ImGui.TextUnformatted(ItemName(market.ItemId));
        ImGui.SetWindowFontScale(1f);
        var fetchedLabel = market.FetchedAt is { } at
            ? $"Updated {TimestampFormat.FormatRelativeOrLocal(at)}"
            : market.Status;
        UiKit.Inline(250);
        UiKit.Muted($"{market.Scope} · {fetchedLabel}");
        UiKit.Inline(130);
        if (ImGui.SmallButton("Open on Ultros")) OpenWebsite(market.ItemId, market.Scope);

        // KPI Summary Strip
        var summary = MarketStats.Summarize(market.Listings, view.Sales);
        UiKit.KpiRow(
            new KpiCard("Cheapest NQ", summary.CheapestNq is { } nq ? $"{nq:N0} gil" : "—", $"{summary.NqCount} listing{(summary.NqCount == 1 ? "" : "s")}"),
            new KpiCard("Cheapest HQ", summary.CheapestHq is { } hq ? $"{hq:N0} gil" : "—", $"{summary.HqCount} listing{(summary.HqCount == 1 ? "" : "s")}", UiKit.HqGold),
            new KpiCard("Recent sale median", summary.MedianSalePrice is { } med ? $"{med:N0} gil" : "—", summary.SaleCount > 0 ? $"Avg {summary.AverageSalePrice:N0} · {summary.SaleCount} sales" : "No recent sales", UiKit.Accent));

        // Sub-view switcher (Listings vs Recent sales) + quality filter + Add to list
        if (UiKit.ToggleChip($"Listings ({market.Listings.Length})##mv-list", !showSales)) showSales = false;
        ImGui.SameLine();
        if (UiKit.ToggleChip($"Recent sales ({view.Sales.Length})##mv-sales", showSales)) showSales = true;

        if (!showSales)
        {
            if (UiKit.Inline(160))
            {
                UiKit.Muted("|");
                ImGui.SameLine();
            }
            for (var i = 0; i < FilterLabels.Length; i++)
            {
                if (i > 0) ImGui.SameLine();
                if (UiKit.ToggleChip($"{FilterLabels[i]}##mf-{i}", qualityFilter == i))
                {
                    qualityFilter = i;
                    addQuality = i switch { 1 => 2, 2 => 1, _ => addQuality };
                }
            }
        }

        // Quick Add to active list (cleanly separated)
        var listName = view.Cart?.Name ?? "list";
        if (listName.Length > 18) listName = listName[..18] + "…";
        if (UiKit.Inline(300))
        {
            UiKit.Muted("|");
            ImGui.SameLine();
        }
        ImGui.SetNextItemWidth(62 * UiKit.Scale);
        ImGui.Combo("##add-quality", ref addQuality, Qualities, Qualities.Length);
        ImGui.SameLine();
        ImGui.SetNextItemWidth(54 * UiKit.Scale);
        if (ImGui.InputInt("##add-qty", ref addQuantity, 0, 0)) addQuantity = Math.Clamp(addQuantity, 1, 1_000_000);
        ImGui.SameLine();
        if (UiKit.PrimaryButton($"+ {listName}##market-add"))
            AddToList(market.ItemId, WireQualities[addQuality], Math.Clamp(addQuantity, 1, 1_000_000));
        if (ImGui.IsItemHovered()) ImGui.SetTooltip($"Add {addQuantity:N0} ({Qualities[addQuality]}) to {view.Cart?.Name ?? "your active list"}");

        if (showSales) DrawSalesTable(view.Sales);
        else DrawListingsTable(market.Listings);
    }

    private (int Id, string Name)[] ResolveMatches(string trimmed)
    {
        if (trimmed.Length == 0) return [];
        if (int.TryParse(trimmed, out var parsedId) && parsedId > 0 && Marketable(parsedId))
            return [(parsedId, $"{ItemName(parsedId)} (#{parsedId})")];
        return trimmed.Length >= 2 ? SearchItems(trimmed) : [];
    }

    private void DrawScopePicker(ClientView view, int activeItemId, float width)
    {
        ImGui.SetNextItemWidth(width);
        var display = scope.Length > 0 ? scope : "Select scope";
        if (ImGui.BeginCombo("##price-scope", display))
        {
            try
            {
                ImGui.SetNextItemWidth(-1);
                var enter = ImGui.InputTextWithHint("##scope-filter", "Filter or type zone…", ref scopeFilter, 64, ImGuiInputTextFlags.EnterReturnsTrue);
                var filter = scopeFilter.Trim();
                if (enter && filter.Length > 0)
                {
                    ApplyScope(filter, activeItemId, automatic: false);
                    scopeFilter = "";
                    ImGui.CloseCurrentPopup();
                }

                var options = BuildScopeOptions(view);
                if (filter.Length > 0)
                    options = options.Where(o => o.Scope.Contains(filter, StringComparison.OrdinalIgnoreCase) || o.Label.Contains(filter, StringComparison.OrdinalIgnoreCase)).ToArray();

                ImGui.Separator();
                foreach (var opt in options.Take(30))
                {
                    var selected = opt.Scope.Equals(scope, StringComparison.OrdinalIgnoreCase);
                    if (ImGui.Selectable($"{opt.Label}##sc-{opt.Scope}", selected))
                    {
                        ApplyScope(opt.Scope, activeItemId, automatic: opt.IsRegionDefault);
                        scopeFilter = "";
                    }
                }
            }
            finally { ImGui.EndCombo(); }
        }
        if (ImGui.IsItemHovered()) ImGui.SetTooltip("Price lookup scope (Region, Data Center, or World)");
    }

    private void ApplyScope(string nextScope, int activeItemId, bool automatic)
    {
        scope = nextScope;
        automaticScope = automatic;
        if (activeItemId > 0 && Marketable(activeItemId)) FetchItem(activeItemId, scope);
    }

    private static (string Scope, string Label, bool IsRegionDefault)[] BuildScopeOptions(ClientView view)
    {
        var result = new List<(string Scope, string Label, bool IsRegionDefault)>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        void Add(string s, string label, bool regionDefault = false)
        {
            if (s.Length > 0 && seen.Add(s)) result.Add((s, label, regionDefault));
        }

        if (view.PriceRegion.Length > 0) Add(view.PriceRegion, $"{view.PriceRegion} (Region)", regionDefault: true);
        if (view.CurrentDatacenter.Length > 0) Add(view.CurrentDatacenter, $"{view.CurrentDatacenter} (Your DC)");
        if (view.CurrentWorld.Length > 0) Add(view.CurrentWorld, $"{view.CurrentWorld} (Your World)");
        if (view.ScopeOptions is { Length: > 0 } opts)
        {
            foreach (var opt in opts) Add(opt, opt);
        }
        return result.ToArray();
    }

    private void DrawListingsTable(MarketRow[] listings)
    {
        var filtered = listings
            .Where(row => qualityFilter == 0 || (qualityFilter == 1 ? row.Hq : !row.Hq))
            .ToArray();

        if (filtered.Length == 0)
        {
            UiKit.Empty("No matching listings", listings.Length == 0
                ? "No cached listings were found for this item and scope."
                : "No listings match the current quality filter.");
            return;
        }

        var sorted = SortListings(filtered);
        var wide = ImGui.GetContentRegionAvail().X >= 660 * UiKit.Scale;
        var columns = wide ? 6 : 5;
        const ImGuiTableFlags flags = ImGuiTableFlags.RowBg | ImGuiTableFlags.BordersInnerH | ImGuiTableFlags.Sortable | ImGuiTableFlags.SizingStretchProp;
        if (!ImGui.BeginTable("market-listings", columns, flags)) return;
        try
        {
            ImGui.TableSetupColumn("World", ImGuiTableColumnFlags.WidthFixed, 108 * UiKit.Scale);
            ImGui.TableSetupColumn("Retainer", ImGuiTableColumnFlags.WidthStretch, 1f);
            ImGui.TableSetupColumn("Qual", ImGuiTableColumnFlags.WidthFixed, 42 * UiKit.Scale);
            ImGui.TableSetupColumn("Qty", ImGuiTableColumnFlags.WidthFixed, 42 * UiKit.Scale);
            ImGui.TableSetupColumn("Unit price", ImGuiTableColumnFlags.WidthFixed | ImGuiTableColumnFlags.DefaultSort, 94 * UiKit.Scale);
            if (wide) ImGui.TableSetupColumn("Total", ImGuiTableColumnFlags.WidthFixed, 96 * UiKit.Scale);
            ImGui.TableHeadersRow();

            var sortSpecs = ImGui.TableGetSortSpecs();
            if (!sortSpecs.IsNull && sortSpecs.SpecsDirty && sortSpecs.SpecsCount > 0)
            {
                sortColumn = sortSpecs.Specs.ColumnIndex;
                sortAscending = sortSpecs.Specs.SortDirection != ImGuiSortDirection.Descending;
                sortSpecs.SpecsDirty = false;
                sorted = SortListings(filtered);
            }

            for (var idx = 0; idx < sorted.Length; idx++)
            {
                var row = sorted[idx];
                ImGui.PushID(idx);
                try
                {
                    ImGui.TableNextRow();
                    ImGui.TableNextColumn();
                    ImGui.TextUnformatted(WorldName(row.WorldId));
                    ImGui.TableNextColumn();
                    ImGui.TextUnformatted(row.Retainer);
                    ImGui.TableNextColumn();
                    UiKit.QualityBadge(row.Hq);
                    ImGui.TableNextColumn();
                    ImGui.TextUnformatted(row.Quantity.ToString("N0"));
                    ImGui.TableNextColumn();
                    if (ImGui.Selectable($"{row.UnitPrice:N0}##price"))
                    {
                        ImGui.SetClipboardText(row.UnitPrice.ToString());
                        Notify($"Copied {row.UnitPrice:N0} gil to clipboard.");
                    }
                    if (ImGui.IsItemHovered()) ImGui.SetTooltip("Click to copy unit price");
                    if (wide)
                    {
                        ImGui.TableNextColumn();
                        UiKit.Muted($"{row.UnitPrice * row.Quantity:N0}");
                    }
                }
                finally { ImGui.PopID(); }
            }
        }
        finally { ImGui.EndTable(); }
    }

    private void DrawSalesTable(SaleRow[] sales)
    {
        if (sales.Length == 0)
        {
            UiKit.Empty("No recent sales", "No recent sales are cached for this item and scope.");
            return;
        }
        var wide = ImGui.GetContentRegionAvail().X >= 660 * UiKit.Scale;
        var columns = wide ? 6 : 5;
        if (!ImGui.BeginTable("market-sales", columns, ImGuiTableFlags.RowBg | ImGuiTableFlags.BordersInnerH | ImGuiTableFlags.SizingStretchProp))
            return;
        try
        {
            ImGui.TableSetupColumn("When", ImGuiTableColumnFlags.WidthStretch, 1f);
            ImGui.TableSetupColumn("World", ImGuiTableColumnFlags.WidthFixed, 108 * UiKit.Scale);
            ImGui.TableSetupColumn("Qual", ImGuiTableColumnFlags.WidthFixed, 42 * UiKit.Scale);
            ImGui.TableSetupColumn("Qty", ImGuiTableColumnFlags.WidthFixed, 42 * UiKit.Scale);
            ImGui.TableSetupColumn("Unit price", ImGuiTableColumnFlags.WidthFixed, 94 * UiKit.Scale);
            if (wide) ImGui.TableSetupColumn("Total", ImGuiTableColumnFlags.WidthFixed, 96 * UiKit.Scale);
            ImGui.TableHeadersRow();
            foreach (var sale in sales)
            {
                ImGui.TableNextRow();
                ImGui.TableNextColumn();
                ImGui.TextUnformatted(TimestampFormat.FormatSaleTime(sale.SoldAt));
                ImGui.TableNextColumn();
                ImGui.TextUnformatted(sale.WorldId > 0 ? WorldName(sale.WorldId) : "—");
                ImGui.TableNextColumn();
                UiKit.QualityBadge(sale.Hq);
                ImGui.TableNextColumn();
                ImGui.TextUnformatted(sale.Quantity.ToString("N0"));
                ImGui.TableNextColumn();
                ImGui.TextUnformatted($"{sale.UnitPrice:N0}");
                if (wide)
                {
                    ImGui.TableNextColumn();
                    UiKit.Muted($"{sale.UnitPrice * sale.Quantity:N0}");
                }
            }
        }
        finally { ImGui.EndTable(); }
    }

    private MarketRow[] SortListings(MarketRow[] rows)
    {
        IEnumerable<MarketRow> ordered = sortColumn switch
        {
            0 => sortAscending ? rows.OrderBy(r => WorldName(r.WorldId), StringComparer.OrdinalIgnoreCase).ThenBy(r => r.UnitPrice)
                : rows.OrderByDescending(r => WorldName(r.WorldId), StringComparer.OrdinalIgnoreCase).ThenBy(r => r.UnitPrice),
            1 => sortAscending ? rows.OrderBy(r => r.Retainer, StringComparer.OrdinalIgnoreCase).ThenBy(r => r.UnitPrice)
                : rows.OrderByDescending(r => r.Retainer, StringComparer.OrdinalIgnoreCase).ThenBy(r => r.UnitPrice),
            2 => sortAscending ? rows.OrderBy(r => r.Hq).ThenBy(r => r.UnitPrice)
                : rows.OrderByDescending(r => r.Hq).ThenBy(r => r.UnitPrice),
            3 => sortAscending ? rows.OrderBy(r => r.Quantity).ThenBy(r => r.UnitPrice)
                : rows.OrderByDescending(r => r.Quantity).ThenBy(r => r.UnitPrice),
            5 => sortAscending ? rows.OrderBy(r => r.UnitPrice * r.Quantity).ThenBy(r => r.UnitPrice)
                : rows.OrderByDescending(r => r.UnitPrice * r.Quantity).ThenBy(r => r.UnitPrice),
            _ => sortAscending ? rows.OrderBy(r => r.UnitPrice).ThenByDescending(r => r.Quantity)
                : rows.OrderByDescending(r => r.UnitPrice).ThenByDescending(r => r.Quantity),
        };
        return ordered.ToArray();
    }
}
