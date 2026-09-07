# Market data everywhere — roadmap

**Date:** 2026-09-07
**Status:** draft for Aaron's review; the workstream briefs are written to be handed to parallel agents as-is
**Base:** `main` at `fee1e4fc` (#1324). Everything below was verified against that tree, not against issue titles.
**Issues audited:** #1325, #1326, #1327, #1328, #1329, #1330, #1331 (open, all from the #1313 brainstorm); #1178, #1133, #1132 (open, adjacent); #1278, #1202, #1233, #1180, #1296 (closed, still informative).

## 0. The vision in five lines

Every page that shows bulk pricing data is an analyzer, whether or not its name says so. Each one should accept the same **market inputs** — a time window, a price basis, the page's market scope — and offer the same **market outputs**: one vocabulary of sale-history and listing-history columns, chosen from the same Columns picker, sorted by the same header click, persisted in the same URL keys, so a view built on the Flip Finder transfers to Trends or Currency Exchange without relearning anything. The `listing_events` / `floor_changes` tables that #1320 started recording become a second column family in that same vocabulary, so a metric added once appears on every analyzer at once.

## 1. Where things stand on `main`

### 1.1 The kit (what is already shared)

`ultros-frontend/ultros-app/src/analyzer_kit/` — `market.rs` (970 lines) and `stat_columns.rs` (235 lines) are the parts this roadmap builds on.

- **`MarketGrid`** (`market.rs:577`) wraps `QueryGrid`. It appends every shared `market-*` column after the page's own (`all_columns` memo, `market.rs:643`), skipping any id the page already declared, and renders shared headers as a bare label (`header=` closure, `market.rs:776`).
- **The window × statistic matrix** (`stat_columns.rs`, from #1313): `Window {D1, D7, D30, D90}` × `StatKind {Min, Median, Average, SalesPerDay, Cadence, Units, Sales, Vwap, GilVolume}` = `STAT_COLUMNS: [StatColumn; 36]` of literal ids (`market-sale-median-7`, `market-gil-90`, …). `MarketData` (`market.rs:49`) holds one `stats` slot and one `wanted` flag per window; the 7d body is always fetched, the others only when `window_wanted(needs, w)` (the effect at `market.rs:676`). `market_picker_options()` groups the 36 under "Sale history (Nd)" headings.
- **Price basis**: `MarketPriceControls` (`market.rs:195`) is one `<select>` over `PriceSignal {ListingMin, SaleMin, SaleMedian, SaleAvg}` (`formula.rs:22`), URL tokens `listing-min | sale-min | sale-median | sale-avg` under `?revenue=` / `?cost-basis=`. **It is 7d-only**: the option labels are the `market_sale_*_7` keys and every consumer resolves against `market.stats7()` (`analyzer.rs:1918`, and the pricing pass of each of the other five routes).
- **Sorting a shared column** works (`?sort=grid:<id>` + `?dir=`, `query_grid.rs:50-60`) but is reachable only from the column context menu's `MetricSortControls` (`virtual_grid/filter.rs:126-141`). Since #1324 that menu has no button — it is right-click or a 500 ms press-and-hold — so a header click is now the *only* discoverable sort gesture, and shared columns do not respond to it. Native columns do (`SortableHeaderCell`).
- **Columns picker**: only the Flip Finder passes `columns=` to `ControlBar` (`analyzer.rs:1713-1717`, extending its own options with `market_picker_options()`), with the helpers `toggle_shared_col` (`analyzer.rs:303`) and `shared_cols_in` (`analyzer.rs:320`). The other five consumers pass nothing, so on those pages a shared column is reachable only by the header menu's "Insert column…" search.
- **Saved views / presets**: every analyzer has `GridSavedViews` with starter presets since #1314; the view is the whole query string minus `lang` and `world`, so any new URL key is persisted for free.

### 1.2 Consumers of `MarketGrid` today

| Route | Toolbar Columns picker | `MarketPriceControls` | Window transport |
|---|---|---|---|
| `analyzer.rs` (Flip Finder) | yes | revenue | path `/flip-finder/:world` |
| `vendor_resale.rs` | no | revenue | path |
| `venture_analyzer.rs` | no | revenue | `?world=` |
| `leve_analyzer.rs` | no | cost + revenue | `?world=` |
| `fc_crafting_analyzer.rs` | no | cost + revenue | path (picker wired to nothing, per #1314) |
| `scrip_sources.rs` | no | cost | `?world=` |

### 1.3 Pages with bulk pricing data outside the kit

Measured on `main`, one row per page. "Distance" is to the target in §2, not to a grid swap.

| Page | Grid | Market data it fetches | Row identity | Has ControlBar / picker / `?cols=` | Distance |
|---|---|---|---|---|---|
| **Trends** `trends.rs` (807) | hand-rolled div table, ≤500 rows | `/api/v1/trends/{world}?window=7\|30\|90` only — no `sale_stats` | `(item_id, hq, world_id)` — exactly a `MarketSubject` | ControlBar + 4 filter chips; **no picker, no `?cols=`**; own `?sort=` tokens; **already uses `?window=` as a view mode, default 30** | **closest** |
| **Currency Exchange** `currency_exchange.rs` (1577) | real `<table>` on `data_table::Column` | `recentSales` + `cheapest`; price = min(cheapest−1, last sale); hours-between-sales computed client-side | trade tuple; the *received* item at NQ is the subject | ControlBar + picker + own `?cols=` vocabulary + 8 range chips | medium |
| **Recipe Analyzer** `recipe_analyzer.rs` (10086) | `AnalyzerGrid` | `cheapest`×N, `sale_stats` 7d + a bespoke lazy 30d store, sparklines | `RecipeId` → `(item_result, stat_hq)` | full: grouped picker, 23 `?cols=` tokens, two-sided basis with scopes, saved views | **ahead of the kit** in places (see §3.3) |
| **Item Explorer** `item_explorer.rs` (2493) + 4 modules | `DataTableGrid`, paginated `?page=` | `cheapest` only, via `CheapestPrices` | one item per row, NQ and HQ are *columns* | since #1316: ControlBar, picker, `?cols=`, 8 filters, every column sortable | large: row model and pagination are load-bearing (SEO pages) |
| **Retainer listings / undercuts** `retainers.rs` | real `<table>` per retainer, live via `retainer_live.rs` | retainer endpoints only; undercut = `cheapest − 1` from the payload | `RetainerRow {hq, item_id, price_per_unit, quantity}` — a `MarketSubject` with a listing price | none | small–medium, and the highest-value new consumer ("should I reprice?") |
| **List View** `list_view.rs` | real `<table>`, editable rows | full `ActiveListing`s per item | list-item id (an item can appear twice) | bespoke `ListFilterRow` | large; Lists 2.0 (Loro) is in flight on the same file — **do not schedule** |
| Job set detail, Item Compare, Home cards | cards / tiny tables | `cheapest`, `best_deals`, `movers` | — | none | not analyzers; out of scope by design |

### 1.4 Backend

- `GET /api/v1/sale_stats/{world|dc|region}?window=1|7|30|90` (`ultros/src/web/api/sale_stats.rs`) reads the `sale_stats_window` rollup (t-digest medians merge across worlds), NQ/HQ as separate rows, `gil_volume` on the wire since #1313. `SaleStatsCache` (`ultros/src/web/sale_stats_cache.rs`): 512 keys, 64 MiB, 5 min fresh / 30 min stale, 2 concurrent loads, 12 s timeout, single-flight. Payloads on the wire: 7d world ≈249 KB, DC ≈481 KB, region ≈578 KB, 30d world ≈438 KB.
- Rollup cadences (`ultros-clickhouse/src/rollups.rs:590-599`): 1d every 15 min, 7d hourly, 30d/90d every 6 h, `sales_hourly` every 15 min (trailing 30 h).
- **`listing_events` and `floor_changes` (#1320) are write-only.** `queries.rs` has zero references to either table; there is no rollup, no query function, no endpoint. The seed makes the alive set complete from deploy. Volume on prod is unmeasured (owed: watch `ultros_clickhouse_writer_written_rows_total{table="listing_events"}` for a day).
- No rate limiting anywhere; back-pressure is structural (cache + semaphore). A new bulk endpoint must copy that contract.

### 1.5 Issue audit

| Issue | State | What it asks | Where it lands below |
|---|---|---|---|
| #1325 | open | Sale median (7d) default-on in the Flip Finder | WS-C |
| #1326 | open | Shared columns placed beside Sale estimate with tuned widths | WS-C |
| #1327 | open | Header-click sort for shared columns, all consumers | WS-B |
| #1328 | open | Window selector instead of 36 per-window columns | WS-A (the keystone) |
| #1329 | open | Revenue / cost basis chooses the window | WS-A (basis follows the page window) |
| #1330 | open | Columns picker for the other five consumers | WS-D |
| #1331 | open | Recipe Analyzer adopts the matrix + gil | WS-E |
| #1178 | open | Stale listings deflate costs | Not a UI item. The upstream fix is deployed; WS-J's alive-listing age is the honest signal for it, and Phase J (listing age on the cheapest listing) stays a separate decision (§7) |
| #1133 | open umbrella | All tools on the flip-finder kit | This roadmap is the market-data half of finishing it; the two remaining #1132 design calls are unrelated |
| #1278 | closed | Phases G–L: H ports done, **J listing age**, **K saved views** (done by #1314), **L multi-window body** | J → §7 decision; L → superseded by the per-window slots (#1313) plus WS-I's second endpoint |
| #1180 / #1296 | closed | Sort every column, both directions, consistently | #1316 did it for the explorer; WS-B finishes it for shared columns |

Nothing open asks for Trends, Currency Exchange, or Retainer listings to join — those are the new items this roadmap adds (WS-F, WS-G, WS-M) and should be filed (§8).

## 2. The target: what every analyzer gets

**Market inputs** (the same three on every page, same URL keys):

| Input | URL key | Today | Target |
|---|---|---|---|
| Window | `?window=1\|7\|30\|90` | Trends only (7/30/90, plain `query_signal`, "a view mode, not a filter") | Every consumer. Default 7 (Trends keeps 30). Persisted in views. |
| Basis | `?revenue=`, `?cost-basis=` | six consumers, 7d only | Same tokens; the statistic resolves against the selected window |
| Scope | the page's world picker (`MarketData.scope`) | per page | Unchanged by this roadmap. (A DC/region *sell* scope is the recipe's Phase F and stays there.) |

**Market outputs**: one id vocabulary, chosen from a Columns picker present on every page, sortable from the header, with a sub-label that names window and place ("30d · Gilgamesh", the recipe's `window_and_place` convention, lifted into the kit).

| Family | Ids | Source |
|---|---|---|
| Sale history, follows the window | `market-sale-median`, `market-sale-min`, `market-sale-avg`, `market-sales-per-day`, `market-cadence`, `market-units`, `market-sales`, `market-vwap`, `market-gil` (9, **new**) | `sale_stats?window=<page window>` |
| Sale history, pinned | the existing 36 `market-…-{1,7,30,90}` (a bookmark contract, unchanged) | `sale_stats?window=N` |
| Listing history, follows the window / pinned | `market-floor-min`, `market-floor-max`, `market-listings-added`, `market-listings-removed`, `market-time-to-sell`, `market-alive`, `market-alive-units`, `market-sellers`, `market-days-of-stock` (+ `-N` pinned) (**new**, WS-J) | `listing_stats?window=N` (**new**, WS-I) |
| Sparklines | `market-trend-7`, `market-drift-7` (exist), `market-floor-30` (**new**, WS-J) | `sparklines`, `floor_series` (new) |
| Identity / text | `market-subject`, `-scope`, `-quality`, `-world`, `-datacenter`, `-listing`, `-last-sold`, `-confidence`, `-trend-world` (exist) | — |

**Naming.** The sidebar's "Tools" section lists nine pages; four carry "Analyzer" in the name, five do not, and the ones that do not (Flip Finder, Trends, Currency Exchange, Vendor Resale, Scrip Sources) are analyzers by the definition above. Decision for Aaron in §7 — it is copy in seven locales, no routes change.

## 3. Design decisions

### 3.1 D1 — A page-level window with follow-window ids (closes #1328, #1329)

**Chosen:** add `?window=` as a page-level *view mode* (as Trends already does) and nine un-suffixed **follow-window** ids. `market-sale-median` means "the median for the page's window"; `market-sale-median-30` keeps meaning 30d forever. The picker lists the nine follow entries first under "Sale history"; the 36 pinned entries stay listed under their "(Nd)" headings so a power user can compare windows side by side. The basis picker's options read "Sale median (30d)" when the window is 30 and resolve against `market.stats(window)`. Profit maths that reads `sales_per_day` from the stats body follows the window too; the Flip Finder's own velocity comes from `resale_quality`, not `sale_stats`, and is untouched.

**Why this shape.** Column ids in `?cols=` and saved views are a contract (#1328 says so explicitly), so the selector cannot change what a suffixed id means. The recipe analyzer already has un-suffixed ids (`rev-sale-median` = 7d today), so under this model its existing ids *become* follow-window ids with no rename and no behaviour change at the default (§3.3). And Trends' `?window=` already carries the same semantics with the same key.

**Rejected.** (a) *Rewrite the suffixes in `?cols=` when the selector changes* — no new ids, but a saved view holding `-7` ids and `window=30` is self-contradictory, and the basis picker still needs its own window. Fallback only if the reactive label plumbing proves hairy. (b) *Selector only collapses the picker groups* (issue shape 2) — does not deliver "one window applied to the whole set".

**Default.** 7d on every page except Trends (30d), via a `default_window` prop. A wider default silently changes every profit number (#1329's own caveat), so the default never moves in this roadmap.

### 3.2 D2 — Joining means adopting `MarketGrid`, with one exception

Trends, Currency Exchange and Retainer listings adopt `MarketGrid` outright: their rows already carry `(item, hq, world)`, their tables are hand-rolled or plain `<table>`s, and the kit gives them virtualisation, typed filters, layout, views and every shared column in one move. Their existing native columns become `GridColumn`s + `GridMetric`s; their existing `?sort=` tokens get aliased on read so old links keep working.

**The exception is the Item Explorer.** Its row is one item with NQ and HQ as columns, it paginates for SEO, and #1316 just rebuilt it on `DataTableGrid` with a picker and `?cols=`. Splitting rows by quality or dropping pagination would be a product change nobody asked for. Instead the kit exposes its column *provider* — ids, labels, `market_value`, and the `MarketData` fetch gate — so `DataTableGrid` can render the same columns with the same ids, quality chosen per column (`market-sale-median` reads the row's cheapest quality; a `-hq` suffix is not needed because the explorer's `?cols=` is page-local). That is WS-H, and it is the only place a second rendering path is accepted.

List View is not scheduled: Lists 2.0 is rewriting that file.

### 3.3 D3 — The Recipe Analyzer converges, it does not "adopt"

#1331 is written as "the recipe adopts the matrix". The survey says the opposite is half true: the recipe already has the window-and-place sub-label (`window_and_place`, `recipe_analyzer.rs:1793`), a sort that degrades while a lazy body loads (`effective_sort_mode`, `:2041`), a generation-guarded lazy fetch (the effect that follows `stats_30_source`), and a two-sided basis with scopes. What it lacks is exactly what #1331 lists: 1d/90d, gil, and it duplicates the 30d gate.

So WS-E has two halves. **Into the kit** (part of WS-A): the sub-label convention and the "demote a sort whose body has not landed" rule become kit behaviour for every consumer. **Into the recipe** (WS-E proper): its `stats_30` store is replaced by per-window slots gated by the `BodyRole::*Stats(u16)` variants `needed.rs` already parameterises; `rev-sale-*` / `cost-sale-*` read `StatKind` at the page window; `volume-30d` / `vwap-30d` stay pinned ids; gil gets `rev-gil` / `cost-gil`. Column ids do not change.

### 3.4 D4 — Listing history is a second stat family behind one new endpoint

`listing_events` + `floor_changes` + `sales` can answer, per `(item, hq)` and window, without any Postgres join:

| Metric | Definition | Caveat to print in the tooltip |
|---|---|---|
| Floor min / max | over `floor_changes` in the window; `price_per_unit = 0` rows excluded from min | "as Ultros observed it" |
| Listings added / removed | counts of `kind` in `listing_events`, `source != snapshot` | a reprice can be `removed`+`added` on one `listing_id` |
| Alive listings / units | listings whose last event is not `removed` | complete from the seed |
| Sellers | distinct `retainer_id` among alive listings | — |
| Oldest / median age | `now − min(reviewed_at)` per alive `listing_id` | `reviewed_at` = retainer's last touch, the honest "listed since" |
| Time to sell (median) | `removed` paired with a `sales` row on `(world, item, hq, price, qty)` within ±300 s, same-retainer re-add excluded, ambiguous pairs dropped — #1317's `SoldMatcher` rule, in SQL | "matched sales only" (≈60% of recent sales pair) |
| Days of stock | alive units ÷ (units sold per day from `sale_stats_window`) | — |

Served as `GET /api/v1/listing_stats/{world|dc|region}?window=N` from a `listing_stats_window` rollup on the same tickers as `sale_stats_window`, through the same cache class and headers. Empty is a legitimate answer here (no history yet), so it returns `200 []`, not the 503 `sale_stats` uses. A `listing_floor_hourly` rollup (forward-filled last floor per hour) feeds a `POST /api/v1/floor_series/{world}` batch endpoint shaped like `sparklines` for the floor sparkline column.

**Phase J** from #1278 (the *cheapest listing's* age via a Postgres `listed_at`) is not replaced by this: the analyzer's cheapest map has no `listing_id` to join on. It stays its own decision (§7). WS-J's "oldest alive age" answers "is this board stale" at the item level, which is most of what #1178 wanted from it.

### 3.5 D5 — One control cluster, rendered per page

A `MarketWindowControl` (select chip, same idiom as `MarketPriceControls`, `allowed` windows per page) plus `MarketPriceControls` plus the Columns picker form the cluster. `ControlBar`'s height lock (76 px) means these are chips and popovers, never a second row. Each route mounts the cluster in its existing `ControlBar` block; the kit provides the pieces and the URL binding, not the placement.

## 4. Workstreams

Every brief follows the same protocol (§4.0). Sizes: S ≤ 1 agent-day, M 1–3, L 3+.

### 4.0 Protocol for every workstream

- Branch off `origin/main` (stacked PRs get zero CI here). One PR per workstream unless the brief says otherwise. Never merge `main` in; `rebase --onto`.
- `./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo REAL_EXIT=$?` — log to the session scratchpad, never `/tmp/ci.log` (other worktrees write there). Exit 137 = clippy OOM, re-run with `-j 2`.
- Every user-facing string through `leptos-i18n`, keys in all seven locale files with real translations. Locale files are the universal merge conflict; keep additions grouped under one prefix per workstream so rebases are trivial.
- A player-visible change gets an entry in `ultros-changelog/changes/<date>-<slug>.json`.
- Tests: for every guard you add, run the mutation it exists to catch and watch it fail (the #1233 series lost ten guards to "compiles and passes").
- E2E: `integration/shared-analyzer-data.cjs` is the regression net for every `MarketGrid` consumer; extend it rather than adding a parallel script. `integration/flip-finder-mobile-bar.cjs` is the model for a height-lock check.
- Local boxes fire no analyzer enrichment on any branch; anything that needs `resale_quality` / `sparklines` pairing is verified on prod after deploy and the PR says so.
- PR body: what changed, test plan with the boxes actually ticked, what is owed on prod.

### WS-A — Foundation: the market window (keystone; blocks C, D, E, F, G, H, J)

**Closes** #1328, #1329. **Size** L. **Files** `analyzer_kit/stat_columns.rs`, `analyzer_kit/market.rs`, `analyzer_kit/formula.rs` (labels only), `components/control_bar.rs` (only if the picker needs a "follows window" hint), `routes/analyzer.rs` (lift helpers out; mount the control), the other five consumer routes (mount the control, switch `stats7()` → `stats(window)`), locales.

Deliverables:
1. `stat_columns.rs`: nine follow-window ids as a second `static` table (`market-sale-median`, …, `market-gil`), `MarketMetric::PageStat(StatKind)` alongside `Stat(StatKind, Window)`. `market_picker_options()` lists the nine first under "Sale history", then the 36 as today. Test: 45 unique ids, the 36 byte-identical to today, every follow id has no numeric suffix.
2. `use_market_window(default: Window, allowed: &'static [Window]) -> (Signal<Window>, SignalSetter)` reading `?window=` with plain `query_signal::<u16>` (a view mode, so `replace` semantics are wrong — copy Trends' choice at `trends.rs:459-463`); unsupported or disallowed values fall back to `default`. `MarketWindowControl` renders the allowed windows as a select chip.
3. `MarketGrid` takes `window: Signal<Window>`; `PageStat` resolves to `Stat(kind, window.get())` for value, label, `needs` → `market.want(window)`, and the sub-label ("30d · Gilgamesh") lifted from the recipe's `window_and_place` into `metric_label` for every stat column, pinned or follow. A header label must change when `?window=` changes without remounting the grid (the `all_columns` memo reads the window; column ids are stable so `?l=` layouts survive).
4. Sort degradation from the recipe (`effective_sort_mode`) becomes kit behaviour: a `?sort=grid:<stat id>` whose window body has not landed keeps the header state but orders by the page default until rows arrive.
5. `MarketPriceControls` labels read `stat_label(kind, window)`; every consumer's pricing pass resolves against `market.stats(window.get())`. `PriceSignal` tokens unchanged. Test per consumer: `?revenue=sale-median&window=30` prices from the 30d index; bare page byte-identical in numbers to today.
6. Lift `toggle_shared_col` / `shared_cols_in` / the `picker_visible` memo out of `routes/analyzer.rs` into `analyzer_kit` as functions parameterised by the page's anchor column, so WS-D does not copy them five times.
7. Mount `MarketWindowControl` beside `MarketPriceControls` in all six consumers (one line each).

Acceptance on prod: `/flip-finder/Gilgamesh?window=30&cols=market-sale-median` renders a "Sale median (30d)" header with numbers and fires exactly `window=7` and `window=30`; switching the chip to 90 changes the header and fires `window=90` once; `/flip-finder/Gilgamesh` bare fires only `window=7` and matches today's numbers; a saved view round-trips `window=`.

### WS-B — Header-click sort for shared columns

**Closes** #1327 (and finishes #1180 for shared columns). **Size** S–M. **Files** `analyzer_kit/market.rs` (header closure), `components/virtual_grid/filter.rs` (extract the `?sort=grid:` writer from `MetricSortControls` into a helper), possibly `components/sort_header.rs`.

The `header=` closure renders every non-`partial()` shared column through the same sortable header cell native columns use. Click writes `?sort=grid:<id>`; a second click toggles `?dir=`; a native `?sort=` active at the time is replaced (they are mutually exclusive in the URL). `Trend7` / `Drift7` (and later `market-floor-30`) stay un-clickable with the existing "visible-window enrichment cannot rank" reason in the title. State is read from the URL, not from page sort signals, because the header closure receives only the id. Since #1324 the context menu is the only other sort path, so this also needs a touch check.

Runs in parallel with WS-A (different regions of `market.rs`); whichever lands second rebases. Tests: click → `sort=grid:market-sale-median-7&dir=desc`; second click → `asc`; partial ids render no button; `aria-sort` matches. E2E: one click assertion in `shared-analyzer-data.cjs`.

### WS-C — Flip Finder: default-on median beside Sale estimate

**Closes** #1325, #1326. **Size** S. **After** WS-A. **Files** `routes/analyzer.rs` (`grid_columns` memo at `:1607`, `DEFAULT_VISIBLE_COLS` `:257`), `global_state/query_defaults.rs` (`seed_flip_finder_default_view`), tests.

Pre-declare `market-sale-median`, `market-sale-avg`, `market-sale-min`, `market-units`, `market-gil` (the follow ids, so they track the window) in `grid_columns` immediately after `sale_estimate`, with fallback widths measured from a settled English Gilgamesh view. `MarketGrid` skips ids the page declared, so rendering and values stay shared. Seed `market-sale-median` visible when `?cols=` is absent (Aaron's call, §7 #1). Update `default_columns_are_everything_but_tax_and_volume` deliberately; keep the `sale_estimate` anchor in `toggle_shared_col`'s lifted form; keep the column id the E2E probe scrolls to.

### WS-D — Columns picker on the other five consumers

**Closes** #1330. **Size** M (five near-identical edits). **After** WS-A. **Files** `routes/{vendor_resale,venture_analyzer,leve_analyzer,fc_crafting_analyzer,scrip_sources}.rs`, locales only if a page has no label for one of its own optional columns.

Per page: `columns=` (the page's own optional columns + `market_picker_options()`), `visible_columns=`, `on_toggle_column=`, `on_reset_columns=`, using the lifted helpers with the page's own anchor column. `QueryGrid` already reads `?cols=` over declared defs and writes it on header-menu toggles, so the picker and the menu stay in sync. One PR is fine — the diff is mechanical and shares one helper — but split into two if review size matters. Per page, one test that a toggle round-trips `?cols=` and one height-lock check modelled on `flip-finder-mobile-bar.cjs`.

### WS-E — Recipe Analyzer converges on the matrix

**Closes** #1331. **Size** L, and it wants its own spec + plan cycle before code (a 10k-line file with source-needle guards; #1233's history says plan reviews caught blockers three rounds deep). **After** WS-A. **Files** `routes/recipe_analyzer.rs`, `analyzer_kit/needed.rs`, `analyzer_kit/grid.rs`, `analyzer_kit/columns.rs`, locales.

Scope per §3.3: replace the `stats_30` trio (`stats_30_wanted` `:1726` / `stats_30_key` `:1735` / the generation-guarded effect that follows `stats_30_source`) with per-window slots gated by `needed_bodies` (`BodyRole::SellWorldStats(u16)` etc.); `rev-sale-*` / `cost-sale-*` read `StatKind` at the page window (ids unchanged, so 7d default is byte-identical); add `rev-gil` / `cost-gil`; `volume-30d` / `vwap-30d` stay pinned; `window_and_place` reads the window from the column; `RecipePriceControls` gains the window chip; `OPTIONAL_COLUMN_ORDER` appends only (its length test moves from 23 deliberately). `effective_sort_mode` per window. Buy-scope × sell-world × sell-scope bodies multiply by window — keep "fetch only when a column or signal wants it" and assert the body set in the existing `needed.rs` order tests.

### WS-F — Trends joins `MarketGrid`

**New issue** (§8). **Size** M. **After** WS-A. **Files** `routes/trends.rs`, locales, `integration/shared-analyzer-data.cjs`.

Rows are `TrendItem {item_id, hq, world_id, price, …}` → `MarketSubject::new(item_id, hq, world_id)` (`market.rs:256`) with `listing_price = Some(price)`. Native columns (24h sparkline, price, window VWAP, % change, sales/day, units, confidence) become `GridColumn`s + `GridMetric`s; the shared columns arrive free. Its `?window=` *is* the market window (`default_window = D30`, `allowed = [D7, D30, D90]` because `trends_v2` clamps anything else to 30). Alias the old `?sort=units|vwap|price|pct|spd` tokens to `grid:` ids on read. Keep the 500-row cap and the `show_suspicious` chip. Two sources for "VWAP": the page's comes from `trends_v2`'s deep scan, the shared one from `sale_stats`; label the native one "VWAP (deep scan)" or drop it in favour of the shared column — decide in the PR with a diff of both on one world.

### WS-G — Currency Exchange joins `MarketGrid`

**New issue** (§8). **Size** M. **After** WS-A. **Files** `routes/currency_exchange.rs`, locales.

Subject = the received item at NQ on the home world, `listing_price = price_per_item`. Native columns (item, qty received, profit, price per item, shops, cost item, hours between sales) become grid columns; its own `?cols=` vocabulary (`price_per_item`, `shops`, `cost`, `hours_between_sales`) is kept verbatim as the native ids so old links work, and `MarketGrid`'s id-skip rule means no collision. The real `<table>` with the `colspan` empty state becomes `QueryGrid`'s empty state. Hours-between-sales stays client-computed from `recentSales` (it is the page's velocity), but "Sales/day" from the shared family sits beside it so the two can be compared. The currency-quantity input stays in `ToolHeader`.

### WS-H — Item Explorer renders the shared columns inside `DataTableGrid`

**New issue** (§8). **Size** M–L. **After** WS-A (needs the provider surface). **Files** `analyzer_kit/market.rs` (expose `market_value` / labels / `use_market_data` as a provider without the grid), `routes/item_explorer.rs`, `item_explorer_filters.rs`, locales.

The explorer keeps `DataTableGrid`, pagination and its row model (§3.2). It gains the follow-window sale-history columns as optional, default-off entries in its existing picker; the quality read is the row's cheapest quality with the quality shown in the cell's title. `MarketData` is created only when a market column is in `?cols=` (the 7d world body is ≈249 KB, unacceptable on every category page by default). `column_availability` treats a market column as always fillable. Sorting by a market column is client-side over the current page only — say so in the sort menu, or exclude market columns from sort (recommended for v1).

### WS-I — Listing-history backend: rollups, queries, endpoint

**New issue** (§8). **Size** L. **No frontend dependency; start now.** **Files** `ultros-clickhouse/src/{schema,rollups,queries}.rs`, `ultros/src/web/api/listing_stats.rs` (new), `ultros/src/web/sale_stats_cache.rs` (generalise the loader or clone it as `ListingStatsCache`), `ultros-api-types/src/listing_stats.rs` (new), `ultros/src/web.rs` routes, smoke tests under `ultros-clickhouse/tests/`.

Deliverables, per §3.4:
1. `listing_stats_window` rollup keyed `(world_id, window_days, item_id, hq)`, refreshed on the same tickers as `sale_stats_window`, with the seven metrics. Time-to-sell pairs `listing_events.removed` with `sales` per #1317's rule; carry `matched_sales` and `ambiguous_sales` counts so the frontend can print coverage.
2. `listing_floor_hourly` (item, hq, world, hour → last floor, forward-filled; 0 = empty board) refreshed every 15 min trailing 30 h like `sales_hourly`, plus a daily fold for 30/90d sparklines.
3. `bulk_listing_stats(ch, world_ids, window) -> Vec<BulkListingStatsRow>` merging across worlds (sums add, floors take min, medians via t-digest states like `sale_stats`), and `floor_series_batch(ch, requests, hours)`.
4. `GET /api/v1/listing_stats/{world}?window=` and `POST /api/v1/floor_series/{world}` with `sale_stats`' cache contract (5 min fresh / 30 min stale / 2 loads / 12 s), the same `Cache-Control`, metric `ultros_listing_stats_cache_total{disposition}`. Empty = `200`.
5. Wire types serde-defaulted for forward compatibility, with the same old-shape test `ItemSaleStats` has.
6. Smoke test against a throwaway ClickHouse: insert a scripted `listing_events` + `sales` fixture, assert each metric including one ambiguous pair that must not match.

Owed before the endpoint is trusted: the #1320 volume watch on prod and a TTL revisit.

### WS-J — Listing-history columns in the kit

**New issue** (§8). **Size** M. **After** WS-A and WS-I, and after prod has ≥7 days of `listing_events`. **Files** `analyzer_kit/stat_columns.rs` (a `ListingKind` table, follow + pinned ids), `analyzer_kit/market.rs` (a second `MarketData` slot family fetched from `listing_stats` with the same `wanted` gate; `market-floor-30` sparkline via `floor_series` with the same visible-window enrichment as `market-trend-7`), `api.rs`, locales, `shared-analyzer-data.cjs`.

Lands on every `MarketGrid` consumer at once — that is the payoff of WS-A. Picker group "Listing history". Tooltips carry the caveats from §3.4 verbatim. Nothing default-on.

### WS-K — Prod verification owed (no code)

Run after each deploy, one agent with a browser: (1) #1313's checks — `sale_stats?window=1` returns `gil_volume > 0`; `/flip-finder/Gilgamesh?cols=profit_per_day,market-sale-median-30,market-gil-90` renders both headers with numbers and fires exactly `window=7,30,90`; bare page fires only `window=7`; four "Sale history (Nd)" groups in the picker. (2) #1320 — a day of `ultros_clickhouse_writer_written_rows_total{table="listing_events"}` and the dropped-rows counter; note the rate in `docs/ingest-observability.md`. (3) #1324 — the header press-and-hold on a real touch device. (4) After WS-A/B/C: the acceptance lines in their briefs.

### WS-L — Later: naming, and one URL contract for the world

**Size** S + M. Independent of everything above; schedule when the waves are done.

- **Naming** (Aaron's call, §7 #4): rename the sidebar section and drop the "Analyzer" suffixes, or leave it. Copy only, seven locales, no routes.
- **World transport** (from #1314's exploration; file as an issue): path `/tool/:world` on Flip Finder, Vendor Resale, Trends, FC Crafting versus `?world=` on Recipe, Venture, Leve, Scrip, plus FC Crafting's picker being wired to nothing. Unify on the path form with redirects from `?world=`; the four query-param pages each carry a copy-pasted `format!`-over-decoded-values effect that re-emits raw `&` and navigates with `replace: false`.

### WS-M — Candidate: Retainer listings as an analyzer

Not scheduled; the highest-value *new* consumer once WS-J exists. `RetainerRow` is already a `MarketSubject` with a listing price; the page needs a `ControlBar`, a scope signal from the retainer's world, and one grid per retainer (the accordions) or one flat grid with a retainer column. With WS-J it answers "is my price above the 7d median, how old is the floor, how long do these take to sell" on the page a seller actually looks at. File the issue now (§8), schedule after wave 2.

## 5. Dependency graph, waves, agent assignment

```
WS-I backend ──────────────────────────────┐
WS-B header sort ──┐                       │
WS-A foundation ───┼─► WS-C flip defaults  │
                   ├─► WS-D picker ×5      │
                   ├─► WS-E recipe (spec→plan→code)
                   ├─► WS-F trends         │
                   ├─► WS-G currency       │
                   ├─► WS-H explorer       │
                   └─► WS-J listing cols ◄─┘   (also needs ≥7d of prod data)
WS-K prod checks: after every deploy
WS-L naming / world transport: anytime after wave 1
WS-M retainers: after WS-J
```

| Wave | Runs in parallel | Agents | Gate to next wave |
|---|---|---|---|
| **0 (now)** | WS-A, WS-B, WS-I, WS-K(#1313/#1320 checks) | 4 | WS-A merged |
| **1** | WS-C, WS-D, WS-F, WS-G, WS-E *spec+plan only* | 5 | all merged; WS-E plan reviewed |
| **2** | WS-E code, WS-H, WS-J (if WS-I merged and data has aged) | 3 | — |
| **3** | WS-L, WS-M | 2 | — |

Thirteen PRs, roughly. Wave 0's four touch disjoint trees except WS-A/WS-B in `market.rs`; wave 1's five touch disjoint route files and only collide in the locale files.

## 6. Conflict map and merge order

| File / area | WS-A | WS-B | WS-C | WS-D | WS-E | WS-F | WS-G | WS-H | WS-I | WS-J |
|---|---|---|---|---|---|---|---|---|---|---|
| `analyzer_kit/market.rs` | ● | ● (header only) | | | | | | ● (provider) | | ● |
| `analyzer_kit/stat_columns.rs` | ● | | | | | | | | | ● |
| `routes/analyzer.rs` | ● (lift, mount) | | ● | | | | | | | |
| five consumer routes | ● (one line each) | | | ● | | | | | | |
| `routes/recipe_analyzer.rs` | | | | | ● | | | | | |
| `routes/trends.rs` | | | | | | ● | | | | |
| `routes/currency_exchange.rs` | | | | | | | ● | | | |
| `routes/item_explorer*.rs` | | | | | | | | ● | | |
| locales ×7 | ● | | ● | ○ | ● | ● | ● | ● | | ● |
| `ultros-clickhouse`, `ultros/src/web` | | | | | | | | | ● | |
| `integration/shared-analyzer-data.cjs` | ● | ● | ● | ● | | ● | | | | ● |

Merge order inside a wave: smallest diff first (locales are append-only conflicts; the big PR rebases once). WS-A before WS-B if both are ready, because B's header cell reads the window-aware label A introduces.

## 7. Decisions needed from Aaron

1. **Sale median default-on in the Flip Finder** (#1325) — recommended yes, `market-sale-median` (follow-window) visible on a first visit, placed after Sale estimate. Saved views and links keep their exact columns because the default is consulted only when `?cols=` is absent.
2. **Window model** — follow-window ids (§3.1, recommended) or rewrite suffixes on switch.
3. **Basis follows the window** (§3.1, recommended) rather than a separate `revenue-window` key.
4. **Naming** — rename "Tools" → "Analyzers" and drop the "Analyzer" suffix from the four that carry it (Recipes, Leves, Ventures, FC Crafting), or leave the names. Seven locales of copy, zero routes.
5. **Item Explorer** — shared columns inside `DataTableGrid` (§3.2, recommended) versus a full grid migration that would drop pagination.
6. **Listing metrics v1 scope** (§3.4) — include time-to-sell despite ≈60% pairing, labelled "matched sales only" (recommended), or hold it until a full month of data exists.
7. **Phase J** (cheapest listing age via Postgres `listed_at`, #1278) — defer until WS-J's alive-age has been on prod (recommended), or run its design pass now.
8. **World transport unification** — file it now as its own issue (recommended) even though it schedules last.
9. **Issue hygiene** — file the seven drafts in §8 and an umbrella that links #1325–#1331; close #1278's J/K/L rows into it.

## 8. Issue drafts (to file on go-ahead)

**Umbrella — "Market data everywhere: the same market inputs and columns on every analyzer"**
Links this document; checklist of WS-A…WS-M with #1325–#1331 mapped as in §1.5; the decisions in §7 with Aaron's answers recorded as they arrive.

**WS-F — "Trends joins MarketGrid"** — body from §4 WS-F. Labels: enhancement, website.

**WS-G — "Currency Exchange joins MarketGrid"** — body from §4 WS-G.

**WS-H — "Item Explorer: shared sale-history columns inside DataTableGrid"** — body from §4 WS-H, with the "row is one item, both qualities" constraint stated so nobody proposes a grid swap.

**WS-I — "listing_stats: rollups, queries and an endpoint over listing_events / floor_changes"** — body from §3.4 + §4 WS-I; references #1320 and the owed volume watch.

**WS-J — "Listing-history columns in the analyzer kit"** — body from §4 WS-J; blocked-by WS-A and WS-I.

**WS-L — "One URL contract for the world on every tool"** — the #1314 findings quoted, FC Crafting's dead picker first.

**WS-M — "Retainer listings as an analyzer (MarketGrid + listing history)"** — body from §4 WS-M.

## 9. Deliberately left out

DC/region *sell* scope on the six consumers (the recipe's Phase F, its own thread); List View (Lists 2.0 owns the file); Job set detail, Item Compare and the home cards (not analyzers; the top-opportunities card documents why ClickHouse-derived columns would be blank there); a server-side multi-window body (#1278 L — the per-window slots and cache already cover it); unifying tax rounding / ROI math; the item page's own basis selector.
