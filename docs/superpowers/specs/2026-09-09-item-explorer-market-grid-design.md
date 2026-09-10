# Item Explorer on the shared market grid (issue #1346)

Move `/items/category/:category` and `/items/jobset/:jobset` from the paginated
`DataTableGrid` onto `MarketGrid`, the host every other analyzer uses. The
catalog keeps one row per item with NQ and HQ prices as columns, keeps every
sort and filter #1316 added, drops pagination, and gains the shared sale-history
columns without paying for them until a column, sort or filter asks.

Tracker: #1349. Depends on #1373 (shared `?window=`) and #1398 (filter harness),
both merged.

## What stays the same

- **Row model.** One row per item. Item icon, name, iLvl, Lv, NQ price, HQ
  price, vendor price, cheapest world, row actions.
- **URL contract.** `?sort=` keeps the eight native tokens (`ilvl`, `lv`,
  `price`, `hq`, `vendor`, `world`, `name`, `key`) and `?dir=`. `?cols=` keeps
  the five native ids (`ilvl`, `lv`, `hq`, `vendor`, `world`) and may now also
  carry shared `market-*` ids. `?world=` and `?show-non-market=` are untouched.
- **Legacy filters** `q`, `min-ilvl`, `max-ilvl`, `min-lv`, `max-price`,
  `vendor-only`, `hq-only`, `listed` keep filtering exactly as before. Numeric
  pairs resolve through `FilterAlias` into one inclusive `Between` bound.
- **Column availability** (#1296). Computed once from the whole unfiltered
  item set. A column nothing in the set can fill is absent from the grid and
  greyed in the Columns picker with the same reason text. World stays gated on
  a multi-world scope.
- **Hydration gate.** The server and the first client render never read the
  listings resource. Price-dependent sorts fall back to iLvl (or Name), price
  filters are inert, and the row set and order are identical on both sides.
  The gate flips from a client `Effect`, as today.
- **Sort fallback.** `?sort=` naming a column the set lacks falls back rather
  than sorting by a constant.
- **Meta.** Titles, descriptions, canonical hrefs, item sitemap and category
  sitemap entries are unchanged.

## What changes

### Grid host

`ItemList` renders `MarketGrid` instead of `DataTableGrid`. Rows are
`ExplorerRow` values built in one memo:

```
struct ExplorerRow {
    item_id: i32,
    item: &'static Item,
    /// Both `None` until the gate flips, then loaded per quality.
    nq: Option<i32>, hq: Option<i32>,
    /// The cheapest listing across qualities: (price, hq, world_id).
    cheapest: Option<(i32, bool, i32)>,
    vendor: Option<u32>,
    prices_loaded: bool,
}
```

The memo applies only the route-level controls that are not metric filters
(`hq-only`, and `show-non-market` upstream) and the native `?sort=`. Everything
else (`?gf=`, aliases, `?sort=grid:*`) is applied by `QueryGrid`.

Native `GridMetric`s (all numbers unless noted): `item` (text, the name),
`ilvl`, `lv`, `price` (NQ), `hq` (HQ), `vendor`, `world` (text), `key`,
`market-listing` (overrides the shared one). Price-backed metrics return
`GridValue::Pending` while `prices_loaded == false`, so a `grid:` sort is
"pending" and a price filter keeps every row on both sides of hydration. That
is the same contract `CheapestPrice::NotLoaded` gave the legacy filters.

Row height 40px, icon small. The compact iLvl/Lv line under the name goes
away: the virtual grid scrolls horizontally on narrow screens instead of hiding
columns by breakpoint.

### Filters

`register_filters` gets these aliases:

| Legacy key   | Column           | Op       | Parse   |
|--------------|------------------|----------|---------|
| `q`          | `item`           | Contains | trimmed |
| `min-ilvl`   | `ilvl`           | Gte      | integer |
| `max-ilvl`   | `ilvl`           | Lte      | integer |
| `min-lv`     | `lv`             | Gte      | integer |
| `max-price`  | `market-listing` | Lte      | integer |
| `vendor-only`| `vendor`         | Present  | `true`  |
| `listed`     | `market-listing` | Present  | `true`  |

`hq-only` is a `toggle_control` (route-level, pre-calculation) because it means
"the item *can* be HQ", which no column expresses. Boolean aliases convert only
`true` to a value; anything else clears.

`+ Filter` and the chip row come from the registry. The page's own chip
components, `ExplorerFilterSignals`, `use_explorer_filters`, `ADDABLE_FILTERS`
and `filter_requires_column` are deleted. `ExplorerFilters::matches` shrinks to
the `hq_only` case; its other predicates are now grid metrics.

### Columns picker

The `ControlBar` picker keeps listing the five native optional columns with
their availability reasons, then appends `market_picker_options(window)` as the
Flip Finder does. Toggling a shared id goes through `toggle_shared_col`.

### Market data on demand

`use_market_data` today wants the 7-day body at mount. A new
`use_market_data_on_demand(scope)` builds the same `MarketData` with no window
wanted. `MarketGrid` already wants windows from visible columns, `?cols=`,
`?gf=` (aliases included) and `?sort=grid:*`, so a restored view or saved
layout that needs statistics requests them, and a bare category page issues
zero `sale_stats` requests. The existing eager constructor and every other
consumer are untouched.

`MarketWindowControl` sits in the ControlBar actions slot beside the sort
select; default window 7.

### Market subject

Statistics describe **the cheapest listed quality in the pricing scope**:
`MarketSubject { item_id, hq: cheapest.hq, world_id: cheapest.world,
listing_price: cheapest.price }`. The shared `market-quality` column names that
quality; `market-world` / `market-datacenter` name the listing's location.

Missing listing (or prices not yet loaded): the subject is **NQ on world 0
with no listing price**. World 0 is what `MarketGrid` already treats as "no
listing", so location columns render "—" and no sparkline is requested for it.
This is a documented rule, pinned by a unit test, not an accident of `bool`
defaults.

### Pagination

`?page=` and `?per_page=` are no longer read. The route paths do not change,
so every old link resolves to the same category or job set, now unpaginated;
the query keys are simply ignored. `paginate` is dropped from the page.

SEO: the canonical href already points every variant at the id-keyed category
URL with no query, so `?page=N` duplicates consolidate rather than compete.
`robots.txt` keeps its existing `?sort=` and `?per_page=` disallows; `?page=`
stays crawlable so the canonical is seen. The server-rendered first viewport
shrinks from 50 rows to the virtual grid's initial range (roughly 18 rows at
40px), which the September 8 decision accepted. **Before shipping**, Aaron
should check Search Console for traffic to `?page=` URLs; the code side needs
nothing further.

## Testing

Rust:
- Aliases: each legacy key resolves to the expected column/op, min+max become
  one inclusive `Between`, boolean keys accept only `true`.
- Subject: cheapest quality/world/price when both qualities are listed; NQ,
  world 0, `None` when no listing or prices not loaded.
- Pending metrics before the gate: a price filter keeps every row and a
  `grid:price` sort reports pending.
- Column availability drops columns from the grid definition and disables them
  in the picker with a reason; sort fallback is unchanged.
- `use_market_data_on_demand` wants no window until asked.
- Existing explorer tests keep passing (round-trip sort tokens, locale keys,
  category resolution); the pagination test is removed with the code.

Browser (`integration/item-explorer-grid.cjs`, run from a local server):
- `/items/category/10`: hydrates without errors, `.virtual-grid` present, no
  `/api/v1/sale_stats/` request on load.
- `?cols=market-sale-median` issues exactly one `sale_stats` request for the
  window; changing the window issues one more.
- `?page=3&per_page=25` renders the full category.
- `?min-ilvl=600&max-ilvl=700` shows the canonical `gf` chip and the row count
  drops; `?sort=price` sorts by NQ price after hydration.
- Mobile viewport: the grid scrolls horizontally, the page does not.

## Out of scope

- Search Console review, robots changes.
- Trends/Currency Exchange migrations (#1344, #1412).
- Making any shared column default-on in the explorer.
