# Shared analyzer data and column capabilities

Status: implemented across all seven tools; build, CI, and browser validation complete.

## Implementation

`virtual_grid/metrics.rs` supplies typed numeric, text, set, mixed-state, and
missing-data queries. `QueryGrid` applies those queries to the complete candidate
set before virtualization, persists them in `gf`, and exposes hidden active
filters and coverage. Incomplete feeds can filter known rows but cannot claim a
global sort. Failed feeds remain unknown rather than counting as confirmed
missing history. Named views preserve query, pricing, sorting, and layout state.

`analyzer_kit/market.rs` supplies reusable market subjects, price controls,
seven-day bulk statistics, optional thirty-day statistics, and lazy world trend
providers to the six non-Recipe adapters. Recipe retains its richer column
registry and custom travel provider through the generalized analyzer grid;
its Labs capabilities are now available without a Labs toggle. The obsolete
toggle is removed while its stored token remains readable.

All seven adapters support the shared price bases and typed column queries.
Venture, Leve, FC Crafting, and Scrip Sources no longer truncate results. Scrip
Sources names its largest-cost ingredient as the market subject. Flip Finder
retains its conservative default pricing and existing saved-view menu, with
explicit alternative sale-price inputs. Median inputs awaiting statistics do
not prematurely exclude candidates through financial filters.

The deterministic browser fixture contains 250 rows, explicit partial-data
transitions, and missing/failed/pending cases. Its regression suite covers rows
beyond the former cap, hidden filters, saved views, both sort directions, and
desktop/mobile behavior. See the final validation record below for executed
checks and environment limitations.

## Required outcome

All seven tools participate: Flip Finder, Recipe, Venture, Leve, FC Crafting,
Vendor Resale, and Scrip Sources. Sharing the viewport alone is insufficient.
They share market definitions, price-signal support, column queries, and an
extension interface for tool-specific calculations.

1. Preserve VirtualGrid's two-axis scrolling, frozen headings, column insertion,
   hiding, reordering, resizing, auto-fit, keyboard/touch controls, and saved
   views. Existing URLs and saved layouts remain readable.
2. Every data column declares its sorting and filtering capabilities. Support
   numeric bounds, text matching, categorical choices, and missing-data queries
   where meaningful. Action buttons and a sparkline graphic are not scalar data.
3. Every analyzer supports the Labs seven-day sale minimum, median, and average
   alongside current listings, both as applicable comparison columns and as
   inputs to its market-dependent calculations.
4. Every analyzer can expose relevant world, datacenter, trend, velocity/cadence,
   and sales/day context. Unmarketable outputs must not acquire fictional sale
   statistics; a cost-only tool can expose ingredient context instead.
5. A tool can register additional metrics and their data requirements through
   the same interface. World-hop savings and worlds-to-visit are the first
   examples, not special cases built into VirtualGrid.

## Research baseline (before this implementation)

`analyzer_kit/grid.rs` already renders Recipe Labs through QueryGrid/VirtualGrid.
The other six tools also use QueryGrid. The shared adapter still hardcodes the
Recipe ID/title, and most tools define their own columns and pricing policies.

Recipe registers 31 columns, including Actions. Flip Finder registers 15.
Recipe has eight alternative price/cost columns, travel comparisons, seven-day
statistics, optional thirty-day statistics, and a visible-window trend feed.
Flip Finder calculates an internal median from a buffer of up to six recent
sales, but exposes neither that median nor the estimated sale price as separate
columns. Its thirty-day enrichment supplies additional market context.

`GET /api/v1/sale_stats/{worldDcOrRegion}?window=N` already supports 1, 7, 30,
and 90 days. Its rows are keyed by item and quality and include minimum,
approximate median, arithmetic mean, sale count, units sold, VWAP, last sold,
sales/day, and world-only confidence. Wider scopes merge median aggregate
states; do not calculate a regional median by averaging world medians.

## Metric semantics

Every market metric identifies the item or ingredient, quality policy, market
scope, time window, unit, source, and availability. Labels and header details
must make these distinctions understandable without inspecting the URL.

| Metric | Shared definition |
| --- | --- |
| Sale median (7d) | Seven-day approximate median per-unit sale price from the bulk statistics API |
| Sale minimum (7d) | Lowest per-unit sale price in that same window |
| Sale average (7d) | Arithmetic mean per-unit sale price, weighted by sale events |
| VWAP (7d / 30d) | Total gil traded divided by units traded in the specified window |
| Sales (7d / 30d) | Sale-event count; distinct from units traded |
| Units sold (7d / 30d) | Sum of traded quantities |
| Sales/day (7d / 30d) | Sale-event count divided by the specified window length |
| Velocity/cadence | A presentation of an identified sales rate and sample count; avoid an unexplained competing rate |
| Profit/day | The tool's profit model and an explicitly identified matching rate and quantity basis |
| World / datacenter | An identified listing's location; an aggregate median has no single listing world |
| Trend | An identified price series with its market, quality, and window |

Resolve the existing differences deliberately:

- Flip Finder's "30d Volume" is currently a cleaned sale sample count; Recipe's
  volumes count units. Retain saved column IDs while making labels truthful.
- Flip Finder's displayed Sales/day can use thirty-day enrichment while its
  Profit/day uses buffer velocity. The selected rate and the calculation must
  agree after migration.
- Recipe's default daily-sales/average summary combines qualities, while its
  price comparisons and lazy context use a selected quality. Never compare an
  HQ price with an NQ median or silently change quality under a named metric.
- Recipe's alternative revenue signals choose the cheaper available quality;
  make that policy explicit rather than treating it as an exact-quality lookup.
- Existing Recipe drift uses an hourly series; Flip Finder drift uses recent
  sale samples. Preserve or migrate their identities explicitly.

Raw statistics remain raw: missing history displays as missing. If a selected
calculation falls back to a listing, carry that provenance separately and show
it. Loading, unavailable, missing, and a genuine zero are distinct states.

## Sorting and filtering

The restriction is data coverage, not ClickHouse:

| Data coverage | Sorting and filtering |
| --- | --- |
| Present or computed for every candidate row | Supported |
| Whole-scope statistics fetched for every candidate row | Supported, including ClickHouse data |
| Whole-scope statistics still loading | Keep the query pending; do not evaluate missing temporary values as zero |
| Visible-window enrichment only | Filtering supported on loaded values with an explicit partial-coverage notice; global sorting remains unsupported |
| Presentation-only/action column | Unsupported unless it declares a meaningful query value |

A graph can expose a separate scalar such as price change over seven days.
Partial-coverage filtering on that scalar is acceptable; a full-result/server
query is not a prerequisite for offering the filter. Fetching a few on-screen
sparklines cannot establish which unseen rows match, and the UI must say so.

Use Flip Finder's existing transparency pattern. Its results summary says
"N rows lack data for these filters". Its volume filter retains unknown rows and
rejects only known values below the threshold; confidence can use a disclosed
derived fallback. Data accumulates as the user scrolls, so filtering applies to
all loaded values, including rows that have since left the viewport, rather than
only to currently mounted cells.

Make this behavior reusable and explicit in each column's capabilities:

- Keep rows awaiting enrichment eligible so they can enter the viewport and
  request their data. Do not filter the initial table to zero and thereby prevent
  enrichment from ever running.
- Reevaluate rows when values arrive. Show partial coverage and the count of
  rows that cannot be fully evaluated; do not describe retained unknowns as
  verified matches. Document each column's fallback/missing-data policy.
- Distinguish data not fetched yet from a completed request with no history or
  a failed request, while retaining the coverage notice wherever the predicate
  cannot be evaluated. Do not infer full coverage merely from scrolling.
- Keep query eligibility and fetch bookkeeping independent of rendered cells,
  preventing filtering/refetch loops and preserving loaded values offscreen.
- Treat sorting separately: partial-coverage filtering does not authorize a
  misleading global ranking based on whichever rows have been loaded so far.

Queries use typed raw values, never parsed display strings. Missing values have
explicit filter behavior and deterministic placement under either sort
direction. Hidden columns can remain active query targets and must still request
their data. Active filters must remain discoverable and clearable after hiding
their columns. Saved views capture filters, selected price bases, sort, and layout.

Do not truncate analyzer candidate or result sets. Venture, Leve, and FC Crafting
currently truncate to 100; Scrip Sources also has a cap. Remove these limits while
retaining eligibility, pricing-coverage rules, and stable row identities. The
grid virtualizes the DOM, not the available results. Request batch limits and
visible-window enrichment remain valid and must not cap the underlying dataset.
Result counts describe the retained results, with a coverage notice when some
rows cannot yet be evaluated against an active filter.

## Reusable interface

Keep three responsibilities separate:

- VirtualGrid: geometry, virtualization, focus, and column interactions.
- Shared analyzer layer: column definitions, typed values, capabilities, source
  requirements, query execution, price-signal selection, and URL persistence.
- Tool adapter: row identity, relevant market subjects, cost/revenue rules,
  quantities, and additional calculated metrics.

A column definition provides a stable ID, label/context, value type, formatter,
measurement/rendering support, query-value extractor, available operators, data
coverage, and dependencies. Custom renderers must not lose sorting/filtering
merely because their visual presentation differs from an ordinary number.

A dependency provider identifies the scope/window/quality and returns keyed data
with explicit loading and failure states. Reuse requests with identical keys;
discard stale responses after changing markets. Fetch for visible columns,
active sorts/filters, and selected formula inputs, rather than fetching every
possible feed on every tool. Reuse the existing bulk-statistics cache and
visible-window enrichment mechanism.

World hops register their required home/scope cost runs and listing locations.
The provider returns numeric savings, world/DC sets, and explicit "trip needed"
or unavailable states. Those states must not be flattened into zero savings.
Other tools can use the same extension contract for their own quantities or
resource-efficiency metrics.

## Tool-specific application

| Tool | Selectable market inputs | Fixed/tool-owned behavior |
| --- | --- | --- |
| Recipe | Ingredient costs and output revenue | Yield, subcrafts, on-hand items, tax, travel comparisons |
| Flip Finder | Sale estimate; buy-side comparisons against actual listings | Current conservative estimator remains a selectable compatibility policy; quality and suspicious-price guards remain explicit |
| Venture | Returned items' market value | Venture quantity and currency costs |
| Leve | Turn-in acquisition and market-valued item rewards | Gil rewards, reward probabilities, turn-in quantities |
| FC Crafting | Ingredient costs and completed-item value | Project quantities, on-hand inputs, tool tax policy |
| Vendor Resale | Market resale value | Vendor purchase price |
| Scrip Sources | Ingredient costs | Scrip type and reward amount; unmarketable collectables do not have output sale history |

For cost-only or unmarketable-output rows, support inspecting the relevant
ingredients' market context. Do not combine unrelated ingredient prices into a
single unlabeled output median or trend. World/DC sets need set-membership
filters rather than an arbitrary single location.

## Delivery and acceptance

1. Generalize column/query capabilities and the analyzer adapter while retaining
   existing VirtualGrid interactions and URL contracts.
2. Make Recipe the reference implementation, including custom travel providers,
   typed filters, and explicit data availability.
3. Migrate Flip Finder onto the same definitions, preserving its default pricing
   policy and saved views while exposing selectable seven-day prices.
4. Migrate the remaining five tools with their market-input adapters. The work
   is complete only when all seven satisfy the contract.

Validate with deterministic fixtures covering NQ/HQ separation, wider scopes,
missing history, outliers, loading/failure, stale responses, quantity/yield math,
and custom metric states. Query tests must include matching rows beyond the old
100-row cap and outside the viewport, hidden query columns, and both sort
directions. Verify same-source definitions agree across tools. Partial-coverage
tests must verify initial unknown rows remain available, loaded failures are
removed, offscreen loaded rows stay filtered, missing/failed feeds retain honest
coverage counts, and filtering does not deadlock enrichment. Verify all eligible
results remain reachable without a fixed row cap.

Browser checks cover desktop/mobile grid interactions, direct SSR+hydrate versus
client navigation, filter and pricing changes, saved-view reloads, old URL/layout
compatibility, and world changes while enrichment is pending. Run check_ci.sh,
cargo leptos build, local JavaScript regression tests, and the required E2E suite
before shipping. Add a player-facing changelog entry with the implementation.

## Validation record

- Final `cargo leptos build --bin-features ''`: passed for WASM and native server.
  The empty feature override is local to Windows validation: the repository's
  default optional jemalloc dependency does not build with MSVC. Production
  feature configuration is unchanged.
- `check_ci.sh`: passed on the final source, including formatting, both Clippy
  stages, 1,644 passing Rust tests, and the game-data pack sanity check. Thirteen
  existing ignored tests remain ignored; six live Universalis smoke tests are
  excluded by the deterministic gate as documented in `scripts/check_tests.sh`.
- The existing JavaScript regression suite passed (75 tests).
- Browser validation used this worktree's fresh server and isolated
  PostgreSQL/ClickHouse containers. The existing virtual-grid suite, analyzer
  grid suite, and broad desktop/mobile/wide route checks passed, as did item
  layout, FC breakdown, recipe planner, and dashboard checks.
- The complete shared-data browser suite passed with
  `CHECK_ANALYZER_ROUTES=1`: 250-row virtualization/filtering, hidden filters,
  named views, partial/missing/failed states, SSR/hydration, mobile rendering,
  and all seven populated adapters. Verified market fixtures use distinct
  NQ/HQ medians and assert calculation changes after selecting median prices.
  Final adapter row counts were Flip 54, Recipe 39, Venture 26, Leve 28,
  FC Crafting 2, Vendor Resale 21, and Scrip Sources 1,116.
- The initial `scripts/run_e2e.sh` invocation returned nonzero solely because
  its dashboard fixture expected history for an unseeded item. Adding that
  item to the isolated database and rerunning the dashboard passed all eight
  checks; every other driver suite had already passed. No production code
  changes were needed for the browser runs. Logs and screenshots are preserved
  under `target/` and `integration/artifacts/`; the owned test services were
  stopped afterward.
