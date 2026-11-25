using System.Numerics;
using Dalamud.Bindings.ImGui;
using Ultros.Client;

namespace UltrosPlugin.Ui;

internal sealed class MarketPanel
{
    private static readonly string[] Qualities = ["Any", "NQ", "HQ"];
    private static readonly string[] WireQualities = ["any", "nq", "hq"];
    private static readonly string[] FilterLabels = ["All", "HQ only", "NQ only"];

    private string search = "";
    private string scope = "";
    private bool automaticScope = true;
    private bool pendingMarket;
    private int itemId = 8;
    private int addQuality;
    private int addQuantity = 1;
    private int qualityFilter;
    private int sortColumn = 4;
    private bool sortAscending = true;
    private (int Id, string Name)[] matches = [];
    private string lastSearch = "";

    internal Func<int, string> ItemName { get; init; } = id => $"Item #{id}";
    internal Func<int, string> WorldName { get; init; } = id => id > 0 ? $"World #{id}" : "—";
    internal Func<int, bool> Marketable { get; init; } = _ => false;
    internal Func<int, bool> HasItem { get; init; } = id => id > 0;
    internal Func<string, (int Id, string Name)[]> SearchItems { get; init; } = _ => [];
    internal Func<int, bool, ImTextureID?>? ItemIcon { get; init; }
    internal Action<int, string> FetchItem { get; init; } = (_, _) => { };
    internal Action<int, string, int> AddToList { get; init; } = (_, _, _) => { };
    internal Action<int, string> OpenWebsite { get; init; } = (_, _) => { };

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
        UiKit.Heading("Market", "Search marketable items, compare listings across worlds, or right-click any item in game.");
        SyncScope(view);

        // Compact single-row (wrapping on narrow windows) search + scope toolbar
        var avail = ImGui.GetContentRegionAvail().X;
        var searchWidth = avail > 760 * UiKit.Scale ? 230 * UiKit.Scale : Math.Max(160 * UiKit.Scale, avail - 90 * UiKit.Scale);
        ImGui.SetNextItemWidth(searchWidth);
        var submitted = ImGui.InputTextWithHint("##market-search", "Search item name…", ref search, 100, ImGuiInputTextFlags.EnterReturnsTrue);
        ImGui.SameLine();
        submitted |= ImGui.Button("Search");
        var trimmed = search.Trim();
        if (submitted || (trimmed.Length >= 2 && trimmed != lastSearch))
        {
            lastSearch = trimmed;
            matches = trimmed.Length >= 2 ? SearchItems(trimmed) : [];
            if (submitted && matches.Length > 0)
            {
                itemId = matches[0].Id;
                if (scope.Length > 0) FetchItem(itemId, scope);
            }
        }
        else if (trimmed.Length < 2 && matches.Length > 0)
        {
            lastSearch = "";
            matches = [];
        }

        UiKit.Inline(330);
        ImGui.SetNextItemWidth(140 * UiKit.Scale);
        if (ImGui.InputTextWithHint("##price-zone", "Region / DC / World", ref scope, 64, ImGuiInputTextFlags.EnterReturnsTrue))
        {
            automaticScope = false;
            if (scope.Length > 0 && Marketable(itemId)) FetchItem(itemId, scope);
        }
        else if (ImGui.IsItemEdited()) automaticScope = false;

        ImGui.SameLine();
        if (ImGui.Button("Region") && view.PriceRegion.Length > 0)
        {
            automaticScope = true;
            scope = view.PriceRegion;
            if (Marketable(itemId)) FetchItem(itemId, scope);
        }
        if (ImGui.IsItemHovered()) ImGui.SetTooltip("Use your current character's region.");
        ImGui.SameLine();
        ImGui.BeginDisabled(scope.Length == 0 || !Marketable(itemId));
        if (ImGui.Button("Load")) FetchItem(itemId, scope);
        ImGui.EndDisabled();
        UiKit.Inline(125);
        ImGui.BeginDisabled(!HasItem(itemId));
        if (ImGui.Button("Open on Ultros")) OpenWebsite(itemId, scope);
        ImGui.EndDisabled();

        if (matches.Length > 0)
        {
            var listHeight = Math.Min(matches.Length, 6) * 28 * UiKit.Scale + 10 * UiKit.Scale;
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
                            if (ImGui.Selectable(label, itemId == match.Id))
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
            UiKit.Empty("Search for an item to inspect prices", "Type an item name above or right-click an item in game to load listings and recent sales.");
            DrawIdFallback();
            return;
        }

        // Selected Item Banner + inline Add to active list
        ImGui.Spacing();
        ImGui.Separator();
        UiKit.ItemIcon(ItemIcon, market.ItemId, qualityFilter == 1, 28f);
        ImGui.SetWindowFontScale(1.2f);
        ImGui.TextUnformatted(ItemName(market.ItemId));
        ImGui.SetWindowFontScale(1f);
        var fetchedLabel = market.FetchedAt is { } at
            ? $"Updated {TimestampFormat.FormatRelativeOrLocal(at)}"
            : market.Status;
        UiKit.Inline(220);
        UiKit.Muted($"{market.Scope} · {fetchedLabel}");

        // KPI Summary Strip
        var summary = MarketStats.Summarize(market.Listings, view.Sales);
        UiKit.KpiRow(
            new KpiCard("Cheapest NQ", summary.CheapestNq is { } nq ? $"{nq:N0} gil" : "—", $"{summary.NqCount} listing{(summary.NqCount == 1 ? "" : "s")}"),
            new KpiCard("Cheapest HQ", summary.CheapestHq is { } hq ? $"{hq:N0} gil" : "—", $"{summary.HqCount} listing{(summary.HqCount == 1 ? "" : "s")}", UiKit.HqGold),
            new KpiCard("Recent sale median", summary.MedianSalePrice is { } med ? $"{med:N0} gil" : "—", summary.SaleCount > 0 ? $"Avg {summary.AverageSalePrice:N0} · {summary.SaleCount} sales" : "No recent sales", UiKit.Accent));

        // Filter chips + Quick Add to active list on one compact row
        for (var i = 0; i < FilterLabels.Length; i++)
        {
            if (i > 0) ImGui.SameLine();
            var active = qualityFilter == i;
            if (active) ImGui.PushStyleColor(ImGuiCol.Button, UiKit.Accent);
            try { if (ImGui.Button($"{FilterLabels[i]}##mf-{i}")) qualityFilter = i; }
            finally { if (active) ImGui.PopStyleColor(); }
        }
        var listName = view.Cart?.Name ?? "active list";
        if (listName.Length > 20) listName = listName[..20] + "…";
        UiKit.Inline(310);
        ImGui.SetNextItemWidth(68 * UiKit.Scale);
        ImGui.Combo("##add-quality", ref addQuality, Qualities, Qualities.Length);
        ImGui.SameLine();
        ImGui.SetNextItemWidth(62 * UiKit.Scale);
        if (ImGui.InputInt("##add-qty", ref addQuantity, 0, 0)) addQuantity = Math.Clamp(addQuantity, 1, 1_000_000);
        ImGui.SameLine();
        ImGui.BeginDisabled(!HasItem(market.ItemId));
        if (ImGui.Button($"Add to {listName}##market-add"))
            AddToList(market.ItemId, WireQualities[addQuality], Math.Clamp(addQuantity, 1, 1_000_000));
        ImGui.EndDisabled();

        var filtered = market.Listings
            .Where(row => qualityFilter == 0 || (qualityFilter == 1 ? row.Hq : !row.Hq))
            .ToArray();

        if (filtered.Length == 0)
        {
            UiKit.Empty("No matching listings", market.Listings.Length == 0
                ? "No cached listings were found for this item and scope."
                : "No listings match the current quality filter.");
        }
        else
        {
            var sorted = SortListings(filtered);
            var wide = ImGui.GetContentRegionAvail().X >= 680 * UiKit.Scale;
            var columns = wide ? 6 : 5;
            const ImGuiTableFlags flags = ImGuiTableFlags.RowBg | ImGuiTableFlags.BordersInnerH | ImGuiTableFlags.Sortable | ImGuiTableFlags.SizingStretchProp;
            if (ImGui.BeginTable("market-listings", columns, flags))
            {
                try
                {
                    ImGui.TableSetupColumn("World", ImGuiTableColumnFlags.WidthFixed, 108 * UiKit.Scale);
                    ImGui.TableSetupColumn("Retainer", ImGuiTableColumnFlags.WidthStretch, 1f);
                    ImGui.TableSetupColumn("Qual", ImGuiTableColumnFlags.WidthFixed, 44 * UiKit.Scale);
                    ImGui.TableSetupColumn("Qty", ImGuiTableColumnFlags.WidthFixed, 42 * UiKit.Scale);
                    ImGui.TableSetupColumn("Unit price", ImGuiTableColumnFlags.WidthFixed | ImGuiTableColumnFlags.DefaultSort, 94 * UiKit.Scale);
                    if (wide) ImGui.TableSetupColumn("Total", ImGuiTableColumnFlags.WidthFixed, 98 * UiKit.Scale);
                    ImGui.TableHeadersRow();

                    var sortSpecs = ImGui.TableGetSortSpecs();
                    if (!sortSpecs.IsNull && sortSpecs.SpecsDirty && sortSpecs.SpecsCount > 0)
                    {
                        sortColumn = sortSpecs.Specs.ColumnIndex;
                        sortAscending = sortSpecs.Specs.SortDirection != ImGuiSortDirection.Descending;
                        sortSpecs.SpecsDirty = false;
                        sorted = SortListings(filtered);
                    }

                    foreach (var row in sorted)
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
                        ImGui.TextUnformatted($"{row.UnitPrice:N0}");
                        if (wide)
                        {
                            ImGui.TableNextColumn();
                            UiKit.Muted($"{row.UnitPrice * row.Quantity:N0}");
                        }
                    }
                }
                finally { ImGui.EndTable(); }
            }
        }

        // Recent Sales
        ImGui.Spacing();
        if (ImGui.CollapsingHeader($"Recent sales ({view.Sales.Length})"))
        {
            if (view.Sales.Length == 0)
            {
                UiKit.Hint("No recent sales recorded for this item and scope.");
            }
            else if (ImGui.BeginTable("market-sales", 5, ImGuiTableFlags.RowBg | ImGuiTableFlags.BordersInnerH | ImGuiTableFlags.SizingStretchProp))
            {
                try
                {
                    ImGui.TableSetupColumn("When", ImGuiTableColumnFlags.WidthStretch, 1f);
                    ImGui.TableSetupColumn("World", ImGuiTableColumnFlags.WidthFixed, 108 * UiKit.Scale);
                    ImGui.TableSetupColumn("Qual", ImGuiTableColumnFlags.WidthFixed, 44 * UiKit.Scale);
                    ImGui.TableSetupColumn("Qty", ImGuiTableColumnFlags.WidthFixed, 42 * UiKit.Scale);
                    ImGui.TableSetupColumn("Unit price", ImGuiTableColumnFlags.WidthFixed, 100 * UiKit.Scale);
                    ImGui.TableHeadersRow();
                    foreach (var sale in view.Sales)
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
                        ImGui.TextUnformatted($"{sale.UnitPrice:N0} gil");
                    }
                }
                finally { ImGui.EndTable(); }
            }
        }

        DrawIdFallback();
    }

    private void DrawIdFallback()
    {
        if (ImGui.CollapsingHeader("Lookup by item ID"))
        {
            ImGui.SetNextItemWidth(120 * UiKit.Scale);
            ImGui.InputInt("Item ID##market-id", ref itemId);
            ImGui.SameLine();
            ImGui.BeginDisabled(scope.Length == 0 || !Marketable(itemId));
            if (ImGui.Button("Load ID##market-load-id")) FetchItem(itemId, scope);
            ImGui.EndDisabled();
        }
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
