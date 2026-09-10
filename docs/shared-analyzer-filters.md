# Shared analyzer filters

`ultros-ui::components::virtual_grid::registry::FilterRegistry` is the common
contract for `ControlBar`, `QueryGrid`, and column menus. The app re-exports it
through `components::virtual_grid`; `analyzer_kit::filters::register_filters`
is the route helper.

Provide a registry once in the component owner containing both the toolbar and
grid, before constructing either view:

```rust
let filters = register_filters(
    vec![FilterAlias::new("profit", "profit", FilterOp::Gte)],
    Signal::derive(move || vec![toggle_control("filter-outliers", label())]),
);
```

The existing `MarketGrid`/`QueryGrid` props remain unchanged. `QueryGrid`
registers its resolved column definitions, metric kinds, and filtered row count.
`ControlBar` discovers that registry and owns the only metric chip row. Its old
`available_filters`, `on_add_filter`, `on_clear_all`, and `is_empty` props are
optional for registered hosts; omit page chip children. Use
`filters.row_count()` for the toolbar total. Unregistered consumers retain their
existing behavior until migrated.

## Definitions and evaluation

- `GridMetric<T>` remains the typed row extractor. Add one for each meaningful
  column value, keeping its existing column ID. QueryGrid attaches its editor to
  both the column menu and toolbar registry. Hidden columns remain filterable.
- `duration_value` converts nonnegative durations to seconds without truncating
  fractional boundaries; absent or negative ages remain missing.
- `GridColumn::picker_group` supplies the same window-aware groups to the filter
  menu. Labels can follow the selected window; persisted IDs must not change.
- `ColumnFilter` registers a URL control whose semantics are owned by the tool.
  Tax, outlier handling, inventory use, job selection, and buy scope must be read
  before their relevant calculations. Do not replace them with a filter on an
  already calculated result. `options` supports fixed tokens; `choices` supports
  owned dynamic tokens and labels; `multiple` edits a comma-separated selection.
- Register additional controls in the registry's `controls` signal. Existing
  nonmetric column controls are collected automatically and deduplicated. These
  controls use the same editor in a header, toolbar menu, or chip.
- Some legacy selectors measure something different from the displayed metric:
  recent-buffer sale count and duration are not a seven-day rate; the confidence
  floor includes a derived fallback unlike the deep-scan confidence column.
  They remain explicit registered controls with their original predicate and
  coverage semantics rather than silently changing the meaning of a bookmark.
- `price_control` registers a window-aware calculation selector with a visible
  default chip. Its `clear_with_filters = false` policy preserves the selected
  price basis during Clear all. Clearing that individual override restores the
  default. `MarketWindowControl` continues to own the selected history window.
  The older `MarketPriceControls` component remains available to unconverted
  hosts and the window fixture.

## Bookmark compatibility

`FilterAlias` maps an old parameter to a metric ID/operator, with an optional
pure `convert` function for tokens or units. For example, Flip Finder's
`last-sold=1d` becomes a maximum age in seconds. Vendor Resale’s `next-sale`
uses the strict `lt` operator on `sale-time`, preserving its exclusive boundary.
Remove the old row predicate
when adding its equivalent alias: QueryGrid evaluates that filter once.

An explicit `gf` entry wins over all aliases for its column. Editing any
registered filter first canonicalizes every alias in one query replacement,
then performs the edit. Removing a filter cannot reveal an old alias on reload.
The landing defaults `min-sales`, `next-sale`, and `last-sold` clear to explicit
empty parameters so their defaults cannot be reseeded. Layout, sorting, world,
and window parameters are preserved unless they are the control being edited.
Edits use `replace: true` and `scroll: false`.

`SortAlias` does the same for a retired native `?sort=` token: the registry's
`register_sort_aliases` maps it to a metric column, so an old
`?sort=units&dir=asc` link still ranks the grid, lights that header, and
requests the window a hidden target needs. Nothing rewrites the token in place;
the first header click writes the canonical `grid:<id>` form. Trends registers
`units`, `vwap`, `price` (now the shared listing column), `pct`, and `spd`.

Existing `{ "op": "gte", "value": "100" }` metric payloads are unchanged.
Inclusive simultaneous bounds use
`{ "op": "between", "value": "100,200" }`. A legacy minimum and maximum on
one column resolve to that range. Contradictory old bounds still reject every
known value; the editor asks for an ordered range when applying a new edit.
New numeric filters open blank and do not write a threshold until Apply receives
a valid value.

## Data coverage

Keep `GridValue::Pending`, `Missing`, and `Unavailable` distinct. Shared queries
retain unresolved rows and preserve the coverage notice so lazy enrichment can
still request their subjects. A hidden active metric must participate in the
provider's required-data set; `MarketGrid` uses the registry's effective filters,
including aliases, when requesting windows. Preserve intrinsic `partial` flags:
filters may evaluate known rows without claiming complete data or a global sort.

## Consumers for issue #1351

The six MarketGrid tools (Flip Finder, Vendor Resale, Ventures, Leves, FC
Crafting, Scrip Sources), Trends, Currency Exchange, and the Recipe Analyzer's `AnalyzerGrid` host use
this registry. Recipe keeps its legacy `profit`, `roi` and `min-sales` keys as
aliases of its `profit`, `roi` and `daily-sales` metrics (`min-sales=0` still
means "no limit"), and registers its job, sub-craft, HQ, outlier, crystal,
on-hand, listing world/DC and pricing (`cost-basis`, `revenue`, `buy-scope`,
`sell-scope`) inputs as controls; Clear all preserves the four pricing inputs.
`AnalyzerGrid`'s `picker` prop forwards the Columns picker's headings to the
`+ Filter` menu, which keeps each group together.

Item Explorer also uses the shared grid and registry; all planned consumers have now adopted it.

T09 (Trends, #1344) is done: the page keeps `category` and `show_suspicious`
as registered controls, aliases `min_sales`/`min_price` onto its `sales` and
`market-listing` metrics, and registers its native cleaned-sample statistics
(`vwap`, `pct`, `sales-per-day`, `units`, `sales`, `confidence`) as their own
metrics. They are not the `sale_stats` follow-window columns: the deep-scan
drops noise-filtered sales and its confidence band comes from that scan, while
`sale_stats` counts every recorded sale and its confidence is a seven-day fact.
Both sets stay available from the Columns picker under separate headings.

T11 (Item Explorer, #1346) is done: category and job-set lists use the shared
grid and filter registry, preserving their existing query keys and saved
column choices. Market statistics load when a selected column, filter, or
sort needs them.

## Regression coverage

Grid-core tests cover range validation and unresolved values. UI registry tests
cover alias precedence, simultaneous and contradictory bounds, canonical clear
and reload, default sentinels, hidden definitions, and control deduplication.
`integration/shared-analyzer-data.cjs` exercises the real SSR/hydrated registry
fixture (`registry-test=1`), including menu/header/chip edits, blank input,
calculation inputs, hidden filters, clear/reload, and mobile wrapping, followed
by the existing seven-tool market probes and the Trends probe (old sort and
chip links, header sorting, the Columns picker, window changes, the
suspicious-sales control, request behavior and reload) when
`CHECK_ANALYZER_ROUTES=1`.
