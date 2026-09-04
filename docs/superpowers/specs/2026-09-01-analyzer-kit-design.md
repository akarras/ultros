# Analyzer kit: one ledger, one catalog, three data layers — design

Date: 2026-09-01
Status: draft, pending Aaron's review
Issue: #1233 (part of; never "closes" while phases remain), #1202, #1178
Supersedes the PR plan of `2026-09-01-recipe-analyzer-profit-formula-columns-design.md`
(its recipe-analyzer decisions stand and are re-homed onto the kit; its phases 1a/1b/2 become
Phases A/C/D below)

## Why this spec exists

After reading the profit-formula design, Aaron asked four things: are all the asks in the
thread captured; how does it apply to the other analyzers including the flip finder; can the
components serve multiple tools with different angles; and why does the recipe analyzer lack
columns the flip finder has (the inline sparkline Trend), given that each analyzer is "sort of
a grab bag of tools". This spec re-frames the profit-formula work as a shared analyzer kit
that every profit tool composes, and answers each question with a mechanism.

## 1. Asks audit

Thirty-six distinct asks across #1233, #1202 (parent), #1178 (stale listings), #1245 and
Aaron's follow-up. Against the profit-formula spec: 20 covered, 10 partial, 4 missing, 2
deliberately dropped. The load-bearing findings:

- **#1202 asked for revenue at region scope.** Aaron's own words: "Revenue metric: region
  median / minimum / average, or home-world minimum". PR #1206 shipped it, #1228 kept it as an
  explicit option, #1238 removed it under the 2026-08-30 decision "sell side = one specific
  world". Nobody told Kosyne. Kosyne's "the current scope selector is limited to worlds only"
  points at the page's world picker (labelled "Select World for Sales Data" when she filed,
  now "Sell on"), because the region/DC scope select was hidden inside "+ Filter" at the
  time. The only scope-varying selector in the tool she cites (Saddlebag craftsim) is on the
  revenue side, so her "is it worth DC hopping" is at least as well read as sell-elsewhere as
  buy-elsewhere. The profit-formula spec's Hop gain is buy-side only.
- **The flip finder's column family** (Trend sparkline, drift, profit/day, 30d volume,
  sales/day, confidence) and a shared column kit are absent from the spec.
- **Aaron's #1178 mitigation** ("age-discount or drop listings older than the last N sales")
  needs a listing timestamp the cheapest maps do not carry; the spec's "zero backend" decision
  excluded it without saying so.
- Smaller misses: the statistic-definition tooltips that #1214 dropped were never restored;
  the item-page basis selector Aaron listed as a follow-up in #1202; no default-view signal for
  the Terminus Putty class (a 999,999-gil listing ranked at 363,884% ROI).

| # | Ask (source) | Status after this design | Mechanism |
|---|---|---|---|
| 1 | Cost basis / revenue controls visible (#1233) | covered | #1238 Market menu; strip in Phase C |
| 2 | "Scope selector is limited to worlds only" (the Sell-on picker) | covered, flagged | Phase F `sell-scope` |
| 3 | "No options for DC or Region" | buy side covered; sell side Phase F | `buy-scope` (#1238) + `sell-scope` |
| 4 | Cost basis region median/min/avg (#1202) | covered | `cost-basis` + cost-* columns (D) |
| 5 | Revenue region median/min/avg (#1202; removed by #1238) | restored, flagged | Phase F term + rev-* at the sell scope; Phase 0 comment |
| 6 | Home-world minimum revenue | covered | defaults |
| 7 | Optional DC-only scope | covered | buy default DC; sell DC in F |
| 8 | "Similar to Saddlebag" | deliberately not copied | originality section of the v1 spec |
| 9 | Ranking on the lowest listing rewards fake listings (#1202) | partial; revenue-side tell on the default view | Phase E2 Price sub-line vs 7d median; cost side via D deltas and J; default formula unchanged (decision point 13) |
| 10 | Cost/revenue as column-system extensions (Aaron) | covered | catalog + per-page column tables (B, D) |
| 11 | Columns rather than a forced choice | covered | "use" pills, grouped picker (D) |
| 12 | Multiple bulk ClickHouse queries per selection | covered, budgeted | `needed_bodies`; at most 4 cache keys per view |
| 13 | Port to every analyzer | covered | E1, G, H, I |
| 14 | Recipe analyzer first | covered | A to F |
| 15 | "Both a column AND a selector" (Kosyne) | covered | pills (D) |
| 16 | Selectors define the formula | covered | `ProfitFormula` + strip (C) |
| 17 | Red/green column borders | covered by a different mechanism | badge + sub-label + tint + hairline |
| 18 | Profit follows the selectors | covered | `profit_line` on the selected pair |
| 19 | "Worth DC hopping" | buy half in D; sell half in F, gated on Kosyne's answer | Hop gain (D), `sell-scope` + Scope vs home (F) |
| 20 | "No need to copy SB exactly" | covered | |
| 21 | Tooltips explaining each statistic (#1202 design; dropped by #1214) | covered | four `price_basis_*_help` keys rendered under the stacked strip and as header/picker titles (C) |
| 22 | Sale stats fetched lazily | covered | `needed_bodies` |
| 23 | No-sales items fall back, never 0 | covered | `SignalView` (A) |
| 24 | ClickHouse down non-fatal | covered | `effective()` + amber dot (C); lazy cells show "—" |
| 25 | Basis selector on the item page (#1202 follow-up) | deferred, stated | types reusable there; not phased |
| 26 | Age-discount or drop old listings (#1178, Aaron) | covered | Phase J listing timestamp → Listing age + `max-listing-age` |
| 27 | Stale listings understate cost (#1178, Kosyne) | covered | D delta sub-lines, E2 revenue tell, J listing age |
| 28 | Sale-history cost basis as a guard | covered | #1206, extended in D |
| 29 | Kosyne will test | covered | D and F ask her to validate |
| 30 | Keep pages snappy and accurate (#1245) | covered | default load byte-identical; sort never re-prices; per-row subscriptions 23 or 14 → 1 |
| 31 | Sparkline Trend and the flip finder's columns on the recipe analyzer | covered | Phase E2 |
| 32 | Other analyzers including the flip finder | covered | per-tool matrix, section 6 |
| 33 | Components for different angles | covered | `Term`, policies, `Layer`, `CellValue`, enrichment hook |
| 34 | Grab-bag convergence | covered | one catalog, kinds per definition, derived sortability, page tokens preserved |
| 35 | "Our own spin" | covered | |
| 36 | "The refactor clobbered the goal" | covered | #1238 + strip |

## 2. Decisions

1. **One kit, every profit tool composes it.** A tool is three declarations: a
   `ProfitFormula` ledger (terms fixed or selectable, per-tool policies), a per-page column
   table instantiating a page-independent catalog, and a data-layer assignment per column. The
   row struct, the pricing function, the page's `SortMode` enum, the custom cells that read page
   context, and the table substrate stay per page.
2. **Kit code lands with its first consumer.** Every module in `ultros-app` is `pub(crate)` and
   `check_ci.sh` runs clippy with `-D warnings`, so an unconstructed enum variant fails CI. Each
   phase lists the variants it introduces and constructs.
3. **Sortability is derived, never declared.** `Layer::{RowLocal, Computed, Bulk}` columns
   sort; `Layer::Lazy` columns never sort, never drop rows, and feed floors only under the flip
   finder's unknown-data rule. Trend is lazy on every page.
4. **Ports are number-identical.** `TaxPolicy` and `TaxMath`, `RoiMath` and `DropRule` are
   per-tool policies so each port reproduces today's numbers bit for bit; the only number
   changes are the recipe ROI clamp (C), the subcraft rescue (D), the FC correctness PR (I),
   and opt-in sell-scope or max-age choices.
5. **Scope on both sides.** The sell side is a term too: `Fixed(World)` everywhere through
   E2, `Select` on the recipe analyzer in the flagged Phase F. Defaults stay byte-identical.
6. **Zero backend change through Phase I.** Phase J (listing age) is the only backend work,
   isolated and gated; Phase L is optional.
7. **Kinds name definitions, not labels.** Volume (7d units), Volume (30d units) and the flip
   finder's 30d sales count are three kinds; 7d raw sales/day and 30d cleaned sales/day are two.
   Sub-labels carry the window and source.

## 3. Kit architecture

All new code lives under `ultros-frontend/ultros-app/src/analyzer_kit/`.

| Module | Owns | Lands with |
|---|---|---|
| `formula.rs` | `PriceSignal` (aliases `CostBasis`, `RevenueMetric`; tokens unchanged), `Scope` (= `BuyScope`), `Term<T>`, `RevenueEstimator`, `CostEstimator`, `TaxPolicy`, `TaxMath`, `RoiMath`, `DropRule`, `ProfitFormula`, `ProfitLine`, `profit_line`, `sale_tax_for`, `effective()`, `FormulaMarks`, `sentence()` | Phase A |
| `signals.rs` | `PriceLookup`, `SignalView`, `StatsIndex` (v1 spec, verbatim) | Phase A |
| `layers.rs` | `Layer`, `BodyRole`, `LazyFeed`, `Sortability<M>` | Phase A |
| `needed.rs` | `needed_bodies(formula, columns, visible, sort_target, thresholds) -> BTreeSet<BodyRole>` | Phase A |
| `columns.rs` | `ColumnKind`, `ColumnSpec`, `CATALOG`, `ToolColumnMeta<M>`, `ToolColumn<T,M>`, derivations | Phase B |
| `cells.rs` | `Enrich<V>`, `CellValue`, `CellStyle`, `render_cell` (non-generic) | Phase B |
| `grid.rs` | `AnalyzerRow`, `AnalyzerGrid<T,M>` over the untouched `VirtualScroller`, hscroll sync, `AnalyzerGridSkeleton` | Phase B |
| `strip.rs` | `FormulaStrip { Inline, Stacked }`, `TermBadge` | Phase C |
| `hop.rs` | `HopInfo`, `hop_info` (v1 spec) | Phase D |
| `enrichment.rs` | `Enrichment<K,V>`, `use_visible_enrichment` (lift of the flip finder's effect) | Phase E1 |

Edits to shared code, all additive: `SortableHeaderCell` gains optional `title`,
`sub_label: MaybeProp<String>`, `badge`, `trailing`, `emphasized` (35 call sites unchanged);
`ColumnOption` gains `group`, `disabled`, `hint` plus a constructor (three literal sites
updated); `ToolCalculation::new` takes `impl Into<Signal<String>>` for the formula (six static
callers compile unchanged); `analysis.rs` gains `signed_delta_class(pct, dead_band)` (folds
seven sign-colour copies) and `profit_per_day_from_rate`; `crafting_cost.rs` becomes generic
over `P: PriceLookup + ?Sized` with blanket impls for `&P` and `Arc<P>`, `IngredientLine`
gains `world_id`, `CostBreakdown` gains `unpriced_market_lines`; `PriceSummary::chosen`;
`style/tailwind.css` line 2543 becomes
`calc(var(--tool-fixed-cols, 30.75rem) + var(--tool-optional-cols, 0px))`. Untouched:
`virtual_scroller.rs`, `data_table.rs` (currency exchange, explorer, retainers, lists stay on
it), `skeleton.rs`.

### Core types

```rust
// formula.rs
pub enum Term<T: Copy> { Fixed(T), Select { value: T, url_key: &'static str, default: T } }
pub enum RevenueEstimator { Signal(PriceSignal), FlipEstimate, LeveReward, RegionListingTimesQty, VendorListingNq }
pub enum CostEstimator { Craft(PriceSignal), ListingTimesCount, FcProject, VendorPrice }
pub enum TaxPolicy { MarketBoard, None }   // FromStr: "false" → None, "true" or absent → MarketBoard; Display: None → "false", MarketBoard → absent
pub enum TaxMath { IntegerFloor, F32Truncate }  // agree below 2,207,541 gil; first divergence 2,097,163 vs 2,097,164
pub enum RoiMath { ClampedF64, UnclampedF64, UnclampedF32 }
pub enum DropRule { CostAtOrAboveNet, CostAtOrAboveNetOrZero /* FC: cost == 0 rows drop first */, Never }
pub struct ProfitFormula {
    pub revenue: Term<RevenueEstimator>, pub sell_scope: Term<Scope>,
    pub cost: Option<(Term<CostEstimator>, Term<Scope>)>,
    pub tax: Term<TaxPolicy>, pub tax_math: TaxMath, pub roi: RoiMath, pub drop: DropRule,
    pub sell_quality: SellQuality,
}
pub struct ProfitLine { pub revenue: i32, pub tax: i32, pub net: i32, pub cost: i32, pub profit: i32, pub roi: i32 }
/// Always returns a line; the bool says whether the page's DropRule removes the row.
pub fn profit_line(gross: i32, cost: i32, f: &ProfitFormula) -> (ProfitLine, bool);
pub fn sale_tax_for(gross: i32, math: TaxMath) -> i32;   // the policy-independent Tax column
```

```rust
// layers.rs
pub enum BodyRole { CheapestBuyScope, CheapestSellWorld, CheapestSellScope, SellWorldStats(u16), BuyScopeStats(u16), SellScopeStats(u16), RecentSalesSellWorld }
pub enum LazyFeed { Sparklines { hours: u16 }, ResaleQuality { window: u16 } }
pub enum Layer { RowLocal, Computed, Bulk(BodyRole), Lazy(LazyFeed) }
pub enum Sortability<M> { No, By(M), LazyNever }
```

```rust
// columns.rs — everything URL- or sort-facing is a closure-free `static`
pub type LabelFn = fn(I18nContext<Locale, I18nKeys>) -> String;     // const-constructible, Sync
pub enum ColumnKind { Hq, Item, Actions, Profit, Roi, ProfitPerDay, RevenueSlot, CostSlot,
    RevSignal(PriceSignal), CostSignal(PriceSignal), Tax, SalesPerDay { window: u16, cleaned: bool },
    AvgPrice, VolumeUnits { window: u16 }, SalesCount { window: u16 }, Vwap { window: u16 }, LastSold,
    Confidence, DriftBuffer, Trend, ListingWorld, ListingDc, HopGain, HopWorlds, ScopeVsHome,
    ListingAge, Level, PageSpecific }
/// Page-independent: `static CATALOG: &[ColumnSpec]`.
pub struct ColumnSpec { pub kind: ColumnKind, pub canonical_id: &'static str, pub label: LabelFn,
    pub tooltip: Option<LabelFn>, pub side: Option<FormulaSide>, pub group: PickerGroup, pub default_dir: SortDir }
/// Per page and still a `static`: no closures, the cell is a plain fn pointer.
pub struct ToolColumnMeta<T, M: SortColumn> { pub spec: &'static ColumnSpec,
    pub id: &'static str /* this page's ?cols= token, "" = always on */, pub sort_id: &'static str,
    pub legacy_sort_tokens: &'static [&'static str], pub sub_label: ScopeLabel, pub layer: Layer,
    pub sort: Sortability<M>, pub width_px: u16, pub cell_class: &'static str, pub header_class: &'static str,
    pub skeleton: SkeletonCell, pub mobile: Mobile, pub default_on: bool,
    pub cell: fn(&T, &CellCtx) -> CellValue }
```

`SortMode`'s `FromStr`, `Display` and `default_dir` delegate to `sort_from_token` and
`sort_token` over the page's static table, so the context-free trait impls that
`query_signal::<SortMode>` needs keep working; `ids()` and `defaults()` are `&'static` slices
and flow into `parse_visible_cols` unchanged. `optional_width_px`, `skeleton_columns`,
`picker_options` and the formula marks are pure derivations of the same table. A test per page
pins that every `SortMode` variant is catalogued exactly once and that `w-28` / `w-[88px]`
class literals equal `width_px`.

```rust
// cells.rs — one non-generic match, one element shape per resource-backed variant
pub enum Enrich<V> { Loading, Missing, Ready(V) }
pub enum CellValue { Gil(i32), OptGil(Option<i32>), GilWithNote(i32, Option<String>), MutedGil(Option<i32>, Option<String>),
    Pct(Option<f32>, f32), RoiBadge(i32), Count(Option<u64>), Rate(f32), Confidence(ConfidenceBand),
    LazyConfidence(Enrich<ConfidenceBand>, DerivedConfidence), Cadence(Enrich<(f32, usize)>, Option<(f32, usize)>),
    Sparkline(Enrich<(Arc<[u32]>, f32)>), LazyCount(Enrich<u32>), LastSoldUnix(i64), LastSoldAgo(Option<Duration>),
    Hop(HopGain), Worlds(u8, u8), Text(String), Custom }
pub fn render_cell(style: &CellStyle, v: CellValue, i18n: I18nContext<Locale, I18nKeys>) -> AnyView;
```

```rust
// grid.rs
pub trait AnalyzerRow: Clone + Send + Sync + PartialEq + 'static {
    type Key: Eq + Hash + 'static; fn key(&self) -> Self::Key; fn enrich_key(&self) -> Option<(i32, bool)> { None } }
```

The host keys rows as `(index, row.key())`. The row body is a nested reactive closure, so a
column toggle or an enrichment merge re-renders mounted rows: keyed rendering only calls
`view` for new keys. One subscription per row replaces 23 (flip finder) or 14 (recipe) gate
closures. `CellValue::Custom` hands the cell to the page (Item, Actions, HQ pill, listing
World/DC buttons, the recipe Cost cell with its yield note and subcraft tooltip, the FC
breakdown disclosure).

Hydration invariants: catalog and page tables are ordered slices; visibility derives only from
`?cols=`; enrichment stores start empty on server and client so lazy cells render the skeleton
on both first paints; `render_cell` is the only place per-variant markup lives and each variant
gets a `to_html` shape test; header cells use `use_i18n_or_default()` and the location
fallback. Wasm size is recorded at Phases B and G.

## 4. Formula model

`Profit = revenue.estimator @ revenue.place − tax − cost.estimator @ cost.place`, evaluated
only against the selected pair (drop rule, ROI, filters, default sort). Alternative columns are
informational and may imply a loss. `effective(loaded)` downgrades a sale signal whose body is
absent to `ListingMin` on either side; every label, readout and number uses the effective
formula.

| Tool | Revenue term | Cost term | Scopes | Tax / math | ROI | Drop rule |
|---|---|---|---|---|---|---|
| Recipe | Select `Signal` (`revenue`) | Select `Craft` (`cost-basis`) | sell Fixed(World) → F Select (`sell-scope`); buy Select (`buy-scope`, default DC) | Fixed MarketBoard / IntegerFloor | UnclampedF64 → ClampedF64 in C | CostAtOrAboveNet |
| Flip finder | Fixed `FlipEstimate` (min of 6-sale median and floor, troll guard) | Fixed `Craft(ListingMin)` over region plus cross-region toggles | sell World (path); buy Region(+) | Select via `?tax=` / F32Truncate | ClampedF64 | Never |
| Leve | Fixed `LeveReward` | Fixed `ListingTimesCount` | Region both | Fixed None ("− no tax") | n/a | Never |
| Venture | Fixed `RegionListingTimesQty` | none | Region | Fixed None | n/a | Never |
| FC crafting | Fixed `Signal(ListingMin)` at region → I sell world | Fixed `FcProject` | Region both → I sell World | Fixed None → I MarketBoard | UnclampedF64 | CostAtOrAboveNetOrZero |
| Vendor resale | Fixed `VendorListingNq` (×50 guard) | Fixed `VendorPrice` | sell World | Select via `?tax=` / F32Truncate | UnclampedF32 | Never |

Sell-side scope (Phase F): with `sell_scope != World`, revenue is `SignalView { over:
Some(cheapest of the scope), base: buy-scope cheapest, stats: sale_stats of the scope }`, which
keeps today's rule (sell place wins, buy scope keeps rows priceable); under World it is
today's composition byte for byte, pinned by a parity test that includes items with no
sell-world listing. Velocity, Avg price, Confidence, Last sold, Volume, VWAP, Drift and Trend
stay on the sell world body, so the confidence chip is never hidden. Hop gain stays buy-side;
Scope vs home (revenue signal at the sell scope minus the same signal on the sell world's own
map, None without a home value, at most zero under listings) is the sell-side counterpart. A
best-sell-world signal needs per-world maxima the cheapest maps do not hold and is left out.

The Tax column is `sale_tax_for(gross, TaxMath)` on every tool, independent of the tax term
(the flip finder shows the full 5% under its pre-tax chip today). The strip's "− no tax" term
explains why Profit ignores it on the untaxed tools.

Rendering: `FormulaStrip` renders `[= result] [+ revenue ▾? · place ▾?] [− tax ▾?]
[− cost ▾? · place ▾?]`; a Fixed term is a static chip with badge and title; a Select term
carries a native `<select class="filter-chip-value">` writing its one URL key through the
page's default-stripping setter. Recipe: inline row under "Sell on" (`hidden md:flex
flex-wrap`, never inside the 76px bar) plus the stacked form as the Market popover body. Flip
finder: stacked form inside its Columns popover above the scope toggles (row 1 untouched; the
strip term and the existing pre-tax chip are two controls over one `?tax=` param). Leve,
venture, FC: stacked form in the info panel only. Vendor resale gains a Market button. The
four statistic definitions render as muted help lines under the stacked strip and as titles
on headers and picker entries (a title on a `<select>` explains only the current value).
Header marks, muted alternatives, "use" pills, the live info sentence and the amber degraded
dot are as in the v1 spec.

## 5. Column catalog

Sortability is derived from the layer. Widths: flip finder px from its reservation table;
recipe `w-28` = 112, `w-32` = 128, `w-40` = 160.

| Kind | Recipe id / flip id | Layer (recipe / flip / others) | Sort | New on recipe? Data path |
|---|---|---|---|---|
| Item, Hq, Actions | always | RowLocal | no | page cells |
| Profit, Roi | always / always, `roi` | Computed | yes | ROI clamp in C |
| ProfitPerDay | `profit-per-day` / `profit_per_day` | recipe Computed = profit × rollup sales/day; flip = profit × buffer velocity | yes | new, zero fetch, default off |
| RevenueSlot, CostSlot | Price, Cost / unit and each tool's names | recipe SignalView (RowLocal or Bulk); flip RowLocal | yes | marks only; Price sub-line `‹signal›[ · listing][ · vs median ±n%]` |
| RevSignal ×4, CostSignal ×4 | `rev-*`, `cost-*` / none | rev Bulk(SellWorldStats 7); cost Computed over Bulk(BuyScopeStats 7) for sale signals | yes | v1 Phase 2 (D) |
| Tax | `tax` / `tax` | Computed `sale_tax_for` | yes | |
| SalesPerDay 7d raw | always "Daily sales" | Bulk(SellWorldStats 7) | yes | wires the dead `recipe_analyzer_sales_per_day` key |
| SalesPerDay 30d cleaned | none / `sales_per_day` | Lazy(ResaleQuality 30) with buffer fallback | never | flip only |
| AvgPrice | always | Bulk | yes | unchanged (v1 decision) |
| VolumeUnits 7d | `volume` | Bulk(SellWorldStats 7) `units_sold` | yes | |
| VolumeUnits 30d | `volume-30d` | Bulk(SellWorldStats 30), client-only body | yes | new (E2) |
| SalesCount 30d | none / `volume_30d` | Lazy(ResaleQuality 30) `sample_size` | never | flip only, a count |
| Vwap 7d, Vwap 30d | `vwap`, `vwap-30d` | Bulk 7d / Bulk 30d | yes | 30d new (E2); arm-switch cell fixed |
| LastSold | `last-sold` / `last_sold` | recipe Bulk; flip RowLocal | yes, asc, none last | shared bucket formatter |
| Confidence | `confidence` (on) / `confidence` (on) | recipe Bulk (same band table); flip Lazy + derived | recipe yes / flip never | |
| DriftBuffer | `drift` / `drift` | `price_drift_pct` over the sell world's 6-sale buffer: flip RowLocal; recipe Bulk(RecentSalesSellWorld), needed-gated | yes, none last | new (E2); falls back to the lazy sparkline first-to-last % if the body is too big |
| Trend | `trend` / `trend` | Lazy(Sparklines 168) everywhere | never | new (E2); recipe key (item, stat-row hq); series colour by first-to-last, flip keeps buy-vs-VWAP |
| ListingWorld, ListingDc | `listing-world`, `listing-dc` / `world`, `datacenter` | RowLocal | no | page cells |
| HopGain, HopWorlds | `hop-gain`, `hop-worlds` | Computed | yes | v1 Phase 2 (D) |
| ScopeVsHome | `scope-vs-home` | Computed | yes | F |
| ListingAge | `listing-age` / `listing_age` | RowLocal after J | yes, asc | J |

Recipe picker groups: Revenue · sell place, Cost · buy scope, Travel (hop-gain, hop-worlds,
scope-vs-home), Market (confidence, last-sold, volume, vwap, tax, drift, trend,
profit-per-day, volume-30d, vwap-30d), Location. Daily sales and Avg price stay fixed
columns. Adding a shared-kind sortable column becomes one table entry, one `SortMode`
variant, one comparator arm and at most two i18n keys, against 13 to 14 sites today.

## 6. Data layers, fetch gating, enrichment, capacity

**Bulk.** `needed_bodies` runs at page level (`?sort=` is hoisted beside `?cols=`; thresholds
are an input so a bookmarked floor on a hidden column still forces its body). Recipe rules:
CheapestBuyScope, CheapestSellWorld and SellWorldStats(7) always (byte-identical default);
BuyScopeStats(7) iff a sale cost signal is selected, visible, the sort target or a bookmarked
floor, and the buy scope is not the sell world; CheapestSellScope and SellScopeStats(7) (F)
iff the sell scope is not World, deduped against the buy scope; RecentSalesSellWorld iff Drift
is visible, the sort target or a floor, or outlier filtering is on. Formula bodies join the
Suspense gate. The sell-history failover from the v1 spec is one resource keyed
`(sell world, outliers || drift_needed)` whose fetcher owns the rollup-failure fallback; no
resource is read inside a memo. The 30-day body is a client-only `RwSignal<Option<Arc<
BulkSaleStats>>>` filled by an Effect and `spawn_local`, `None` on the server and first paint,
passed to the table as a signal; it is never read under Suspense (a `LocalResource` there would
render the whole table as a skeleton). When `?sort=volume-30d` or `vwap-30d` is the target,
the fallback sort applies until the body arrives, then the table re-sorts.

**Lazy.** `use_visible_enrichment(store, rows, visible_range, scope, key_of, fetch, cfg)` is
a lift of the flip finder's effect: visible keys with a 30-row margin, generation bump, 150 ms
debounce, bail if superseded or disposed, claim after the debounce, chunk above the cap,
bail if the scope changed, merge and settle all keys on success or error. `requested` stays
non-reactive because the flip finder's filter memo reads the store. The recipe store lives at
page level so a cost-basis switch that remounts the table does not refetch (the two
enrichment endpoints are POSTs, which browsers do not cache). Windows: recipe Container mode
renders 19 rows (viewport 720 minus the 64 px header, over 60 px, plus overscan 8) so 79 keys;
the flip finder is Window mode, 28 rows on SSR (the 20-row fallback plus overscan 8) and
about 32 at 1080p, so 88 to 92 keys; both under the 200-key sparkline and 250-key quality
caps. The tests compute the window from `rows_for_viewport` rather than literals. The
≤100-row tools (leve, venture, FC, scrip) use the whole table as the window; vendor resale does not truncate and uses the visible window.

**Viewport gate.** A lazy column that is `hidden md:*` draws nothing below `md`, so neither
gate that feeds one may open there: `analyzer_kit::enrichment::use_wide_viewport()` (a
`leptos-use` `use_media_query` on `(min-width: 48rem)`, Tailwind's `md` verbatim) is `&&`-ed
into `stats_30_wanted` and `spark_rows_wanted`. **Fetch path only** — the signal reaches a
`Memo` an `Effect` consumes and nothing that renders, because it is `false` on the server and
on the first client render, and a markup branch on it would tear hydration. It is also the
one coupling the phones decision (#7 below) has to carry: whoever gives the recipe analyzer a
horizontal-scroll layout and drops `hidden md:*` must drop this gate in the same change, or
the newly visible columns will never load.
Coverage caveats stated in tooltips: `sales_hourly` accretes from a 30-hour refresh with no
backfill; `item_stats_window` covers about 7% of traded items.

**Capacity per view** (gzip on the wire; the cache budget counts raw JSON):

| View | sale_stats bodies | Cache keys |
|---|---|---|
| Recipe default | (world, 7): 1.85 MB raw, 249 KB wire | 1 |
| plus profit/day, rev-*, hop, tooltips, Price tell | same | 1 |
| plus cost-sale-* or a sale cost basis | + (DC, 7) 3.36 MB / 481 KB, or region 3.92 MB / 578 KB | 2 |
| plus Volume or VWAP 30d | + (world, 30), est. 2.3 MB / 300 KB (unmeasured) | 3 |
| plus Drift | + recentSales (in memory, size unmeasured) | 3 |
| plus Trend | one POST of ≤ 79 keys per scroll settle | 3 |
| plus sell scope Region with buy DC (F) | + (region, 7) | 4 |

Fleet-wide, 64 MiB of raw bodies is roughly 17 to 35 resident bodies. The 30-day columns add
up to one key per sell world; the sell scope adds no new keys because DC and region 7d keys
already exist as buy-scope keys. The byte budget, not the 512-key cap, binds under broad
traffic: an LRU miss is one ClickHouse merge through a two-permit semaphore whose 12 s
timeout includes queue wait and exceeds the 10 s SSR loopback. Levers in order: 30d columns
default off and client-only; watch `ultros_sale_stats_cache_total{disposition=loaded}`; raise
`max_bytes`; Phase L. Client CPU is unchanged from the v1 spec.

## 7. Per-tool composition

| Analyzer | Row key | Default columns | Lazy columns | What changes numbers |
|---|---|---|---|---|
| Recipe (~7k rows) | `(index, recipe.key_id)`; enrich `(item_result, stat_hq)` | Item, Profit, ROI, Cost/unit, Price, Daily sales, Avg price, Confidence, Actions (unchanged) | Trend | C ROI clamp; D subcraft rescue; F and J only when opted in |
| Flip finder (~20k rows) | `(index, item, world, hq, profit)`; enrich `(item, hq)` | HQ, Item, Profit, Buy price + profit_per_day, drift, confidence, world, sales_per_day, last_sold (ROI deliberately off) | confidence, sales_per_day, volume_30d, trend (unchanged) | none (E1, G) |
| Leve (≤100) | `(index, leve.key_id)` | 7 fixed (Revenue and Cost already beside Profit) | Trend optional | none (H) |
| Venture (≤100) | `(index, item_id)` | 6 fixed | Trend optional | none (H) |
| FC crafting (≤100, variable height) | `(index, sequence.key_id)` | 6 fixed; the sales-count badge in `tool_help.rs` renamed `SalesCountBadge` | Trend optional | I: every row |
| Vendor resale (no truncation) | `(index, item_id, profit)`; NQ only | 7 fixed, blank HQ cell kept | Trend (visible window) | none (H) |
| Trends, currency exchange, scrip sources | as today | as today | | header tooltips and `signed_delta_class` only; currency exchange drops its private cols parser |

## 8. Phases

Each phase is one PR against main (stacked PRs get no CI), with local `cargo test -p
ultros-app` and `./check_ci.sh`, and a player-facing changelog line where anything visible
moves. Variant ledger rule: an enum variant is introduced in the phase whose consumer
constructs it (A: `Term::{Fixed, Select}`, `RevenueEstimator::Signal`, `CostEstimator::Craft`,
`TaxPolicy::MarketBoard`, `TaxMath::IntegerFloor`, `RoiMath::UnclampedF64`,
`DropRule::CostAtOrAboveNet`, `Layer::{RowLocal, Computed, Bulk}`, the four recipe
`BodyRole`s; C: `RoiMath::ClampedF64`; D: hop kinds and cells; E1: `Layer::Lazy`, `LazyFeed`,
`Sortability::LazyNever`, the lazy `CellValue`s; F: the sell-scope roles; G: `FlipEstimate`,
`F32Truncate`, `DropRule::Never`; H: the rest).

- **Phase 0 — issue hygiene and two measurements, no code.** A comment on #1233 (drafted in
  the appendix, posted by Aaron or with his go-ahead) explaining that region-scope revenue
  shipped in #1206 and was removed in #1238, and asking Kosyne whether "DC hopping" means
  buying elsewhere, selling elsewhere or both. Measure on prod: one 80-key sparkline POST at
  168 hours (latency and bytes), the 30-day world body size, the recentSales world body size,
  and `min(bucket)` in `sales_hourly` for a world.
- **Phase A — formula model, zero-copy pricing, memo split** (v1 Phase 1a generalised).
  `formula`, `signals`, `layers`, `needed`; generic `compute_cost`; `PriceSummary::chosen`;
  `IngredientLine.world_id`; recipe map clones replaced by views; indexes hoisted;
  `computed_data` split into pure `price_rows` and `filter_and_sort`; `?sort=` hoisted; the
  folded sell-history resource; `key_id` tiebreak; debug-build timing. The raw-sales map stays
  keyed by item (re-keying by quality would change the outlier and failover numbers). Numbers:
  none, pinned by a recorded oracle over a fixture. No changelog. (shipped without `layers.rs`,
  `PriceSummary::chosen` and `IngredientLine.world_id`: none has a Phase A consumer, and an
  unread item fails `-D warnings`; `layers.rs` lands with Phase B/E1, the other two with Phase D)
- **Phase B — column kit and recipe table adoption, byte-identical pixels.** `columns`,
  `cells`, `grid`; `SortableHeaderCell` and `ColumnOption` props; currency exchange's private
  cols parser deleted; `--tool-fixed-cols`. Recipe: the static column table with its seven
  optional ids verbatim, `impl AnalyzerRow`, custom cells, `SortMode` delegating to the table,
  the hand-written header and cell blocks deleted, `visible_range` wired, the VWAP cell's
  arm switch fixed, header labels locale-reactive. Not in this PR: the item link, tooltips,
  new columns. Tests: `optional_column_order_is_a_stable_url_contract` (nothing pins the
  recipe's tokens today), `every_recipe_sort_mode_is_catalogued_exactly_once`,
  `lazy_source_iff_lazy_never`, `cell_class_width_matches_width_px`,
  `render_cell_keeps_one_shape_per_resource_variant`, a recorded `to_html` row and header
  shape. Wasm size recorded. No changelog.
- **Phase C — formula strip, marked headers, live info panel, statistic help** (v1 Phase 1b).
  As the v1 spec, plus the four `price_basis_*_help` keys, the five dead recipe keys wired
  over the hardcoded English, `/recipe-analyzer` in the e2e runner, and the item link change
  only if decision point 2 says so. Numbers: the ROI clamp. Changelog.
- **Phase D — signals as columns, "use" pills, Hop gain and Worlds to visit** (v1 Phase 2).
  As the v1 spec, on the kit; `needed_bodies` gates the buy-scope body. Numbers: the subcraft
  rescue on a small set of rows, delta recorded. Changelog. Kosyne validates hop semantics.
- **Phase E1 — enrichment hook extracted; flip finder switched.** Pure refactor of the flip
  finder onto `use_visible_enrichment`; every existing enrichment, width and URL test green.
  No changelog.
- **Phase E2 — the flip finder's column family on the recipe analyzer.** Profit/day
  (computed, default off), Trend (lazy), Drift (recentSales body, needed-gated; the folded
  resource key gains `drift_needed`; the `(item, hq)` buffer index is built here), Volume 30d
  and VWAP 30d (client-only 30d body), tooltips and "· 7d" sub-labels on Sales/day and
  Confidence, the signed Price sub-line vs the 7d median, Market and Location picker groups.
  Trend and Drift ship only if Phase 0's numbers are acceptable. Numbers: none on existing
  columns. Changelog.
- **Phase F — sell-side scope term and Scope vs home, flagged.** `sell-scope` on the
  recipe analyzer (default world, stripped, counted in active filters, reset by Clear all;
  pinned in the URL-contract test), a fourth Market select and strip term, revenue and rev-*
  over the sell place, `scope-vs-home`. Numbers: none for any existing URL. Changelog. If
  declined, the fallback is rev-* columns at a fixed region scope without a selector.
- **Phase G — flip finder adopts the kit.** Static column table with its underscore `?cols=`
  and hyphen `?sort=` tokens verbatim and its widths, `impl AnalyzerRow`, custom cells, the
  fixed-term strip in its Columns popover, `GridLayout { hscroll: true }`, its width table,
  skeleton mirrors, label match and 23 gate closures deleted. Numbers: none. The SSR shape
  test asserts exactly 20 rows. One changelog line. Ordered before the small ports because its
  columns are the catalog's origin and the hscroll substrate must be proven.
- **Phase H — fixed-formula ports, one PR each: leve first, then venture, vendor resale, FC
  vocabulary.** Static tables, `AnalyzerGrid` in Container mode keeping `hidden md:block`, const
  formulas, stacked strip in the info panel (vendor: a Market button), tooltips, optional
  Trend. Vendor's inline tax and ROI become `profit_line` with `F32Truncate`, `UnclampedF32`,
  `Never`. Numbers: none; each PR pins a pre-kit fixture.
- **Phase I — FC crafting correctness, flagged.** Market-board tax and sell-world revenue;
  rows can drop. Before and after row count and top-20 diff in the PR. Changelog.
- **Phase J — listing age, the only backend change.** `CheapestListingValue` gains
  `listed_at`; rebuild and refill SQL become `DISTINCT ON` ordered by price then timestamp; a
  new covering index created concurrently in a non-transactional migration, the old one
  dropped after, gated on a prod `EXPLAIN` showing an index-only scan and on boot-rebuild
  timing; snapshot versioning so only the cheapest part falls back to the slow path; the wire
  row gains a serde-default field (about 20% larger cheapest bodies). UI: Listing age column on
  the recipe analyzer and flip finder, a stale tone on Cost/unit and Price, and an optional
  `max-listing-age` term under which a too-old listing falls through to the next signal and,
  with none, leaves the line unpriced and counted, never 0. Pinned in the URL-contract test.
- **Phase K — saved views for every tool.** `SavedViewsConfig` with path-world and
  query-world href strategies; recipe built-ins as formula presets; vendor's static preset
  buttons become built-ins.
- **Phase L — optional backend.** A multi-scope, two-window `-If` sale_stats body in one
  region scan, justified only by cache-load telemetry after E2 and F; smoke-tested against a
  live ClickHouse.

## 9. URL and i18n

Every existing key and token stays pinned; `migrate_legacy_params` is untouched; page tokens
are copied verbatim (no canonicalisation on write; `legacy_sort_tokens` empty at launch).
Additive recipe tokens: D appends the v1 spec's ten; E2 appends `profit-per-day`, `trend`,
`drift`, `volume-30d`, `vwap-30d` (no sort token for `trend`); F adds the key `sell-scope`
and the token `scope-vs-home`; J adds `listing-age` and the key `max-listing-age`. Phases F
and J each add a selection key, which the v1 spec's Decision 1 ruled out; both name that in
their PR. `TaxPolicy` parses `false` as None and `true` or absent as MarketBoard. i18n: labels
are function pointers resolved inside reactive closures, so headers become locale-reactive;
new keys roughly C 22, D 26, E2 8, F 6, G 3, H 3 to 5 per tool, J 6, K 5, each in all seven
locales with real translations.

## 10. Decision points for Aaron

1. **Re-open the sell side (Phase F)?** Recommended yes, gated on Kosyne's Phase 0 answer.
   It is your own #1202 ask, shipped by #1206 and removed by #1238 without anyone saying so.
2. **Recipe item link:** keep the buy-scope link, or point the Item cell at the sell world in
   Phase C with a changelog line? Recommended sell world; the row's Price is the sell-world
   price.
3. **Drift on the recipe analyzer:** recentSales body (sortable, same definition as the flip
   finder) or the lazy sparkline delta? Recommended the body, with the lazy variant as the
   downgrade if Phase 0's size is bad.
4. **Profit/day:** opt-in (recommended), default on, or default on and default sort?
5. **30d columns:** ship default-off and watch the cache metric (recommended), gate on L, or
   skip?
6. **Flip finder cost signals:** keep its cost term fixed (recommended) or take the eager
   region sale_stats body?
7. **Phones:** keep `hidden md:block` on the recipe analyzer through F (recommended), then a
   dedicated hscroll PR after G.
8. **Header mark:** badges (recommended) or role words.
9. **FC:** vocabulary port then the flagged correctness PR (recommended) or one PR.
10. **Listing age (J):** ship after E2 and F (recommended) or decline on #1178.
11. **The v1 spec's open points** (VWAP signal deferred, Avg price kept, unpriced rows marked,
    subcraft rescue shipped, subcraft cap, no product columns, no-tax ports first): accept as
    written (recommended).
12. **When does #1233 close?** After F, with the ports tracked on a new issue (recommended).
13. **Default formula:** keep listing-min / listing-min (recommended until Kosyne has used
    the alternative columns) or switch the default cost basis to the sale median once listing
    age exists.

## 11. Incremental delivery and the Labs toggle

The refactor phases (A, B, E1, G, H) ship unflagged: each is pinned byte-identical to the
page it replaces by a recorded oracle, so a flag would only mean maintaining two renderers.
The phases that change what a player sees (C, D, E2, F, J's column) ship behind a **Labs**
toggle, so they can merge to main and run on prod for Aaron and Kosyne before becoming the
default.

Mechanism, fitting what the repo already has: a cookie, because the recipe analyzer renders on
the server and a client-only flag would hydrate differently. `global_state/labs.rs` holds
`Labs { enabled: BTreeSet<String> }` with `FromStr`/`Display` as a comma-separated list under
the cookie name `LABS`, read through `Cookies::use_cookie_typed`, the same pattern as
`CraftOptions` and the theme cookies. Each experiment is a `&'static str` token. The
recipe analyzer has exactly one, `analyzer-recipe`: Phase E2 merged Phase C's
`analyzer-ledger` and Phase D's `analyzer-signal-columns` into it (one tool, one toggle —
separate flags per phase made "which permutation is this?" a question), and Phase F's sell
scope ships under the same token. `use_lab(token) -> Signal<bool>` is true when the cookie set
contains the token or the page URL carries `?labs=token[,token]`, so a link can be shared with
a tester without touching their settings. The Settings page gains a "Labs" section listing the
live experiments as `Toggle`s with a one-line description each, all strings in the seven
locales. When a flag is off, the page renders exactly as before the phase, pinned by the same
shape test the phase adds. Every flag names the phase that removes it: a flag is deleted, and
its behaviour becomes the default, in the phase after the one where Kosyne has validated it,
so at most three flags exist at any time. The toggle module lands with Phase C, its first
consumer.

## 12. Deliberately left out

Scope × signal product columns; a best-sell-world signal; a bulk sparkline endpoint, cheapest
ladder, p25/p75, cleaned median, Real Price in bulk; canonicalising the flip finder's
underscore tokens; unifying tax rounding or ROI math (identical below 2,207,541 gil; a later
flagged one-liner); a window selector; profit bands and presets beyond saved views; migrating
the profit analyzers to `DataTableGrid`; the item-page basis selector; the 2026-08-29 spec's
phases 2 and 3 (their fields are laid down); a non-gating resource for the formula bodies;
sub-recipe memoisation unless Phase A's timing says so; hop and drift filter chips; server
fixes noted in passing (one listing per world per event, read guards across ClickHouse awaits,
the ghost-row predicate, which measured negligible at 7d).

## Appendix — draft comment for #1233 (Phase 0)

> Quick status and one question. The region-scope revenue options from #1202 ("region median /
> minimum / average") shipped in #1206, then #1238 reworked the model around a buy/sell split
> and made revenue always the sell world's price, so they went away without a note here. That
> was a simplification on our side, not a decision that the ask was wrong. The current plan
> restores a "Sell across" scope (world, datacenter, region) on the revenue side as an opt-in
> term, alongside the buy scope that already exists.
>
> The question: when you say it's useful for seeing whether DC hopping is worth it, do you
> mean buying ingredients on another world, selling the product on another world, or both?
> The buy-side answer is a "Hop gain per unit" column; the sell-side answer is the scope term
> above. Knowing which you actually use decides what ships first.
