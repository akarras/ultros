# Recipe Analyzer convergence on the shared window, columns and filters

Status: design for #1331 (tracker #1349, WS-E). Implemented in the same change.

## Problem

The Recipe Analyzer is the last analyzer with its own grid host, its own
market-data loader and its own filter chips. It renders `AnalyzerGrid` over
`QueryGrid` directly, fetches a bespoke client-only 30-day body through a
generation-guarded effect, labels every sale signal "(7d)" from fixed locale
keys, and keeps a hand-wired `+ Filter` menu and chip row. It has none of
#1328's page window, none of #1313's gil-traded statistic, and none of #1351's
shared filter registry.

## What converges, and what stays the recipe's own

The recipe keeps everything that is genuinely recipe-shaped: the two-sided
ledger with a buy scope and a sell scope, the sub-craft cost pass, the
`ToolColumnMeta` table with its `?cols=` / `?sort=` contract, the grouped
picker, the formula marks and "use" pills, and the SSR-priced rows. Everything
that duplicates the kit converges onto it:

| Surface | Before | After |
|---|---|---|
| Grid host | `AnalyzerGrid` → `QueryGrid` | `AnalyzerGrid` → `MarketGrid` → `QueryGrid` |
| Page window | none; labels hard-code 7d | `MarketWindow` (`?window=`, default 7) shared with every analyzer |
| Sale-based prices | 7d bodies | the selected window's bodies, both sides |
| Extra windows on the sell world | bespoke `stats_30` store + effect | the sell world's `MarketData` slots |
| Window comparisons | none | the 45 shared `market-*` columns, in the toolbar picker |
| Gil traded | none | `rev-gil`, `cost-gil` |
| Filters | page chips, `ADDABLE_FILTERS`, `Thresholds` | `FilterRegistry` aliases and controls |

Column ids do not change. Every serialized `?cols=`, `?sort=`, `?gf=` and every
filter key keeps its meaning; new ids are appended.

## D1 — One page window; which columns follow it

`?window=1|7|30|90`, default 7, via the kit's `MarketWindow`. The chip renders
in the price-controls block beside the formula strip, where the basis selects
already live.

**Follows the window** (a sale *signal*): the cost basis and revenue basis
when they are a sale statistic, the four `rev-*` and four `cost-*`
alternative columns, and the two new gil columns. Their labels read
"Sale median (30d)" and their sub-labels "30d median · Aether". Absent
`?window=` means 7d, so every existing URL prices and labels exactly as
before.

**Stays 7d** (the sell world's *market context*): sales/day, average price,
confidence, last sold, `volume`, `vwap`, tax, profit/day, trend, drift, the
Price "vs median" tell and the VWAP percentage. These are the columns whose
kinds are seven-day definitions (`SalesPerDay7`, `VolumeUnits7`, `Vwap7`) and
whose labels already say "(7d)"; #1329 forbids relabelling a velocity estimate
as a windowed statistic, and the kit keeps `market-last-sold` /
`market-confidence` at 7d for the same reason. **Stays 30d**: `volume-30d`,
`vwap-30d`, as the issue requires.

A user who wants the 30-day volume of the output item beside the 7-day one
turns on `market-units-30` from the shared picker; that is what the shared
matrix is for.

## D2 — Bodies per role and window

The recipe prices rows on the server, so the bodies a price depends on stay
`ArcResource`s on the Suspense gate. `needed_bodies` decides which bodies the
view needs; the page maps each `BodyRole` to a resource key. `RecipeNeeds`
gains `window` (the selected window in days, default 7), `rev_gil`,
`cost_gil`, `hop` and `scope_vs_home`:

| Role | When | Fetched by |
|---|---|---|
| `SellWorldStats(7)` | always (context columns, velocity) | `sell_history` resource, as today |
| `BuyScopeStats(w)` | a sale cost signal, a visible/sorted `cost-sale-*`, or `cost-gil` (not when the buy scope aliases the sell world) | `sale_stats` resource, keyed on `(scope, w)` |
| `SellWorldStats(w)`, w ≠ 7 | a sale signal reads the sell world's own market: revenue at the default sell scope (a sale revenue signal, a `rev-sale-*` column, `rev-gil`), an aliased buy side wanting sale statistics, Hop gain's home side under a sale cost signal, or Scope vs home's home side under a sale revenue signal at a wider scope | new `sell_window_stats` resource, keyed on `(world, w)` |
| `SellScopeStats(w)` | a wider sell scope, same triggers, unless the buy scope already holds that body | `sell_scope_bodies` resource, keyed on `(place, listings, stats, w)` |
| `SellWorldStats(30)` | `volume-30d` / `vwap-30d` visible or the sort target | the sell world's `MarketData` slot, client-only, never on the gate |

The alias rules (`buy_scope_is_sell_world`, `sell_scope_is_buy_scope`) dedupe
across roles as before, now per window: a body only stands in for another
body at the same window.

**The shared loader.** `use_market_data_with(scope, window, provided)` gives
the recipe one `MarketData` for the sell world, sharing the page's
`MarketWindow`. `provided` names the windows the page fetches itself on the
gate (`{7}` plus the selected window when a pricing body wants it); the loader
never requests those, and the table `supply()`s the resolved bodies into the
matching slots as it mounts, so a shared column or a pinned 30-day column
reads the page's body instead of fetching it again. Windows nobody supplies
are fetched by the loader on demand, deduplicated, scope-guarded and
generation-guarded exactly as on every other analyzer. The bespoke
`stats_30` store, its effect and its failure flag are deleted; `CellCtx`'s
30-day handles are mirrored from the slot.

## D3 — Gil traded

Gil traded is a market-size statistic (`ItemSaleStats::gil_volume`), not a
price. On the recipe it has two meanings, one per side of the ledger:

- **`rev-gil` — Gil traded (Nd), revenue side.** The output item's gil traded
  over the window at the revenue place, for the row's priced quality (the
  same exact NQ/HQ the Price and the 7-day context use). "—" when the place
  has no row for that quality; "unavailable" when the body failed.
- **`cost-gil` — Thinnest ingredient market (Nd), cost side.** An ingredient
  basket is not one item's statistic, so the column answers the sourcing
  question: over every ingredient line the cost pass bought on the market
  (after sub-craft, vendor, on-hand and shard handling), the *smallest* gil
  traded on the buy scope over the window. Winning subcraft market leaves
  are included; losing candidates are not. Under Require HQ, use the quality
  the cost lookup selected, including its existing NQ fallback when HQ is
  unavailable; use NQ + HQ otherwise. An ingredient with no row traded nothing and counts as
  0. "—" when no line was bought on the market or the buy-scope body is
  absent. A low number warns that one ingredient cannot be sourced
  repeatedly; the sub-craft cap does not apply (it is a lookup, not a run).

Both are Bulk columns (sortable, "—" last), appended to the column table under
the Revenue and Cost picker groups, and both need the same statistics bodies
their sale-signal siblings need.

## D4 — Filters through the shared registry (#1351 T08)

The page provides one `FilterRegistry` (at page level, so edits survive table
remounts):

- **Aliases** (old key → metric): `profit` → `profit` ≥, `roi` → `roi` ≥,
  `min-sales` → `daily-sales` ≥, `listing-world` → `listing-world` =,
  `listing-dc` → `listing-dc` =. `QueryGrid` evaluates them once; the page's
  `Thresholds` predicates go. Explicit `gf` wins; edits canonicalize;
  `min-sales` clears to an explicit empty value so the landing default cannot
  reseed it. The sales/day metric reports 0 for an output with no sales in the
  window so the seeded `min-sales=1` keeps excluding it.
- **Controls** (URL keys read before pricing): `job`, `cost-basis`, `revenue`,
  `buy-scope`, `sell-scope`, `subcrafts`, `require-hq`, `filter-outliers`,
  `shards-exclude`, `on-hand`. The basis and scope controls carry their
  visible default and `clear_with_filters = false`: Clear all preserves the
  selected bases, scopes and window, the kit-wide policy #1351 recorded.

`ControlBar` owns the only chip row and the `+ Filter` menu; the recipe's
chip markup, `ADDABLE_FILTERS`, `active_filters`, `pending_filter`,
`clear_all` and the duplicate grid summary are removed. The header pills and
the formula strip keep writing the same keys.

## D5 — The grid host

`AnalyzerGrid` gains `market: MarketData` and `subject` and renders
`MarketGrid` instead of `QueryGrid`. The recipe's subject is the output item
at its priced quality, located on the sell world (the cheapest sell-place
listing's world when one exists), with the sell-place listing as its listing
price. That makes every shared column describe the same market the recipe's
own 7-day context describes. The toolbar picker lists the shared columns after
the recipe's groups; toggling preserves foreign ids in `?cols=` the way the
Flip Finder does.

## Verification

Unit tests pin: the default URL's body set and prices (7d, unchanged); a 30-day
window pricing sale bases from 30-day bodies on both sides; buy = sell-world
aliasing per window; the pinned pair still gated on its columns and served by
the slot; exact-quality gil on the revenue side and the min-over-lines rule on
the cost side; the window chip's labels; alias resolution for every legacy
filter key; column-order and sort-token contracts with the two appended ids.
E2E: the analyzer-grid and shared-analyzer-data probes treat the recipe as a
registered host.
