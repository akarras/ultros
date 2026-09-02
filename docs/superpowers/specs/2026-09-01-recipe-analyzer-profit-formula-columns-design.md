# Recipe Analyzer: the profit formula as columns — design

Date: 2026-09-01
Status: draft, pending Aaron's review. The model, UI vocabulary, URL contract and decision
points here stand; the PR plan (Phases 1a/1b/2/3) is superseded by
`2026-09-01-analyzer-kit-design.md`, which re-homes them onto a shared analyzer kit (its
Phases A/C/D) and adds the sell-side scope, the flip finder's column family and the ports.
Issue: #1233 (follow-up after #1238 / #1239 / #1240 / #1248; does not close it — the port
to the other analyzers stays open)

## Problem

After the market-model PRs the recipe analyzer has a Market popover (Buy from / Cost basis /
Revenue), a Columns picker, and rollup-backed sale stats. What the #1233 thread still asks for:

- Kosyne: the selectors should *define the profit formula*; the underlying price signals should
  be *visible as columns*; the columns that feed Profit should be *visibly marked* (Saddlebag
  uses red/green column borders); and the whole thing should answer "is it worth hopping to
  another DC/region".
- Aaron: cost basis and revenue should become *extensions of the column system* (add them as
  columns rather than an exclusive choice), iterate on the recipe analyzer first, port to every
  analyzer later, and make it an original take rather than a Saddlebag copy.

Today nothing links the Market selection to any column: the formula is a static string in the
collapsed info panel ("profit = (market price − 5% tax) − cost per unit"), Cost / unit and Price
show only the selected basis, and the alternatives are invisible. On prod, Terminus Putty shows
ROI 363,884% because revenue is one 999,999-gil listing on Gilgamesh — exactly the kind of row
the sale-history signals exist to expose.

Measured on prod today (7-day window, gzip on the wire): sale_stats for a world is 9,250 rows /
249 KB, a datacenter 16,713 / 481 KB, a region ~20k / 578 KB; cheapest listings are 89 / 147 /
171 KB. Ghost rows (rollup rows whose last sale left the window) are 38 of 9,250 on Gilgamesh
and 43 of 16,713 on Aether, all under 14 days old — refresh lag, not accumulation, so no
backend fix is needed for this work.

## Decisions

1. **The formula is the control.** Profit / unit = ‹revenue signal on the sell world› − 5% tax −
   ‹cost signal over the buy scope›. The three existing URL params (`revenue`, `cost-basis`,
   `buy-scope`) *are* the formula; no new selector dialect, no new URL key for selection.
2. **Signals are columns; the formula's inputs are marked columns.** Four signals per side —
   cheapest listing, 7d sale minimum, 7d sale median, 7d sale average — each addable as a
   sortable column through the Columns picker. The column whose signal is selected is the
   existing Cost / unit or Price slot; the others render muted with a delta against the input
   and carry a "use" pill that makes them the input. Nothing disappears on a swap.
3. **No red/green.** The mark is the formula's own arithmetic: an operator badge (`=` `+` `−`)
   and a `‹signal› · ‹place›` sub-label on the three formula-term headers, plus a brand-tinted
   header with a bottom hairline. Palette-safe, colour-blind-safe, visible on phones.
4. **Hop gain / unit and Worlds to visit** answer the DC question in the table, computed from
   the sell-world listings and stats the page already fetches. Zero new network.
5. **Zero backend change.** Every signal is already in `ItemSaleStats`; every scope is already
   served. The default page load stays byte-identical on the wire.
6. **Three PRs against main, then ports.** 1a is a pure refactor with pinned-identical numbers;
   1b is the formula UI; 2 is the columns, pills and hop columns. Stacked PRs get no CI, so
   each targets main and later ones rebase with `rebase --onto`.

## Model

All new pure code lives in `ultros-frontend/ultros-app/src/profit_formula.rs` (new, DOM-free,
unit-tested) and additive changes to `price_basis.rs`, `components/crafting_cost.rs` and
`ultros-api-types/src/cheapest_listings.rs`.

- `PriceSignal { ListingMin (default), SaleMin, SaleMedian, SaleAvg }` with the existing tokens
  `listing-min | sale-min | sale-median | sale-avg`. `CostBasis` and `RevenueMetric` become type
  aliases of it so every call site, chip and test compiles unchanged. VWAP is *not* a signal in
  this design (see decision points).
- `ProfitFormula { revenue: PriceSignal, cost: PriceSignal, buy_scope: BuyScope, tax: TaxPolicy,
  sell_quality: SellQuality }`. `TaxPolicy::{MarketBoard (5%, integer floor, today's
  `net_after_tax`), None}` exists for the leve/venture/vendor ports. `SellQuality::{Either,
  PreferHq}` is reserved for the specced "assume HQ sale" toggle and is `Either` everywhere in
  this design. `from_query(..)` builds it from the three params; `effective(buy_stats_loaded,
  sell_stats_loaded)` downgrades a sale signal whose body is absent to `ListingMin`, and every
  label, sub-label, readout and computation uses the *effective* formula, so the UI never names
  a signal the numbers do not use.
- `profit_line(gross, cost_per_unit, tax) -> Option<ProfitLine { revenue, tax, net, cost, profit,
  roi }>` is the one place the drop rule lives: `None` when `cost_per_unit >= net`. ROI uses the
  existing `analysis::return_on_investment` (0 when cost ≤ 0, clamped at ±100,000 — Terminus
  Putty reads 100,000% instead of 363,884%). `per_unit_cost` moves here with its test. The drop
  rule, ROI, filters and the default sort are evaluated **only against the selected pair**;
  alternative columns are informational and may imply a loss.
- `PriceLookup` trait (`find_matching_listings(item_id) -> PriceSummary`) implemented by
  `CheapestListingsMap`, `&P` and `Arc<P>`; `compute_cost` and friends become generic over it.
  `SignalView<'a> { over: Option<&CheapestListingsMap>, base: &CheapestListingsMap, stats:
  Option<(&StatsIndex, SaleStat)> }` reproduces today's `override_listings` +
  `overlay_sale_stats` composition lazily, per lookup, with no map clones (a parity test pins
  price *and* world_id against the two helpers). `stat_only(item, hq)` returns the bare
  statistic with no fallback, for alternative revenue columns.
- `PriceSummary::chosen(prefer_hq) -> Option<CheapestListingData>` replays `lowest_gil` /
  `price_preferring_hq` but returns the entry (LQ wins an equal-price tie under lowest, HQ under
  prefer), property-tested against the two existing functions. `IngredientLine` gains
  `world_id` (0 for vendor, subcraft and unpriced lines).
- `CostBreakdown` gains `unpriced_market_lines: u16`: lines with `source == Market &&
  used_from_market > 0 && unit_price == 0`, excluding shards under `ExcludeShards` and items with
  a vendor price, counted after the shard flag and the subcraft pass, propagated from the winning
  sub-run. One algorithm fix rides along: a 0-priced line can now be rescued by a cheaper
  sub-recipe (`unit_cost == 0 || sub_unit < unit_cost`), so an unlisted intermediate is no longer
  free when it is craftable. This raises Cost / unit on a small set of default-view rows and is
  changelog'd in Phase 2.
- `SIGNAL_COLUMNS: [SignalColumn { side, signal, col_id }; 8]` — one ordered const that drives
  `?cols=` tokens, `?sort=` tokens, picker labels, and the header and cell loops
  (hydration-safe: no map iteration reaches the DOM).
- `NeededSignals { cost: BTreeSet<PriceSignal>, hop: bool, worlds: bool }` from
  `needed_signals(formula, visible_cols, sort, use_subcrafts)` = {effective cost} ∪
  {visible cost-* columns} ∪ {the sort target's cost signal} ∪ {ListingMin when Worlds to visit is
  visible}; `hop` / `worlds` are set when either column is visible or is the sort target. With
  subcrafts on, the set keeps the selected signal plus
  the first two visible cost-* columns in `SIGNAL_COLUMNS` order; extra cost columns render "—"
  with a title. The cap is enforced here, not in the picker, so it holds for any bookmarked URL
  and identically on SSR and CSR.
- `HopInfo { gain: HopGain::{Gain(i32), Needed, Unavailable}, worlds: u8, dcs: u8, world_ids }`.
  Home cost = `compute_cost` over `SignalView { over: None, base: sell-world listings, stats:
  sell-world index under the selected cost signal }` — deliberately *not* layered over the buy
  scope, otherwise an ingredient with no home listing would be priced at the scope price and
  zero the gain for exactly the ingredients that force the trip. `gain = home − scope` per unit,
  signed and never clamped (negative is possible under sale signals and under require_hq; the
  tooltip says so). `Needed` when the home run has unpriced lines; `Unavailable` when the scope
  run has unpriced lines or Buy from = This world only. Worlds to visit = distinct non-zero,
  non-home `world_id` over the *listing-min* scope run's top-level market lines; `dcs` groups
  them by datacenter. Sub-craft materials are not counted (stated in the tooltip).

Semantics stated in the info panel: ingredients use `price_preferring_hq` under require_hq
(vendor floor skipped there) else `lowest_gil`, identically for every cost signal; revenue for
every signal uses the cheaper of NQ/HQ on the sell world (today's rule). A missing or zero stat
falls through per ingredient to the scope listing — never to 0. Alternative *revenue* columns
show "—" when the sell world has no stat row; the Price slot keeps its buy-scope listing
fallback and marks it with a 10px "listing" sub-line. Unpriced ingredients still cost 0 under the
selected formula (row membership unchanged) but are counted and shown as "n unpriced" on the
Cost / unit cell; dropping such rows is a decision point.

## UI

**Formula strip.** A `FormulaStrip` component renders four chip-styled terms (`.filter-chip`
vocabulary, brand-ring tint): `[=] Profit / unit`, `[+] ‹revenue signal› ▾ · ‹sell world›`,
`[−] 5% tax`, `[−] ‹cost signal› ▾ · ‹Buy from› ▾`. The two ▾ terms are native
`<select class="filter-chip-value">` elements reusing `cost_basis_options` /
`buy_scope_options` and the existing default-stripping setters, so a change writes the same
URL params the Market popover writes today; the recompute is client-side and instant.

- Desktop (≥ md): the strip is its own full-width row directly beneath the "Sell on" row
  (outside Suspense), `hidden md:flex flex-wrap gap-2`. `.filter-chip` is `nowrap`, so the row
  is allowed to wrap onto two lines at md and at 1024px-with-sidebar; it never enters the 76px
  ControlBar and never widens the page.
- Every width: the Market popover body becomes the same component in `Stacked` layout (one
  term per line, popover widened from 16rem to 20rem, 92vw cap). This is the phone's formula
  surface and stays reachable while scrolled deep into 7k rows.
- The info panel's calculation block goes live: "profit / unit = Sale median (7d) on Gilgamesh
  − 5% tax − Cheapest listing across Aether". `ToolCalculation.formula` becomes
  `Signal<String>`; `new()` keeps accepting strings via `impl Into<Signal<String>>` so the six
  static callers compile unchanged. The sentence is built from already-resolved label strings
  (the route does the `t_string!` work) so `profit_formula.rs` stays i18n-free and testable.

**Formula-term headers.** Profit, Cost / unit and Price widen from `w-32 p-4` to `w-40 px-3
py-2 leading-tight flex flex-col` (cells follow; default table ~1,280 → ~1,376px inside the
existing overflow-x-auto wrapper) and render two lines: line 1 = `[badge] label [sort arrow]`
(label `truncate`, full text in `title`); line 2 = 10px muted `‹short signal› · ‹place›`
("listing · Aether", "7d median · Gilgamesh"; Profit's reads "per unit · after 5% tax"). The
three headers are emphasized: brand-ring 18% tint over the row's 10% and a 2px inset *bottom*
hairline (`shadow-[inset_0_-2px_0_var(--brand-ring)]`, the active-tab idiom — a top rule on the
first row of a rounded panel reads as chrome). The badge is a 16px bordered mono square with an
`aria-hidden` glyph and an `sr-only` role name inside it ("adds to profit", "subtracted cost");
the header cell keeps its visible name for screen readers. Hovering a Profit cell shows that
row's arithmetic: "12,560 (price) − 628 (tax) − 11,300 (cost / unit) = 632".

`SortableHeaderCell` gains optional, backward-compatible props: `title`, `sub_label`, `badge`,
`trailing`, `emphasized`. DOM order is the SortHeader `<a>` first, then the sub-label `<span>`
containing any trailing `<button>` — two focus stops, never nested.

**Columns picker.** Grouped headings rendered from an ordered `PICKER_COLUMNS` Vec (distinct
from `OPTIONAL_COLUMN_ORDER`, which stays the parse/serialize contract): "Revenue · Gilgamesh"
(Cheapest listing, Sale minimum, Sale median, Sale average), "Cost · Aether" (same four),
"Travel" (Hop gain / unit, Worlds to visit), "Other" (Confidence, Last sold, Volume, VWAP, Tax,
World, Datacenter). The entry equal to the selected signal is suffixed "(= Price)" /
"(= Cost / unit)". The Cost group heading carries the title "Shows sale history for Aether
(loads once)" because ticking the first sale-cost column fetches the buy-scope stats body and
remounts the table — the same behaviour as choosing a sale cost basis today. `ColumnOption`
gains `group`, `disabled` and `hint` so the subcraft cap can grey out extra cost columns with
a note.

**Alternative signal columns.** Muted text (`--color-text-muted`; `<Gil>` sets no colour class
so it inherits) with an always-present 10px sub-line showing the delta against the same-side
formula input ("+38%", title "vs the formula's cost input") — the #1178 thin-listing tell per
row. Headers carry no badge and no tint; line 2 = sub-label plus an outlined "use" pill
(`<button type=button aria-pressed>`, calculator icon, aria-label "Use Sale median (7d) as the
cost in Profit"), always visible. Pressing it writes exactly one param (`cost-basis` or
`revenue`) through `filter_query_signal`, which sidesteps the leptos_router same-frame batching
rule; the badge, tint, hairline and sub-label move to the slot header; the pressed column stays
on screen as a muted duplicate marked "(= Cost / unit)" with its pill filled and disabled.

**Hop cells.** Hop gain renders a signed `GilOrDash`, the text "needed", or "—", inside one
fixed element shape (class toggle, never an arm switch), with a title "≈ 13.5k gil/day at 6.3
sales/day". Worlds to visit renders a count with a tooltip listing "world · n ingredients"
grouped by datacenter from a first-appearance-ordered Vec, plus "m datacenters" and "sub-craft
materials not counted". Every new cell keeps one element shape between value and no-value
states (the `GilOrDash` rule); the existing VWAP cell's arm-switch is fixed in passing.

**Mobile.** Row 1 of the ControlBar is unchanged (icon-only). The inline strip is hidden below
md; the stacked strip in the Market popover is the formula surface. The five always-on columns
(now 944px wide inside the horizontal scroller, 848 today) keep badge, tint, hairline and a
truncated sub-label on Cost / unit and Price. Alternative and Travel columns keep the page's
`hidden md:block` convention — the hop answer is desktop-only in this design; adopting the flip
finder's hscroll table is a separate change.

**ClickHouse down.** The sell-world stats fetch failing still mounts the table with the amber
banner; `effective()` downgrades a sale revenue signal to the listing, alternative revenue cells
show "—", hop under a sale cost signal degrades to the listing pass. The strip's affected term
shows a 6px amber dot ("Sale history unavailable — using cheapest listing"). The degraded flag
reaches the strip (outside Suspense) only through an `RwSignal` written by an Effect inside the
mounted table, so SSR and the first client paint render the *selected* formula and no resource
is read outside Suspense.

**Kosyne deciding whether to hop** (Gilgamesh on Aether, defaults). She opens Columns, ticks
Hop gain / unit and Worlds to visit (no network), sorts Hop gain descending. Row "Grade 8
Tincture of Strength": Cost / unit 11,300 (listing · Aether) · Price 12,560 (listing ·
Gilgamesh) · Tax 628 · Profit 632 · Hop gain +2,150 · Worlds 2 (Cactuar, Adamantoise; 1
datacenter) · Daily sales 6.3 (≈ 13.5k gil/day of hop value). Buying everything at home costs
13,450/unit, so the home-only profit is −1,518 and the row would not exist without the trip. A
row showing "needed" has an ingredient with no Gilgamesh listing and no vendor. She flips the
cost term to Sale median (7d): Hop gain becomes signed (negative = home is cheaper, stay). She
switches Buy from to Region: Hop gain re-frames as region-trip-vs-home and the Worlds tooltip
groups by datacenter. The tooltip states the comparison is buy-side only — revenue stays the
sell world by the 2026-08-30 decision — so it is not read as Saddlebag's home-vs-regional
revenue comparison.

## Data flow

The same five resources, no new join term in the Suspense gate:

1. `GET /api/v1/cheapest/{buy scope}` — always.
2. `GET /api/v1/sale_stats/{buy scope}?window=7` — iff `needed_signals().cost` contains a sale
   signal, and not when the buy scope is the sell world (the table reuses the sell-world index;
   one body instead of two identical ones). With no cost-* column visible and no cost-* sort this
   is today's gate exactly, so the default load and every existing bookmark fire the same
   requests.
3. `GET /api/v1/cheapest/{sell world}` — always.
4. `GET /api/v1/sale_stats/{sell world}?window=7` — always (since #1248). This one body feeds
   Price under every revenue signal, all four revenue columns, the stat columns, and the
   sale-signal variants of Hop gain.
5. Sell-world history failover: the rollup fetch and the raw recent-sales fetch fold into **one**
   resource keyed on `(sell world, filter_outliers)` whose fetcher tries the rollup and, on
   error or when outlier filtering is on, also fetches recent sales, returning a struct with both
   optionals. This removes the ArcResource read inside a Memo (the #1248 hydration warning)
   without an Effect, so the SSR failover path keeps working when ClickHouse is down.

Per UI state: default URL → 1, 3, 4 (byte-identical to main). Any revenue column, Hop gain,
Worlds to visit, or "use as revenue" → still 1, 3, 4. Any cost-sale-* column, sale cost basis,
or `?sort=cost-sale-*` → + 2 (one cached bulk body, one `SaleStatsCache` key). Cache keys per
page view stay at most `(sell world, 7)` and `(buy scope, 7)`; no SQL changes.

`needed_signals` is computed **at page level** (`?sort=` is hoisted from the table to the page
alongside `?cols=`, and the table receives `needed` and `sell_world` as props — the table today
only receives the buy-scope name), because the fetch-2 key must see the sort target.

Memos in the table: `sell_stats_index` / `buy_stats_index` built once per body (today the map is
rebuilt on every recompute); `formula`, `needed`; `priced = price_rows(&PriceInputs) ->
Vec<PricedRecipe>` keyed on payloads, indexes, levels, craft options, `formula`, `needed`, and
`job_filter` read once above the loop — per recipe it runs `compute_cost` once per needed cost
signal (+ once against the home view when hop or worlds is on) with a fresh on-hand snapshot per
run only when on-hand is enabled, calls `profit_line` for the selected pair, and fills
`rev_alt`, `cost_alt`, `hop`, `unpriced`, `revenue_fell_back`; `rows = filter_and_sort(..)` keyed
on thresholds and sort/dir. A header click re-sorts ~7k Arcs and never re-runs `compute_cost`
(today it re-runs everything). `compare_recipes` gains a `recipe.key_id` tiebreak — the input is
a std HashMap and ties would otherwise order differently on SSR and CSR for the 20 SSR rows.

`compute_cost` runs per recipe: default 1 (unchanged); hop view 2; typical exploration 2–3;
maximum without subcrafts 4 + 1 = 5; with subcrafts the cap makes it ≤ 3 + 1 = 4. A run without
subcrafts is ≤ 8 ingredients × 3 map lookups, so 7k × 5 ≈ 100–200 ms worst case, paid once per
selection change and never on sort or filter. Phase 1a logs the pass duration in debug builds
so Phase 2 is gated on a measured number; per-(sub-recipe, signal) memoisation is the named
next lever if that number is bad.

## URL contract

All existing keys and tokens are kept and stay pinned by the existing tests; defaults are still
stripped, so a bare URL means what it meant yesterday; `migrate_legacy_params` is untouched.
Additive in Phase 2: `?cols=` accepts ten tokens appended to `OPTIONAL_COLUMN_ORDER` after the
existing seven — `rev-listing-min, rev-sale-min, rev-sale-median, rev-sale-avg,
cost-listing-min, cost-sale-min, cost-sale-median, cost-sale-avg, hop-gain, hop-worlds`
(`parse_visible_cols` drops unknown tokens, so old clients degrade gracefully and every
serialized old URL is byte-identical; `DEFAULT_COLS` stays `[confidence]`). `?sort=` accepts the
same ten via `SortMode::{RevSignal(PriceSignal), CostSignal(PriceSignal), HopGain, HopWorlds}`
(Display `rev-{token}` / `cost-{token}` / `hop-gain` / `hop-worlds`; 21 variants total; `cost`,
`price`, `vwap`, `avg-price`, `tax` keep their meaning). Hop columns sort with `cmp_none_last`;
`Needed` / `Unavailable` sort last in both directions; `HopWorlds` defaults ascending. New tests:
`optional_column_order_is_a_stable_url_contract` (pins all 17 tokens and `DEFAULT_COLS` — nothing
pins them today), `picker_columns_are_a_subset_of_optional_column_order`, and the sort round-trip
extended to 21 variants with malformed `rev-` / `cost-mars` rejected.

## Phases

**1a — Formula model, zero-copy pricing, memo split (no UI change).** `profit_formula.rs`;
`PriceSignal` aliases; `PriceLookup` + `SignalView` + `StatsIndex`; generic `compute_cost` /
`compute_ingredient_cost` / `calculate_fc_project_cost` (callers in recipe_analyzer, item_view,
related_items, fc_crafting_analyzer recompile); map clones replaced by borrowed views; indexes
hoisted; `computed_data` split into `priced` + `rows`; `job_filter` read once; on-hand clone
moved under `use_on_hand`; `key_id` tiebreak; folded sell-history resource; stale comments at
recipe_analyzer.rs:402-408 / 629-633 / 1893-1895 corrected; debug-build timing. No changelog
entry (purely internal). Tests: `profit_line_*`, `per_unit_cost_divides_by_yield` (moved),
`effective_downgrades_absent_sale_signal_to_listing`, `signal_view_matches_override_then_overlay
_on_fixture` (price and world_id), `signal_view_never_prices_missing_stat_at_zero`,
`stat_only_has_no_fallback`, `chosen_matches_lowest_gil_and_prefer_hq_with_tie_rule`,
`price_rows_matches_recorded_oracle_on_fixture` (recorded `(key_id, profit, roi, cost,
market_price, tax)` tuples, since the old loop is deleted), `filter_and_sort_is_pure_and_inclusive`,
`compare_recipes_breaks_ties_by_key_id`; existing 22 route tests and 9 price_basis tests
unchanged. Item-page e2e screenshots re-run (its callers recompile).

**1b — Formula strip, marked headers, live info panel.** `components/formula_strip.rs`
(Inline + Stacked, `TermBadge`); strip row under "Sell on"; Market popover body = stacked strip;
`SortableHeaderCell` props; w-40 two-line formula headers with badge, sub-label, tint,
hairline; Profit cell readout; reactive `ToolCalculation`; degraded dot via Effect-written
signal; ROI via `analysis::return_on_investment`; the five dead `recipe_analyzer_subcraft_*` /
`_sales_per_day` / `_item_level_label` keys wired over the hardcoded English at
recipe_analyzer.rs:1640-1644 / 1697 / 1707 / 1723; `/recipe-analyzer?world=Gilgamesh` added to
`integration/runner.cjs` (desktop + 375px). i18n ×7 (~18 keys + reworded
`recipe_analyzer_calc_formula` with four placeholders, grep-checked per locale). Changelog:
"Recipe Analyzer: the profit formula is now a control — pick the revenue and cost signals in
the header and see exactly which columns feed Profit" (mentions the ROI clamp). Tests:
`strip_terms_render_identical_shape_with_and_without_world`,
`term_select_writes_default_stripped_value`, `sortable_header_cell_renders_sub_label_badge_and
_trailing` (to_html under an Owner, precedent at sort_header.rs:440-457),
`tool_calculation_accepts_static_strings_and_signals`,
`formula_sentence_names_signal_world_scope_and_tax`,
`profit_readout_arithmetic_matches_profit_line`, `roi_is_clamped_at_display_ceiling`. Manual:
375/768/1024 in en/fr/de against prod CSS (strip wrapping, popover at 92vw, light-mode
hairline), ClickHouse-down curl for the amber dot.

**2 — Signals as columns, "use" pills, Hop gain / Worlds to visit.** `SIGNAL_COLUMNS` + hop
tokens appended to `OPTIONAL_COLUMN_ORDER`; `PICKER_COLUMNS` with groups, "(= Price)" suffix,
loads-once title, subcraft cap note; `SortMode` variants; page-level `needed_signals` re-gating
fetch 2 and the buy-equals-sell index reuse; `PricedRecipe` alt/hop/unpriced fields; muted cells
with delta sub-line; "use" pills; Price "listing" fallback sub-line; "n unpriced" note;
`IngredientLine.world_id` + `PriceSummary::chosen`; `CostBreakdown.unpriced_market_lines`; the
0-priced subcraft rescue; hop cells and tooltips. i18n ×7 (~26 keys; reuse the bare `revenue` /
`cost` / `datacenter` / `region` keys for group words). Changelog: "Recipe Analyzer: every
price signal is a column you can sort, and Hop gain tells you whether the trip to another world
pays" (+ the subcraft-rescue sentence, with the prod row-count delta recorded in the PR). Tests:
`ingredient_line_records_the_chosen_listing_world`, `unpriced_lines_counted_after_shard_flag
_and_subcraft_pass`, `unpriced_ignores_excluded_shards_and_vendor_sold`,
`zero_priced_line_can_be_rescued_by_subcraft`, `signal_columns_have_unique_ids_and_sort_tokens`,
`needed_signals_is_selection_union_visible_union_sort_target`,
`needed_signals_sets_hop_when_a_hop_column_is_the_sort_target`,
`subcraft_cap_applies_to_url_bookmarks`, `hop_gain_is_home_cost_minus_scope_cost_signed`,
`hop_is_needed_when_home_has_unpriced_lines`,
`hop_is_unavailable_when_scope_has_unpriced_lines_or_world_scope`,
`hop_worlds_counts_distinct_non_home_listing_worlds_and_dcs`,
`hop_needed_sorts_last_both_directions`, `buy_stats_fetch_only_when_a_sale_cost_signal_is
_needed`, `buy_stats_key_is_none_when_buy_scope_is_the_sell_world`,
`alt_columns_never_change_row_membership`, `revenue_alt_columns_are_none_without_sell_world
_data`, `use_as_pill_writes_exactly_one_param`, `grouped_picker_keeps_option_order`. Manual:
fr/de picker at 375px; the 1a timing numbers recorded in the PR for K = 1, 2, 4 with and without
subcrafts. Ask Kosyne to validate the hop semantics on the PR before merge.

**3 — Ports (one PR each, leve first).** Leve: badges/sub-labels/live calculation with a fixed
`ProfitFormula { ListingMin, ListingMin, Region, TaxPolicy::None }` (the readout says "− no
tax"; numbers unchanged; a fixed side renders a static badge, never a pill). Venture:
revenue-only strip. Vendor resale: fixed cost term, `PriceSignal` revenue term on the sell
world, `?tax=` mapped to `TaxPolicy`. FC crafting: vocabulary port first, then a separately
flagged correctness PR adopting `profit_line` with market-board tax and sell-world revenue.
Flip finder last: fixed revenue term (its estimator is min(6-sale median, floor) with a troll
guard, not a `PriceSignal`), header badge/sub-label props replacing its seven hand-wrapped
`title=` headers, `?tax=` mapped to the tax term; cost-* columns only if it takes the eager
region sale_stats decision.

## Non-goals and things deliberately left out

- Any backend change: the multi-scope `-If` sale_stats query, a cheapest-ladder endpoint,
  p25/p75, a ghost-row predicate (measured negligible), a per-column window.
- Scope × signal product columns (DC vs region side by side) — Hop gain answers the stated
  question from bodies already loaded; Buy from = Region re-frames it.
- A profit band / scenario presets — presets are not monotone against the data ("Optimistic"
  listing/listing is often below the median in undercut markets) and add a legend dependency.
- VWAP as a selectable signal — see decision points.
- A "region-competitive" revenue token — a model change against the sell-world-only decision.
- ▾ selectors on the formula-term table headers — no width; the strip, popover and "use" pills
  cover selection.
- Re-homing or retiring the always-on "Avg price" column — its number would move on the
  default view; the per-quality version is the opt-in Revenue · Sale average column.
- Dropping rows with unpriced marketable ingredients — counted and marked instead.
- Auto-hiding the alternative column that duplicates the selected signal.
- A non-gating resource for the buy-scope stats body (to avoid the remount when a sale-cost
  column is first ticked) — same behaviour as switching cost basis today.
- The 2026-08-29 spec's phases 2–3 (gil/day sort, search chip, HQ toggle, expandable row,
  craft-list handoff) — `SellQuality`, `IngredientLine.world_id` and `unpriced_market_lines` are
  laid down so they slot in later.
- Migrating the table to `data_table.rs`; the flip finder's hscroll mobile table; generalising
  `SavedViewsMenu` (the later home for named presets); fixing the row item link that points at
  the buy scope (recipe_analyzer.rs:1653, pre-existing).

## Decision points for Aaron

1. **Header mark: operator badges (`=` `+` `−`) or role words ("revenue", "cost") in the
   sub-label?** Recommended: badges (bordered squares with the signal sub-label beside them; the
   per-row readout teaches them on first hover). Known collision: the `+` glyph next to row 2's
   "+ Filter". If a fr/de review at 375px dislikes it, swapping to role words is a two-key change
   in one component.
2. **"Use as cost / revenue" header pills in Phase 2, or strip + popover only?** Recommended:
   ship the pills. They are what makes a column "both a column and a selector" and no surveyed
   tool has them; without them the alternative columns are passive duplicates.
3. **Avg price column:** keep as is (recommended, through Phase 2), retire into Revenue · Sale
   average with a `?cols=` migration heuristic, or re-home its semantics in place. Any change
   moves a default-on number and silently drops the column from explicit `?cols=` bookmarks.
4. **Rows with an unpriced marketable ingredient:** keep at cost 0 with the "n unpriced" note
   (recommended; decide on dropping after the prod row-count delta is measured), or drop them.
   The naive drop empties `require_hq` and world-scope tables.
5. **The 0-priced subcraft rescue:** ship in Phase 2 with a changelog line and the row delta
   recorded (recommended), or leave the pre-existing behaviour.
6. **VWAP as a selectable signal (`sale-vwap`, Cost · VWAP column)?** Recommended: defer. Adding
   it makes `SortMode::RevSignal` non-injective against the existing `vwap` token, and the
   existing VWAP column is require_hq-aware while every revenue signal takes min(NQ, HQ), so a
   "(= Price)" duplicate would not actually equal Price. Revisit with the "assume HQ sale"
   toggle, when one quality rule can be decided for all revenue columns.
7. **K × subcrafts bound:** enforce the cap of two extra cost columns in `needed_signals` and
   measure with the 1a timing (recommended), or memoise sub-recipe runs in 1a (changes
   `compute_cost`'s contract because on-hand consumption makes sub-runs impure).
8. **Scope × signal product columns now, or Hop gain only?** Recommended: Hop gain only; revisit
   after Kosyne validates the workflow.
9. **Ports: label untaxed analyzers with `TaxPolicy::None` first, or adopt the 5% tax in the port
   PR?** Recommended: `None` first (zero-number-change reviews; "− no tax" is visible), with FC's
   tax + sell-world revenue as its own flagged PR.
10. **Phones: keep `hidden md:block` for the new columns (recommended) or adopt the flip
    finder's hscroll table for all optional columns in Phase 2?** Stating honestly that the hop
    answer is desktop-only beats half-porting the mobile table inside this work.

## Relationship to prior work

Builds on the 2026-08-30 market-model spec (Market menu, `?cols=` picker, listing world/DC
columns) and keeps its decisions: sell side is one world, buy scope is the travel model, no
per-ingredient shopping planner. Replaces the static formula string from #1247 with a live one.
Fixes the #1248 follow-up (resource read inside a Memo) by folding the failover into one
resource. Leaves the 2026-08-29 improvements spec's phases 2–3 untouched but lays down the
fields they need. Saddlebag comparison (verified from their open-source frontend and wiki): two
dropdowns, a server round-trip per change, all seven signal columns always shown, red/green
4px pseudo-element bands on desktop only, no formula text, no tax, per-craft profit with a
yields column, travel in a separate paid tool. Nothing here copies those; the formula as a
first-class control, the operator legend, muted alternatives with a delta, the header pill as
the selector, and Hop gain / Worlds to visit have no counterpart in any tool surveyed
(Saddlebag, Teamcraft, itinerare, XIV Profitability, ffxivmb, GilGoblin).
