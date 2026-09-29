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
`filters.row_count()` for the toolbar total. `QueryGrid` renders no filter
strip of its own: the bar is a host's only filter surface, so a grid with
metric filters must be paired with a registered `ControlBar`. Every host,
including the `/__test/shared-analyzer-data` fixtures, is registered.

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
  nonmetric column controls are collected automatically and deduplicated. Row
  controls use the same editor in a header, toolbar menu, or chip. Calculation
  controls set `calculation = true` and are excluded from those surfaces.
- Some legacy selectors measure something different from the displayed metric:
  recent-buffer sale count and duration are not a seven-day rate; the confidence
  floor includes a derived fallback unlike the deep-scan confidence column.
  They remain explicit registered controls with their original predicate and
  coverage semantics rather than silently changing the meaning of a bookmark.
- `price_control` registers a window-aware calculation selector for the formula
  strip. Defaults do not become active row filters. Its `clear_with_filters = false` policy preserves the selected
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

## Consumers

All ten grid hosts use this registry: the six MarketGrid tools (Flip Finder,
Vendor Resale, Ventures, Leves, FC Crafting, Scrip Sources), Trends, Currency
Exchange, Item Explorer, and the Recipe Analyzer's `AnalyzerGrid` host. There
is one row-filter surface; calculation assumptions have their own formula strip.

- Recipe keeps its legacy `profit`, `roi` and `min-sales` keys as aliases of
  its `profit`, `roi` and `daily-sales` metrics (`min-sales=0` still means "no
  limit"), and registers its job, sub-craft, HQ, outlier, crystal, on-hand and
  pricing (`cost-basis`, `revenue`, `buy-scope`, `sell-scope`) inputs as
  controls. Listing world/DC keys are metric aliases. Its MarketGrid host
  shares the page window, and Clear all preserves that window and the four
  pricing inputs. `AnalyzerGrid`'s `picker` prop forwards the Columns picker's
  headings to the `+ Filter` menu, which keeps each group together.
- Trends keeps `category` and `show_suspicious` as registered controls,
  aliases `min_sales`/`min_price` onto its `sales` and `market-listing`
  metrics, and registers its native cleaned-sample statistics (`vwap`, `pct`,
  `sales-per-day`, `units`, `sales`, `confidence`) as their own metrics. They
  are not the `sale_stats` follow-window columns: the deep-scan drops
  noise-filtered sales and its confidence band comes from that scan, while
  `sale_stats` counts every recorded sale and its confidence is a seven-day
  fact. Both sets stay available from the Columns picker under separate
  headings.
- Item Explorer's category and job-set lists preserve their existing query
  keys and saved column choices. Market statistics load when a selected
  column, filter, or sort needs them.
- The chip row's empty state is the shared `no_active_filters` string unless a
  tool has a more specific hint ("No filters — showing every recipe").

## Regression coverage

Grid-core tests cover range validation and unresolved values. UI registry tests
cover alias precedence, simultaneous and contradictory bounds, canonical clear
and reload, default sentinels, hidden definitions, and control deduplication.
`integration/shared-analyzer-data.cjs` exercises the real SSR/hydrated registry
fixture, including menu/header/chip edits, blank input,
calculation inputs, hidden filters, clear/reload, and mobile wrapping, followed
by the existing seven-tool market probes and the Trends probe (old sort and
chip links, header sorting, the Columns picker, window changes, the
suspicious-sales control, request behavior and reload) when
`CHECK_ANALYZER_ROUTES=1`.

## Calculation presentation

`analyzer_kit::calculation::Calculation` is provided in the route owner, beside
its registry and before constructing either the strip or grid. Its inputs read
`FilterRegistry::controls()` directly: reading resolved grid columns while
building formula headings would create a reactive cycle. Setters are created
once, outside rendering closures. The strip and header Use shortcuts write the
same existing URL keys, using replacement navigation and preserving scroll.

`CalculationStrip` presents direct selects with arithmetic roles, followed by
Window. Recipe keeps its specialized `FormulaStrip` for the dual buy/sell scopes
and fallback indicators; the same four controls remain registered for URL
semantics but never render a second chip or filter editor. `calculation = true`
excludes an input from active predicates, the Filter menu, column filter editors,
and both global and per-column filter clearing. A direct select restores its
default by removing the override. Tax remains in the formula when row filters
are cleared, just like the price basis and window.

`MarketGrid` decorates native headers using each route's role mapping. Shared
listing and selected-window minimum/median/average headers may offer Use for the
market subject's compatible input. Fixed-window statistics, VWAP, volume and
other unrelated metrics never offer this shortcut. Leves target turn-in cost;
FC Crafting and Scrip Sources target ingredient pricing. These shortcuts change
the input methodology, not the arithmetic identity of the individual ingredient
statistic. Currency Exchange presents total gil = unit value × quantity received;
Trends and Item Explorer have no profit assumptions to show.
