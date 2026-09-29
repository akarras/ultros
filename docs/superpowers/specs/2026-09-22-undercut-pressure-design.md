# Undercut pressure on the item page

Show flippers how contested an item's market is: when a world's floor is
under attack, when it is just busy, and how long a listing at the floor
survives. Two surfaces share one reducer:

1. **World scope** (`/item/Gilgamesh/35555`): an undercut-pressure pane under
   the price chart plus a row of four stat cards.
2. **Datacenter / region scope** (`/item/Aether/35555`,
   `/item/North-America/35555`): a per-world market board that replaces
   `WorldMarketShare`, grouped by datacenter at region scope.

Undercutting is a per-world phenomenon (retainers only compete with listings on
their own world), so the pressure series is never summed across worlds. The
multi-world scopes get a per-world comparison instead.

Ships as two PRs: phase 1 (world pane + cards), phase 2 (market board).

## Why

`listing_events` has recorded every board change since 2026-09-07 and #1377
turned it into two grid columns (undercuts/day, median undercut %). The item
page still shows only sales and the floor line. Prod has about 810k same-listing
price drops a week plus ~260k remove-and-relist drops a day, so for most traded
items the signal is dense. A flipper needs both halves of the question "if I
buy at the floor now and relist, will it sell before it is undercut, and how
far will the price slide meanwhile?"

## Definitions

All definitions are per world and per the page's quality filter
(`HqFilter::{Any, Nq, Hq}`; `Any` takes both qualities' events and the minimum
of both qualities' floors, as `floor_history::sample` already does).

- **Undercut event.** A same-listing price drop, exactly as `reprice_sql`
  (`ultros-clickhouse/src/listing_history.rs:135`) defines it: an `updated`
  event with `prev_price > price_per_unit`, or a `removed` → `added` pair on
  the same `listing_id` within 600 s at a lower price. Same exclusions
  (`source = 'snapshot'`, empty `listing_id`, raises, same price, `DISTINCT`
  against retried inserts). Each event carries `event_time`, `retainer_id`
  (the retainer that cut), `prev_price`, `price_per_unit`.
- **Trim vs cut.** `drop = (prev_price - price_per_unit) / prev_price`. A
  **trim** is `drop < 0.01` (covers the 1-gil undercut on anything above
  100 gil); a **cut** is `drop >= 0.01`. Prod median drop is 2.6%, so the
  split is meaningful.
- **Floor.** The world's lowest listing price over time from `floor_changes`,
  exact transitions (`load_changes` with `step: None`), carried as-of like
  `sample`. `0` = empty board.
- **Coverage start.** The world's earliest `floor_anchors` row (`anchors()`).
  Before it the pressure series is `Unknown`, never "calm". A world with no
  anchor has no coverage and the endpoint returns an empty series.
- **Floor episode.** A maximal span during which the floor price is constant
  (consecutive equal prices merged). It **ends by undercut** if the next price
  is lower and non-zero, and **ends by leaving** if the next price is higher
  or the board empties (the floor listing was bought, pulled or raised).
  Left-censored episodes (in force at the window start) and right-censored
  ones (still open at `to`) are excluded from lifetime and outcome stats.
- **Baseline.** The median of `trims + cuts` over the window's known buckets:
  the item's own normal level on that world at that bucket width.
- **Bucket state** (known buckets only):
  - **War** when all three hold:
    1. floor erosion `(floor_open - floor_close) / floor_open >= min(0.01 × bucket_hours, 0.10)`
       (`floor_open`/`floor_close` are the as-of floors at the bucket's
       edges, both non-empty);
    2. `trims + cuts >= max(2 × baseline, 3)`;
    3. at least 2 distinct cutting retainers in the bucket (ping-pong).
  - **Calm** when `trims + cuts == 0`, or `baseline >= 2` and
    `trims + cuts < 0.5 × baseline`.
  - **Churn** otherwise.
- **War span.** Maximal run of consecutive `War` buckets. Carries start, end,
  undercut count, distinct sellers, and floor change over the span.
- **Contested share.** Fraction of known buckets in `Churn` or `War`. The UI
  labels it "Always contested" at `>= 0.8`, "Often contested" at `>= 0.3`,
  "Mostly quiet" below.

All thresholds live as named constants in one module
(`ultros-clickhouse/src/undercut_pressure.rs`) with a doc comment saying they
are first guesses. Before merging phase 1, run the reducer against 10 prod
items (5 hot, 5 slow, read-only) and adjust; record the result in the PR.

## Wire types — `ultros-api-types/src/undercut_pressure.rs`

```rust
pub enum PressureState { Unknown, Calm, Churn, War }

pub struct PressureBucket {
    pub start: i64,              // epoch-aligned bucket start
    pub trims: u32,
    pub cuts: u32,
    pub sellers: u16,            // distinct cutting retainers
    pub floor_open: Option<u32>, // None = empty board or unknown
    pub floor_close: Option<u32>,
    pub state: PressureState,
}

pub struct WarSpan { pub start: i64, pub end: i64, pub undercuts: u32, pub sellers: u16, pub floor_change: Option<f64> }

pub enum WarStatus { None, Active, Ended { at: i64 } } // Active: a span touches the last hour; Ended: within 24 h

pub struct PressureSummary {
    pub floor_trend_24h: Option<f64>,   // as-of floor now vs now-24h, fraction
    pub war: WarStatus,
    pub last_war: Option<WarSpan>,      // most recent span within 24 h (card subtitle)
    pub contested_share: Option<f64>,
    pub typical_undercuts_per_hour: Option<f64>, // baseline / bucket_hours
    pub floor_holds_median_secs: Option<i64>,
    pub episodes_left: u32,             // ended by leaving (bought/pulled)
    pub episodes_undercut: u32,
}

pub struct UndercutPressure {
    pub world_id: i32,
    pub from: i64, pub to: i64, pub bucket_seconds: i64,
    pub coverage_from: Option<i64>,
    pub baseline: Option<f64>,
    pub buckets: Vec<PressureBucket>,
    pub wars: Vec<WarSpan>,
    pub summary: PressureSummary,
}

pub struct WorldPressureRow {           // phase 2
    pub world_id: i32,
    pub state_24h: PressureState,       // worst state over the last 24 h: War > Churn > Calm > Unknown
    pub war_sellers: Option<u16>,
    pub floor_holds_median_secs: Option<i64>,
    pub newest_added: Option<i64>,      // newest non-snapshot `added` event, any time in retention
}
pub struct ScopePressure { pub rows: Vec<WorldPressureRow> }
```

All derive `Serialize, Deserialize, Clone, Debug, PartialEq`; new optional
fields get `#[serde(default)]`.

## Backend

### `ultros-clickhouse/src/undercut_pressure.rs`

- `undercut_events(ch, item_id, worlds, hq, from, to) -> Vec<UndercutEvent>`:
  `reprice_sql`'s two branches without the `GROUP BY`, projecting
  `world_id, event_time, retainer_id, prev_price, price_per_unit`
  (`retainer_id` added to both `DISTINCT` subselects; for the pair branch it is
  the `added` row's). Same `LIMITS`. Extract the shared pairing SQL so the
  grid column and this read cannot drift: `reprice_sql` becomes
  `SELECT ... GROUP BY` over the shared event subquery.
- Floor transitions: `load_changes(ch, &[item_id], worlds, ChangeWindow { from: read_from, to, hq, step: None })`
  plus `anchors(ch, worlds)`.
- **Pure reducer** `pressure(events, changes, anchor, from, to, bucket_seconds, now) -> UndercutPressure`
  does all bucketing, state, war spans, episodes and summary. It is the unit
  under test; no ClickHouse types leak into it.
- **Pure reducer** `world_rows(events, changes, anchors, added, now) -> Vec<WorldPressureRow>`
  for phase 2, reusing the same bucket classifier over the last 24 h at 1 h
  buckets.
- `read_from = min(from, now - 86400)` so the 24 h summary is available on any
  chart window; buckets are only emitted for `[from, to)`.
- `floor_trend_24h`, `war` and `last_war` are always computed on their own
  1 h buckets over `[now - 86400, now)`, independent of the chart's bucket
  width, so the cards read the same at every zoom. `contested_share`,
  `typical_undercuts_per_hour` and the episode stats use the chart window.
- Buckets use `start = floor(t / bucket_seconds) * bucket_seconds`, the same
  epoch alignment as `price_series` (`toStartOfInterval`), so pane bars line up
  with the price chart's buckets.

### Endpoints (`ultros/src/web/api/undercut_pressure.rs`)

- `GET /api/v1/undercut_pressure/{world}/{item_id}?hq=&from=&to=&bucket=`
  (phase 1). `world` must resolve to a single world, otherwise **400**
  (`market_pulse.rs` precedent). `bucket` must be a `BUCKET_LADDER` value and
  `(to - from) / bucket <= 2000`, otherwise 400. `from`/`to` parsing and
  clamping match `PriceSeriesQuery`.
- `GET /api/v1/undercut_pressure/scope/{scope}/{item_id}?hq=` (phase 2).
  Accepts world, datacenter or region; one query set covers every world in
  scope (`world_cache.get_all_worlds_in`). `newest_added` is one extra
  `SELECT world_id, max(event_time) ... WHERE kind='added' AND source != 'snapshot' GROUP BY world_id`.
- Both: in-process 60 s cache keyed like `floor_history` (open-window `to`
  quantized with `open_window_cache_stamp`), `Cache-Control: public, max-age=60`,
  a `Semaphore::const_new(4)` + 15 s `tokio::time::timeout` returning
  `TemporarilyUnavailable` (503, not reported), ClickHouse errors as
  `ClickHouseQueryError::new("undercut_pressure", e)`.

## Frontend

### API client (`ultros-frontend-core/src/api.rs`)

`get_undercut_pressure(item_id, world, hq, range, bucket_seconds)` and
`get_scope_pressure(item_id, scope, hq)`, both via `fetch_api`.

### Phase 1: pane + cards (world scope only)

- **Fetch gate** (`routes/item_view.rs` `ChartWrapper`): a `LocalResource`
  that stays `None` unless the scope is a single world, the chart mode is
  Price / Candles / Range, and the price series has loaded (it supplies
  `bucket_seconds`). Same `debounced_decision` range as the floor resource.
  Datacenter/region pages and Density mode make no request.
- **Layout** — new `ultros-charts/src/charts/undercut_pressure.rs`:
  `build_undercut_pressure_chart(&UndercutPressure, sales: &[(i64, u64)], &PressurePaneOptions) -> PressureModel { scene, hover }`.
  Options carry `width`, `time_range`, and the price chart's `plot_left = 68`,
  right margin `16`, so its `TimeScale` matches exactly. Height
  `(width × 0.16).clamp(90, 150)`. Nodes:
  - a 6 px state ribbon at the top (`Rect` per bucket; war red, churn
    neutral, calm green-muted, unknown hatched-looking dim) — batched with
    `rects_path_d` per state;
  - stacked bars (cuts bottom, trims on top), batched per series;
  - a dashed baseline `Line` at `baseline` (`Stroke.dash`);
  - a sales-count `Polyline` from the price series' per-bucket sale counts
    (summed over the series; same unit as the bars, so one y axis);
  - an `Unknown` region before `coverage_from` drawn as a flat dim band with
    no bars.
- **War shading on the price chart**: `PriceChartOptions` gains
  `war_spans: Vec<(i64, i64)>`; `build_price_history_chart` draws them as
  translucent `Rect`s behind the series. Empty by default so every other
  caller and snapshot is unchanged.
- **Rendering** (`ultros-ui-charts/src/components/price_history_chart.rs`):
  `PriceHistoryChart` gains `pressure: Signal<Option<UndercutPressure>>`
  (default `None`). When `Some`, a second `<svg>` renders under the main one
  with the same measured width, plus an HTML legend. Pointer handlers write
  the existing `hover_index`; the pane's hover buckets are keyed by bucket
  start so the shared `HoverTooltip` gains a pressure section: "12 undercuts
  (8 cuts, 4 trims) · 3 sales · War — 3 sellers".
- **Stat cards** (`ultros-ui-charts/src/components/market_history.rs`):
  `MarketHistory` gains `pressure: Signal<Option<UndercutPressure>>`. When
  `Some`, a second `.mh-stats` row renders:
  | Card | Value | Subtitle |
  |---|---|---|
  | Floor trend (24h) | `floor_trend_24h` as ±% | "falling" (< −2%) / "steady" / "rising" (> +2%) |
  | Price war | Active / Ended _n_ h ago / None | sellers + floor change, or the contested label + typical undercuts/hr when none |
  | Floor holds | median episode, humanized | "at the floor, this period" |
  | Sold vs undercut | `left / (left + undercut)` as `62% / 38%` | "how floor listings left" |
  Cards show `—` when their value is `None`.
- **Failure**: an error hides the pane and cards and shows one muted line
  ("Undercut data is temporarily unavailable"); sales and floor are unaffected.

### Phase 2: per-world market board (datacenter / region scope)

Replaces `WorldMarketShare` in place (below the chart, same `Transition` /
hydration guard, still hidden at world scope).

- Columns: World (home icon), Floor, Supply (existing bar + units), Sellers
  (distinct `retainer_id`), Pressure chip (24 h state), Floor holds, Newest
  listing. Floor, supply, sellers come from `filtered_listings` and render
  immediately; the pressure columns fill in when `get_scope_pressure` lands
  (pending cells show a shimmer, failure shows `—`).
- Rows sort by floor ascending (empty boards last). Clicking a row navigates
  to `/item/<World>/<id>` preserving `hq` and `range`.
- **Region scope**: rows grouped by datacenter. The group header shows the
  cheapest world and its floor, total supply and sellers, a strip of one
  square per world coloured by state, and "_n_ at war". Groups sort by floor.
  The home world's datacenter starts expanded (else the cheapest datacenter);
  the rest start collapsed. No inner scroll container.
- **Mobile** (< 640 px): Sellers and Floor holds fold into the row's tooltip.

### i18n

Every label, subtitle, chip, legend entry, tooltip line, aria-label and the
failure line are keys in all seven locale files with real translations,
prefixed `undercut_pressure_*` (pane, cards) and `world_board_*` (board).
`market_share_*` keys that become unused are removed from all seven.

## Tests

- **Reducer (pure, `undercut_pressure.rs`)**: trim/cut split at the 1%
  boundary; epoch bucket alignment; baseline median; each state rule
  including the `bucket_hours` erosion scaling and 10% cap; ping-pong needing
  two sellers; war-span merging and seller dedupe; `Unknown` before the anchor;
  episodes excluding both censored ends; undercut vs leave outcomes including
  empty board; `HqFilter::Any` taking the min of both qualities; 24 h summary
  computed when `from` is older than a day and when it is newer; `WarStatus`
  transitions; `world_rows` worst-state fold.
- **Shared SQL**: the existing `set_undercut_rates` / reprice tests stay green
  after `reprice_sql` is rebuilt on the shared subquery.
- **ClickHouse (gated `ULTROS_CH_INTEGRATION=1`)**: a smoke in
  `tests/undercut_pressure_smoke.rs` seeding one world with an anchor, an
  `updated` drop, a remove→add drop, a raise, a duplicate row, and floor
  changes; asserts bucket counts, sellers, and one closed episode.
- **Wire**: round-trip and old-shape deserialize for every type.
- **Endpoint**: 400 for a datacenter on the world route, bad bucket, too many
  buckets; 404 unknown world.
- **Layout**: snapshot SVGs for a calm item, a hot item with a war, and a
  series with an unknown prefix; structural tests that bars stay in plot
  bounds, the pane's x for a bucket equals the price chart's, and batched node
  counts stay bounded. `war_spans` empty leaves existing snapshots byte-equal.
- **Frontend**: `market_history` summary tests for the card values and `—`
  states; board sort/group/expand-default helpers as pure functions with unit
  tests.
- **E2E**: a fixture-backed probe in `integration/` that loads a world item
  page and asserts the pane and cards render, then a datacenter page and
  asserts the board rows and that no world-route pressure request fired.
- `./check_ci.sh` green before each PR.

## Out of scope

- Summing pressure across worlds on any surface.
- Counting a *new* listing placed below the floor as an undercut event (it
  still shows through floor erosion and episode outcomes).
- Splitting "left" into sold vs pulled via sale matching (a v2 refinement of
  the Sold vs undercut card).
- The crowding band (depth near the floor), the user's-own-retainer overlay,
  and URL-persisted board expansion.
- Alerts driven by war state.

## Risks

- **Thresholds are guesses.** Mitigated by the prod calibration step and
  constants in one place.
- **Short history.** Coverage starts 2026-09-07 at the earliest; long chart
  ranges show a dim unknown prefix and the cards say "this period".
- **Hot items at 1 h buckets over long ranges.** Bounded by the 2000-bucket
  cap and `LIMITS` (`max_result_rows`); events are reduced in Rust, a hot
  item-world is a few thousand rows a week.
- **One extra request on world item pages** (one on multi-world pages for
  the board). Cached 60 s, gated to scope and time-axis modes.
