# Undercut Pressure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Phase 1 — on world-scope item pages, show an undercut-pressure pane under the price chart (state ribbon, stacked cut/trim bars, baseline, sales line, war shading) plus four stat cards (floor trend 24h, price war, floor holds, sold vs undercut). Phase 2 (outlined at the end, separate PR) replaces `WorldMarketShare` with a per-world market board.

**Architecture:** A pure Rust reducer in `ultros-clickhouse` turns one item-world's same-listing price drops (`listing_events`, via SQL shared with the #1377 grid columns) and exact floor transitions (`floor_changes`) into buckets, states, war spans and a summary. A world-only axum endpoint serves it with the floor-history caching pattern. The frontend fetches it only at world scope in time-axis modes, renders a second SVG scene built by a new `ultros-charts` layout that shares the price chart's time domain and hover index, and renders the cards as a child of `MarketHistory`.

**Tech Stack:** Rust (edition 2024), axum, ClickHouse (`clickhouse` crate), Leptos 0.8 + leptos-i18n, the in-house `ultros-charts` scene graph, Puppeteer E2E (`integration/`).

**Spec:** `docs/superpowers/specs/2026-09-22-undercut-pressure-design.md`

## Global Constraints

- Work in worktree `C:\Users\chw11\code\ultros\.claude\worktrees\adoring-heyrovsky-a5002b`, branch `claude/undercut-pressure`. Before the first commit of a session run `git rev-parse --show-toplevel` and confirm it prints that path (this worktree vanished once mid-session).
- Every user-facing string goes through leptos-i18n (`t!` / `t_string!`), with the key in **all seven** locale files `ultros-frontend/ultros-i18n/locales/{en,fr,de,ja,cn,ko,tc}.json` and a real translation in each. Interpolation syntax is `{{name}}`.
- Undercut definition = `reprice_sql`'s (same-listing price drop, `updated` or `removed`→`added` within 600 s, `DISTINCT`, `source != 'snapshot'`, non-empty `listing_id` for pairs). Trim = drop `< 0.01` of the previous price; cut = `>= 0.01`.
- War bucket = erosion `>= min(0.01 × bucket_hours, 0.10)` AND `trims + cuts >= max(2 × baseline, 3)` AND `>= 2` distinct cutting retainers. Calm = 0 undercuts, or `baseline >= 2` and total `< 0.5 × baseline`. Churn otherwise. Unknown before the world's anchor.
- Pane and cards render only at **world** scope and only in Price / Candles / Range modes; no request otherwise.
- The pane's horizontal geometry must equal the price chart's: `margin_left = 68.0`, `margin_right = 16.0`, same time domain.
- Endpoint: `GET /api/v1/undercut_pressure/{world}/{item_id}?hq=&from=&to=&bucket=`; non-world scope → 400; ≤ 2000 buckets (widen along `BUCKET_LADDER`); 60 s cache; semaphore of 4 + 15 s timeout → 503.
- Windows build env (see CLAUDE.md): Strawberry Perl first on PATH and `OPENSSL_RUST_USE_NASM=0` for any cargo build that rebuilds `openssl-sys`; use `CARGO_PROFILE_DEV_DEBUG=0` from the first build of `ultros` tests (4 GiB rlib link limit).
- `market_history.rs` is not modified (a parallel session is moving its strings to i18n); cards render as a `MarketHistory` child and reuse `.mh-stats` / `.mh-stat`.
- Before every commit that touches Rust: `cargo fmt --all`. Before the PR: `./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"` and read the log (never trust a piped exit code). `$SCRATCH` = the session scratchpad, never `/tmp`.

## Review Focus

1. **HQ toggle.** `hq=hq` must count only HQ undercuts and only the HQ floor; `hq=any` must take the cheaper of the two qualities' floors. Pinned by `hq_any_takes_min_of_qualities` (Task 3) and `hq_filter_restricts_events_sql` (Task 5).
2. **Chart window entirely before tracking started** (e.g. `?range=1y` on a slow item, or a slicer drag into August 2026). Every bucket must be `Unknown`, no bars, cards `—`, no panic on empty medians. Pinned by `window_before_anchor_is_all_unknown` (Task 3) and `no_anchor_means_no_coverage_anywhere` (Task 4).
3. **Hot item on a wide window with a small requested bucket.** The server must widen the bucket rather than 400 or emit 10k buckets. Pinned by `fit_pressure_bucket_widens_past_cap` (Task 6).
4. **A bucket the price chart has no sales in.** The pane must still draw bars there and the sales line must read 0, not skip the point. Pinned by `sales_line_zero_fills_missing_buckets` (Task 8).
5. **Navigating world → datacenter on the same item.** The stale world pressure must disappear and no pressure request may fire for the DC scope. Pinned by the E2E probe's second leg (Task 11).

---

## File Structure

| File | Responsibility |
|---|---|
| `ultros-api-types/src/undercut_pressure.rs` (create) | Wire types for the endpoint (phase 1) and the scope board (phase 2). |
| `ultros-clickhouse/src/listing_history.rs` (modify) | Extract `reprice_events_sql` shared by the grid columns and the pane; `LIMITS` → `pub(crate)`. |
| `ultros-clickhouse/src/floor_history.rs` (modify) | `pub(crate) fn exact_changes` (exact transitions with an `HqFilter`). |
| `ultros-clickhouse/src/undercut_pressure.rs` (create) | Constants, floor timeline, bucket classifier, war spans, episodes, summary (pure), and `load`. |
| `ultros-clickhouse/tests/undercut_pressure_smoke.rs` (create) | Gated ClickHouse integration smoke. |
| `ultros/src/web.rs` (modify) | `PressureQuery`, `fit_pressure_bucket`, `undercut_pressure` handler, route. |
| `ultros-frontend/ultros-frontend-core/src/api.rs` (modify) | `get_undercut_pressure`. |
| `ultros-frontend/ultros-charts/src/charts/price_history.rs` (modify) | `PriceChartModel.time_domain`, `HoverBucket.ts`, `PriceChartOptions.war_spans`. |
| `ultros-frontend/ultros-charts/src/charts/undercut_pressure.rs` (create) | Pane layout → `Scene`. |
| `ultros-frontend/ultros-ui-charts/src/components/undercut_pressure.rs` (create) | `UndercutPressurePane`, `UndercutPressureCards`, `pressure_bucket_at`, formatting helpers. |
| `ultros-frontend/ultros-ui-charts/src/components/undercut_pressure.css` (create) | Pane/cards styles and `--mh-*` pressure tokens. |
| `ultros-frontend/ultros-ui-charts/src/components/price_history_chart.rs` (modify) | `pressure` prop, pane under the main svg, tooltip lines, war spans, color tokens. |
| `ultros-frontend/ultros-app/src/routes/item_view.rs` (modify) | Fetch gate, pass `pressure` to chart and cards. |
| `ultros-frontend/ultros-i18n/locales/*.json` (modify ×7) | `undercut_pressure_*` keys. |
| `integration/undercut-pressure.cjs` (create), `integration/package.json`, `scripts/run_e2e.sh` (modify) | E2E probe. |

---

### Task 1: Wire types

**Files:**
- Create: `ultros-api-types/src/undercut_pressure.rs`
- Modify: `ultros-api-types/src/lib.rs` (add `pub mod undercut_pressure;` after `pub mod trends;`)

**Interfaces:**
- Produces: `PressureState { Unknown, Calm, Churn, War }`, `PressureBucket`, `WarSpan`, `WarStatus { None, Active, Ended { at } }`, `PressureSummary`, `UndercutPressure`, `WorldPressureRow`, `ScopePressure` exactly as below. All later tasks use these names.

- [ ] **Step 1: Write the types and tests**

```rust
//! Undercut pressure for one item on one world (item page pane and cards),
//! plus the per-world rows the datacenter/region market board shows.
use serde::{Deserialize, Serialize};

/// How contested a bucket was. `Unknown` = before the world's floor anchor,
/// where Ultros cannot tell a quiet board from an untracked one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PressureState {
    #[default]
    Unknown,
    Calm,
    Churn,
    War,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PressureBucket {
    /// Epoch-aligned bucket start (unix seconds), same alignment as `PriceSeries`.
    pub start: i64,
    /// Same-listing drops below 1% of the previous price.
    pub trims: u32,
    /// Same-listing drops of 1% or more.
    pub cuts: u32,
    /// Distinct retainers that cut in this bucket.
    pub sellers: u16,
    /// As-of floor at the bucket start / last second. `None` = empty or unknown.
    pub floor_open: Option<u32>,
    pub floor_close: Option<u32>,
    pub state: PressureState,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WarSpan {
    pub start: i64,
    /// Exclusive end (last war bucket's start + bucket width).
    pub end: i64,
    pub undercuts: u32,
    pub sellers: u16,
    /// Floor change over the span as a fraction (−0.18 = fell 18%).
    pub floor_change: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum WarStatus {
    #[default]
    None,
    Active,
    Ended { at: i64 },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PressureSummary {
    /// As-of floor now vs 24 h ago, as a fraction.
    #[serde(default)]
    pub floor_trend_24h: Option<f64>,
    #[serde(default)]
    pub war: WarStatus,
    /// Most recent war span in the last 24 h.
    #[serde(default)]
    pub last_war: Option<WarSpan>,
    /// Share of the chart window's known buckets in Churn or War.
    #[serde(default)]
    pub contested_share: Option<f64>,
    #[serde(default)]
    pub typical_undercuts_per_hour: Option<f64>,
    /// Median life of a floor price that started and ended in the window.
    #[serde(default)]
    pub floor_holds_median_secs: Option<i64>,
    /// Floor episodes that ended because the floor listing left (bought, pulled or raised).
    #[serde(default)]
    pub episodes_left: u32,
    /// Floor episodes that ended because a cheaper listing took the floor.
    #[serde(default)]
    pub episodes_undercut: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UndercutPressure {
    pub world_id: i32,
    pub from: i64,
    pub to: i64,
    pub bucket_seconds: i64,
    /// The world's floor anchor; buckets before it are `Unknown`.
    #[serde(default)]
    pub coverage_from: Option<i64>,
    /// Median undercuts per known bucket in the window.
    #[serde(default)]
    pub baseline: Option<f64>,
    pub buckets: Vec<PressureBucket>,
    #[serde(default)]
    pub wars: Vec<WarSpan>,
    #[serde(default)]
    pub summary: PressureSummary,
}

/// Phase 2: one row of the datacenter/region market board.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldPressureRow {
    pub world_id: i32,
    /// Worst state over the last 24 h: War > Churn > Calm > Unknown.
    pub state_24h: PressureState,
    #[serde(default)]
    pub war_sellers: Option<u16>,
    #[serde(default)]
    pub floor_holds_median_secs: Option<i64>,
    /// Newest non-snapshot `added` event (unix seconds).
    #[serde(default)]
    pub newest_added: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScopePressure {
    pub rows: Vec<WorldPressureRow>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> UndercutPressure {
        UndercutPressure {
            world_id: 34,
            from: 0,
            to: 7200,
            bucket_seconds: 3600,
            coverage_from: Some(0),
            baseline: Some(1.0),
            buckets: vec![PressureBucket {
                start: 0,
                trims: 1,
                cuts: 3,
                sellers: 2,
                floor_open: Some(1000),
                floor_close: Some(900),
                state: PressureState::War,
            }],
            wars: vec![WarSpan { start: 0, end: 3600, undercuts: 4, sellers: 2, floor_change: Some(-0.1) }],
            summary: PressureSummary {
                war: WarStatus::Ended { at: 3600 },
                episodes_left: 2,
                ..Default::default()
            },
        }
    }

    #[test]
    fn round_trips() {
        let value = sample();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<UndercutPressure>(&json).unwrap(), value);
        let row = WorldPressureRow { world_id: 1, state_24h: PressureState::Churn, war_sellers: None, floor_holds_median_secs: Some(60), newest_added: Some(5) };
        let json = serde_json::to_string(&ScopePressure { rows: vec![row.clone()] }).unwrap();
        assert_eq!(serde_json::from_str::<ScopePressure>(&json).unwrap().rows, vec![row]);
    }

    #[test]
    fn war_status_is_tagged_snake_case() {
        assert_eq!(serde_json::to_string(&WarStatus::Ended { at: 5 }).unwrap(), r#"{"kind":"ended","at":5}"#);
        assert_eq!(serde_json::to_string(&WarStatus::Active).unwrap(), r#"{"kind":"active"}"#);
    }

    #[test]
    fn minimal_body_deserializes_with_defaults() {
        let body = r#"{"world_id":1,"from":0,"to":1,"bucket_seconds":3600,"buckets":[]}"#;
        let value: UndercutPressure = serde_json::from_str(body).unwrap();
        assert_eq!(value.summary, PressureSummary::default());
        assert!(value.wars.is_empty() && value.coverage_from.is_none());
    }

    #[test]
    fn states_order_by_severity() {
        assert!(PressureState::War > PressureState::Churn);
        assert!(PressureState::Churn > PressureState::Calm);
        assert!(PressureState::Calm > PressureState::Unknown);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ultros-api-types undercut_pressure`
Expected: 4 passed. (If `serde_json` is not a dev-dependency of `ultros-api-types`, copy the setup the `floor_history.rs` tests use.)

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add ultros-api-types/src/undercut_pressure.rs ultros-api-types/src/lib.rs
git commit -m "feat(api-types): undercut pressure wire types"
```

---

### Task 2: Share the reprice SQL

**Files:**
- Modify: `ultros-clickhouse/src/listing_history.rs:14` (`LIMITS` → `pub(crate) const`), `:125-159` (`reprice_sql`)

**Interfaces:**
- Produces: `pub(crate) fn reprice_events_sql(item_sql: &str, world_sql: &str, from: i64, to: i64) -> String` — a subquery (no `SETTINGS`) yielding columns `item_id, hq, world_id, event_time, retainer_id, prev_price, price_per_unit`, one row per undercut event. `pub(crate) const LIMITS: &str`.

- [ ] **Step 1: Write the failing test** (append to the `tests` module in `listing_history.rs`)

```rust
    #[test]
    fn reprice_columns_and_pane_share_one_event_query() {
        let events = reprice_events_sql("7", "34", 1000, 2000);
        assert!(!events.contains("SETTINGS"), "subquery must stay composable");
        assert!(events.contains("retainer_id"));
        assert!(events.contains("toDateTime(400)"), "pair branch reads 600 s before from");
        let grouped = reprice_sql("7", "34", 1000, 2000);
        assert!(grouped.contains(&events), "grid column must aggregate the shared events");
        assert!(grouped.ends_with(LIMITS));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ultros-clickhouse reprice_columns_and_pane_share_one_event_query`
Expected: FAIL — `cannot find function reprice_events_sql`.

- [ ] **Step 3: Implement** — replace `reprice_sql` and its doc comment (lines 125-159) with:

```rust
/// Every same-listing price drop in `[from, to)`, one row per event, paired
/// in ClickHouse so no listing id string reaches Rust. Shared by the grid's
/// undercut columns and the item page's pressure pane so they cannot drift.
/// `DISTINCT` mirrors the events read: a retried writer batch stores every
/// row twice. The first row of a partition has no predecessor; `lagInFrame`
/// yields the type default (0) and the `prev_removed = 1` test rejects it.
/// `event_time` has only second resolution and a remove-then-add reprice
/// commonly arrives as two websocket messages within the same second, so the
/// window orders by `event_time, kind = 'added'` to break same-second ties
/// with `removed` first — without it, ties are undefined and a pair can
/// silently sort `added` before `removed` and get dropped. For a pair,
/// `retainer_id` is the `added` row's (the retainer that cut).
pub(crate) fn reprice_events_sql(item_sql: &str, world_sql: &str, from: i64, to: i64) -> String {
    format!(
        "SELECT item_id, hq, world_id, event_time, retainer_id, prev_price, price_per_unit
        FROM (SELECT DISTINCT item_id, hq, world_id, listing_id, retainer_id, event_time, price_per_unit, prev_price FROM listing_events
              WHERE kind = 'updated' AND source != 'snapshot' AND item_id IN ({item_sql}) AND world_id IN ({world_sql})
                AND event_time >= toDateTime({from}) AND event_time < toDateTime({to}))
        WHERE prev_price > price_per_unit
        UNION ALL
        SELECT item_id, hq, world_id, event_time, retainer_id, prev_price, price_per_unit
        FROM (SELECT item_id, hq, world_id, kind, event_time, retainer_id, price_per_unit,
                     lagInFrame(kind = 'removed') OVER w AS prev_removed,
                     lagInFrame(price_per_unit) OVER w AS prev_price,
                     lagInFrame(event_time) OVER w AS prev_time
              FROM (SELECT DISTINCT item_id, hq, world_id, listing_id, retainer_id, kind, event_time, price_per_unit FROM listing_events
                    WHERE kind IN ('removed', 'added') AND source != 'snapshot' AND listing_id != ''
                      AND item_id IN ({item_sql}) AND world_id IN ({world_sql})
                      AND event_time >= toDateTime({}) AND event_time < toDateTime({to}))
              WINDOW w AS (PARTITION BY item_id, hq, world_id, listing_id ORDER BY event_time, kind = 'added' ROWS BETWEEN 1 PRECEDING AND CURRENT ROW))
        WHERE kind = 'added' AND prev_removed = 1 AND event_time >= toDateTime({from})
          AND dateDiff('second', prev_time, event_time) <= 600 AND prev_price > price_per_unit",
        from - 600
    )
}

fn reprice_sql(item_sql: &str, world_sql: &str, from: i64, to: i64) -> String {
    format!(
        "SELECT item_id, hq, count() AS undercuts,
        quantileExact(0.5)((prev_price - price_per_unit) / prev_price) AS undercut_median
        FROM ({}) GROUP BY item_id, hq{LIMITS}",
        reprice_events_sql(item_sql, world_sql, from, to)
    )
}
```

Change line 14 to `pub(crate) const LIMITS: &str = ...` (value unchanged).

- [ ] **Step 4: Run the crate's tests**

Run: `cargo test -p ultros-clickhouse`
Expected: all pass. If a disposable loopback ClickHouse is available (`ULTROS_CH_INTEGRATION=1`, `CLICKHOUSE_URL=http://127.0.0.1:…`, `CLICKHOUSE_DATABASE=ultros_t12_…`), also run `cargo test -p ultros-clickhouse --test listing_history_smoke` — it runs `reprice_sql` against real SQL. If not available, record that for the PR.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-clickhouse/src/listing_history.rs
git commit -m "refactor(clickhouse): share undercut event SQL between grid columns and item pane"
```

---

### Task 3: Pressure reducer — floor timeline, buckets, states, wars

**Files:**
- Create: `ultros-clickhouse/src/undercut_pressure.rs`
- Modify: `ultros-clickhouse/src/lib.rs` (add `pub mod undercut_pressure;` after `pub mod rows;`)

**Interfaces:**
- Consumes: `crate::floor_history::WindowChange { item_id, hq: u8, world_id, timestamp: i64, price: u32 }` (`pub(crate)`, pub fields); Task 1 types.
- Produces (used by Tasks 4-5):
  - `pub struct UndercutEvent { pub time: i64, pub retainer_id: i32, pub prev_price: u32, pub price: u32 }` (derives `Row, Deserialize, Clone, Debug, PartialEq`)
  - `pub(crate) fn floor_points(rows: &[WindowChange], anchor: Option<i64>, start: i64, end: i64) -> Vec<(i64, Option<u32>)>`
  - `pub(crate) fn floor_at(points: &[(i64, Option<u32>)], t: i64) -> Option<u32>`
  - `pub(crate) fn median(values: Vec<f64>) -> Option<f64>`
  - `pub(crate) struct Classified { pub buckets: Vec<PressureBucket>, pub sellers: Vec<HashSet<i32>>, pub baseline: Option<f64>, pub bucket_seconds: i64 }`
  - `pub(crate) fn classify(events: &[UndercutEvent], points: &[(i64, Option<u32>)], anchor: Option<i64>, from: i64, to: i64, bucket_seconds: i64) -> Classified` (`events` sorted by `time`)
  - `pub(crate) fn state_for(bucket: &PressureBucket, baseline: f64, bucket_seconds: i64) -> PressureState`
  - `pub(crate) fn war_spans(c: &Classified) -> Vec<WarSpan>`
  - constants `TRIM_FRACTION, WAR_EROSION_PER_HOUR, WAR_EROSION_CAP, WAR_BASELINE_MULTIPLE, WAR_MIN_UNDERCUTS, WAR_MIN_SELLERS, CALM_BASELINE_SHARE, CALM_MIN_BASELINE`, `pub(crate) const HOUR, DAY`

- [ ] **Step 1: Write the module header and failing tests**

```rust
//! Undercut pressure for one item on one world. Pure reducers turn
//! same-listing price drops and exact floor transitions into buckets,
//! calm/churn/war states, war spans and floor-episode stats; `load` does the
//! two bounded ClickHouse reads. Spec:
//! docs/superpowers/specs/2026-09-22-undercut-pressure-design.md
use crate::floor_history::WindowChange;
use clickhouse::Row;
use serde::Deserialize;
use std::collections::HashSet;
use ultros_api_types::undercut_pressure::{PressureBucket, PressureState, WarSpan};

// Thresholds are first guesses, calibrated against prod before phase 1
// merged (see the PR). Keep them together so tuning is a one-file change.
/// A drop below this share of the previous price is a trim (covers the
/// 1-gil undercut on anything above 100 gil); at or above it, a cut.
pub const TRIM_FRACTION: f64 = 0.01;
/// Floor erosion a war bucket needs, per hour of bucket length...
pub const WAR_EROSION_PER_HOUR: f64 = 0.01;
/// ...capped for day-and-longer buckets.
pub const WAR_EROSION_CAP: f64 = 0.10;
/// A war bucket needs this multiple of the baseline...
pub const WAR_BASELINE_MULTIPLE: f64 = 2.0;
/// ...and at least this many undercuts...
pub const WAR_MIN_UNDERCUTS: u32 = 3;
/// ...from at least this many distinct retainers (ping-pong).
pub const WAR_MIN_SELLERS: u16 = 2;
/// A busy item's bucket below this share of its baseline reads as calm...
pub const CALM_BASELINE_SHARE: f64 = 0.5;
/// ...once the baseline is at least this high.
pub const CALM_MIN_BASELINE: f64 = 2.0;
pub(crate) const HOUR: i64 = 3600;
pub(crate) const DAY: i64 = 86_400;

#[derive(Clone, Debug, PartialEq, Row, Deserialize)]
pub struct UndercutEvent {
    pub time: i64,
    pub retainer_id: i32,
    pub prev_price: u32,
    pub price: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use PressureState::*;

    fn ev(time: i64, retainer_id: i32, prev_price: u32, price: u32) -> UndercutEvent {
        UndercutEvent { time, retainer_id, prev_price, price }
    }
    fn fc(timestamp: i64, hq: u8, price: u32) -> WindowChange {
        WindowChange { item_id: 1, hq, world_id: 34, timestamp, price }
    }
    fn states(c: &Classified) -> Vec<PressureState> {
        c.buckets.iter().map(|b| b.state).collect()
    }

    #[test]
    fn hq_any_takes_min_of_qualities() {
        let rows = [fc(0, 0, 1000), fc(0, 1, 900), fc(50, 1, 0)];
        let points = floor_points(&rows, Some(0), 0, 100);
        assert_eq!(floor_at(&points, 10), Some(900));
        assert_eq!(floor_at(&points, 60), Some(1000), "HQ board emptied, NQ floor remains");
    }

    #[test]
    fn floor_is_unknown_before_the_anchor_and_empty_after_it() {
        let points = floor_points(&[], Some(100), 0, 200);
        assert_eq!(floor_at(&points, 50), None);
        assert_eq!(floor_at(&points, 150), None);
        let c = classify(&[], &points, Some(100), 0, 200, 100);
        assert_eq!(states(&c), vec![Unknown, Calm], "anchored world with no rows is a known-empty board");
    }

    #[test]
    fn floor_points_dedupe_and_carry_seed_rows() {
        // Seed row at 5 precedes the window start 10; equal-price rows collapse.
        let rows = [fc(5, 0, 700), fc(20, 0, 700), fc(30, 0, 650)];
        assert_eq!(floor_points(&rows, Some(0), 10, 100), vec![(10, Some(700)), (30, Some(650))]);
    }

    #[test]
    fn trims_and_cuts_split_at_one_percent() {
        let events = [ev(10, 1, 1000, 991), ev(20, 2, 1000, 990)];
        let points = floor_points(&[fc(0, 0, 1000)], Some(0), 0, 3600);
        let c = classify(&events, &points, Some(0), 0, 3600, 3600);
        assert_eq!((c.buckets[0].trims, c.buckets[0].cuts, c.buckets[0].sellers), (1, 1, 2));
    }

    #[test]
    fn buckets_are_epoch_aligned_and_events_clip_to_the_window() {
        let from = 10 * HOUR + 1800;
        let events = [ev(10 * HOUR + 100, 1, 100, 90), ev(10 * HOUR + 1900, 1, 100, 90)];
        let points = floor_points(&[fc(0, 0, 100)], Some(0), from, 13 * HOUR);
        let c = classify(&events, &points, Some(0), from, 13 * HOUR, HOUR);
        let starts: Vec<i64> = c.buckets.iter().map(|b| b.start).collect();
        assert_eq!(starts, vec![10 * HOUR, 11 * HOUR, 12 * HOUR]);
        assert_eq!(c.buckets[0].cuts, 1, "the event before `from` is outside the window");
    }

    /// Six hourly buckets with one churn undercut each except bucket 3,
    /// which carries `war` while the floor moves per `floor`.
    fn six_hours(war: &[UndercutEvent], floor: &[WindowChange]) -> Classified {
        let mut events: Vec<UndercutEvent> =
            [0, 1, 2, 4, 5].iter().map(|h| ev(h * HOUR + 10, 9, 1000, 999)).collect();
        events.extend_from_slice(war);
        events.sort_by_key(|e| e.time);
        let mut rows = vec![fc(0, 0, 1000)];
        rows.extend_from_slice(floor);
        let points = floor_points(&rows, Some(0), 0, 6 * HOUR);
        classify(&events, &points, Some(0), 0, 6 * HOUR, HOUR)
    }
    fn war_events(retainers: [i32; 4]) -> Vec<UndercutEvent> {
        retainers.iter().enumerate().map(|(i, r)| ev(3 * HOUR + 100 + i as i64, *r, 1000, 960)).collect()
    }

    #[test]
    fn war_needs_erosion_volume_and_two_sellers() {
        let drop = [fc(3 * HOUR + 200, 0, 950)];
        assert_eq!(states(&six_hours(&war_events([1, 2, 1, 2]), &drop)), vec![Churn, Churn, Churn, War, Churn, Churn]);
        assert_eq!(six_hours(&war_events([1, 1, 1, 1]), &drop).buckets[3].state, Churn, "one seller is not a war");
        assert_eq!(six_hours(&war_events([1, 2, 1, 2]), &[]).buckets[3].state, Churn, "flat floor is churn");
    }

    #[test]
    fn calm_is_relative_to_a_busy_baseline() {
        let mut events = vec![];
        for h in 0..4 {
            for i in 0..10 {
                events.push(ev(h * HOUR + i, 1, 100, 99));
            }
        }
        for i in 0..3 {
            events.push(ev(4 * HOUR + i, 1, 100, 99));
        }
        let points = floor_points(&[fc(0, 0, 100)], Some(0), 0, 6 * HOUR);
        let c = classify(&events, &points, Some(0), 0, 6 * HOUR, HOUR);
        assert_eq!(c.baseline, Some(10.0));
        assert_eq!(states(&c), vec![Churn, Churn, Churn, Churn, Calm, Calm]);
    }

    #[test]
    fn erosion_requirement_scales_with_bucket_and_caps() {
        let bucket = |close| PressureBucket { start: 0, trims: 0, cuts: 5, sellers: 2, floor_open: Some(1000), floor_close: Some(close), state: Unknown };
        assert_eq!(state_for(&bucket(990), 1.0, HOUR), War, "1% in an hour");
        assert_eq!(state_for(&bucket(960), 1.0, 6 * HOUR), Churn, "6h bucket needs 6%");
        assert_eq!(state_for(&bucket(910), 1.0, DAY), Churn, "day bucket needs the 10% cap");
        assert_eq!(state_for(&bucket(900), 1.0, DAY), War);
    }

    #[test]
    fn war_spans_merge_consecutive_buckets_and_dedupe_sellers() {
        // Eight hourly buckets: one churn undercut in each except 1 and 2,
        // which carry three cuts each. Totals [1,3,3,1,1,1,1,1] → baseline 1
        // → war needs max(2, 3) = 3. (With only four buckets the war buckets
        // would drag the median up and no longer qualify.)
        let mut events: Vec<UndercutEvent> =
            [0, 3, 4, 5, 6, 7].iter().map(|h| ev(h * HOUR + 5, 9, 1000, 999)).collect();
        for (h, rs) in [(1, [1, 2, 1]), (2, [2, 3, 2])] {
            for (i, r) in rs.iter().enumerate() {
                events.push(ev(h * HOUR + 100 + i as i64, *r, 1000, 900));
            }
        }
        events.sort_by_key(|e| e.time);
        let rows = [fc(0, 0, 1000), fc(HOUR + 200, 0, 900), fc(2 * HOUR + 200, 0, 800)];
        let points = floor_points(&rows, Some(0), 0, 8 * HOUR);
        let c = classify(&events, &points, Some(0), 0, 8 * HOUR, HOUR);
        assert_eq!(states(&c), vec![Churn, War, War, Churn, Churn, Churn, Churn, Churn]);
        let spans = war_spans(&c);
        assert_eq!(spans.len(), 1);
        let span = &spans[0];
        // Sellers {1,2} ∪ {2,3} = {1,2,3}.
        assert_eq!((span.start, span.end, span.undercuts, span.sellers), (HOUR, 3 * HOUR, 6, 3));
        assert!((span.floor_change.unwrap() - (800.0 / 1000.0 - 1.0)).abs() < 1e-9);
    }

    #[test]
    fn window_before_anchor_is_all_unknown() {
        let points = floor_points(&[fc(0, 0, 100)], Some(10 * DAY), 0, 2 * DAY);
        let c = classify(&[ev(10, 1, 100, 50)], &points, Some(10 * DAY), 0, 2 * DAY, DAY);
        assert_eq!(states(&c), vec![Unknown, Unknown]);
        assert_eq!(c.baseline, None);
        assert!(c.buckets.iter().all(|b| b.trims + b.cuts == 0 && b.floor_open.is_none()));
        assert!(war_spans(&c).is_empty());
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p ultros-clickhouse undercut_pressure`
Expected: FAIL to compile — `floor_points`, `floor_at`, `classify`, `Classified`, `state_for`, `war_spans` not found.

- [ ] **Step 3: Implement** (insert above the `tests` module)

```rust
/// Distinct as-of floors from `start` to `end`: the floor in force at
/// `start`, then one point per change that moves it. Each quality's last
/// observed price carries forward; from the anchor on, a quality never seen
/// is a known-empty board. The floor is the cheapest non-zero price, `None`
/// when empty. No points exist before the anchor (unknown), and with no
/// anchor the world has no coverage at all.
pub(crate) fn floor_points(
    rows: &[WindowChange],
    anchor: Option<i64>,
    start: i64,
    end: i64,
) -> Vec<(i64, Option<u32>)> {
    let Some(anchor) = anchor else {
        return vec![];
    };
    let start = start.max(anchor);
    let mut rows: Vec<&WindowChange> = rows.iter().collect();
    rows.sort_by_key(|r| r.timestamp);
    let floor = |state: &[u32; 2]| state.iter().copied().filter(|p| *p > 0).min();
    let mut state = [0u32; 2];
    let mut rows = rows.into_iter().peekable();
    while let Some(r) = rows.next_if(|r| r.timestamp <= start) {
        state[usize::from(r.hq.min(1))] = r.price;
    }
    let mut points = vec![(start, floor(&state))];
    for r in rows.take_while(|r| r.timestamp < end) {
        state[usize::from(r.hq.min(1))] = r.price;
        let next = floor(&state);
        if points.last().is_some_and(|(_, p)| *p != next) {
            points.push((r.timestamp, next));
        }
    }
    points
}

/// Floor in force at `t` per `floor_points`; `None` before the first point.
pub(crate) fn floor_at(points: &[(i64, Option<u32>)], t: i64) -> Option<u32> {
    let i = points.partition_point(|(ts, _)| *ts <= t);
    i.checked_sub(1).and_then(|i| points[i].1)
}

pub(crate) fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 0 { (values[mid - 1] + values[mid]) / 2.0 } else { values[mid] })
}

pub(crate) struct Classified {
    pub buckets: Vec<PressureBucket>,
    /// Cutting retainers per bucket, parallel to `buckets` (war-span union).
    pub sellers: Vec<HashSet<i32>>,
    pub baseline: Option<f64>,
    pub bucket_seconds: i64,
}

fn is_trim(e: &UndercutEvent) -> bool {
    f64::from(e.prev_price - e.price) / f64::from(e.prev_price) < TRIM_FRACTION
}

/// Buckets `[floor(from), to)` at `bucket_seconds`, epoch-aligned like
/// `price_series`. Only events inside `[from, to)` count. A bucket is known
/// once it starts at or after the anchor.
pub(crate) fn classify(
    events: &[UndercutEvent],
    points: &[(i64, Option<u32>)],
    anchor: Option<i64>,
    from: i64,
    to: i64,
    bucket_seconds: i64,
) -> Classified {
    let is_known = |start: i64| anchor.is_some_and(|a| start >= a);
    let mut buckets = Vec::new();
    let mut sellers = Vec::new();
    let mut start = from.div_euclid(bucket_seconds) * bucket_seconds;
    while start < to {
        let end = start + bucket_seconds;
        let known = is_known(start);
        let lo = events.partition_point(|e| e.time < start.max(from));
        let hi = events.partition_point(|e| e.time < end.min(to)).max(lo);
        let (mut trims, mut cuts, mut who) = (0u32, 0u32, HashSet::new());
        if known {
            for e in &events[lo..hi] {
                if is_trim(e) {
                    trims += 1;
                } else {
                    cuts += 1;
                }
                who.insert(e.retainer_id);
            }
        }
        buckets.push(PressureBucket {
            start,
            trims,
            cuts,
            sellers: u16::try_from(who.len()).unwrap_or(u16::MAX),
            floor_open: if known { floor_at(points, start) } else { None },
            floor_close: if known { floor_at(points, end.min(to) - 1) } else { None },
            state: PressureState::Unknown,
        });
        sellers.push(who);
        start = end;
    }
    let baseline = median(
        buckets
            .iter()
            .filter(|b| is_known(b.start))
            .map(|b| f64::from(b.trims + b.cuts))
            .collect(),
    );
    if let Some(baseline) = baseline {
        for b in buckets.iter_mut().filter(|b| is_known(b.start)) {
            b.state = state_for(b, baseline, bucket_seconds);
        }
    }
    Classified { buckets, sellers, baseline, bucket_seconds }
}

pub(crate) fn state_for(bucket: &PressureBucket, baseline: f64, bucket_seconds: i64) -> PressureState {
    let total = bucket.trims + bucket.cuts;
    let erosion = match (bucket.floor_open, bucket.floor_close) {
        (Some(open), Some(close)) if open > 0 => (f64::from(open) - f64::from(close)) / f64::from(open),
        _ => 0.0,
    };
    let needed = (WAR_EROSION_PER_HOUR * bucket_seconds as f64 / HOUR as f64).min(WAR_EROSION_CAP);
    let volume = (WAR_BASELINE_MULTIPLE * baseline).max(f64::from(WAR_MIN_UNDERCUTS));
    // The epsilon keeps the spec's `>=` when float division lands a hair low.
    if erosion >= needed - 1e-12 && f64::from(total) >= volume && bucket.sellers >= WAR_MIN_SELLERS {
        PressureState::War
    } else if total == 0 || (baseline >= CALM_MIN_BASELINE && f64::from(total) < CALM_BASELINE_SHARE * baseline) {
        PressureState::Calm
    } else {
        PressureState::Churn
    }
}

/// Maximal runs of consecutive war buckets.
pub(crate) fn war_spans(c: &Classified) -> Vec<WarSpan> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < c.buckets.len() {
        if c.buckets[i].state != PressureState::War {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < c.buckets.len() && c.buckets[j].state == PressureState::War {
            j += 1;
        }
        let run = &c.buckets[i..j];
        let who: HashSet<i32> = c.sellers[i..j].iter().flatten().copied().collect();
        let (open, close) = (run[0].floor_open, run[run.len() - 1].floor_close);
        spans.push(WarSpan {
            start: run[0].start,
            end: run[run.len() - 1].start + c.bucket_seconds,
            undercuts: run.iter().map(|b| b.trims + b.cuts).sum(),
            sellers: u16::try_from(who.len()).unwrap_or(u16::MAX),
            floor_change: open
                .zip(close)
                .filter(|(o, _)| *o > 0)
                .map(|(o, c)| f64::from(c) / f64::from(o) - 1.0),
        });
        i = j;
    }
    spans
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ultros-clickhouse undercut_pressure`
Expected: 10 passed. If `war_needs_erosion_volume_and_two_sellers` or `calm_is_relative_to_a_busy_baseline` fails, recompute the fixture's baseline by hand before touching the reducer — the rules are relative to the median.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-clickhouse/src/undercut_pressure.rs ultros-clickhouse/src/lib.rs
git commit -m "feat(clickhouse): classify undercut pressure buckets and war spans"
```

---

### Task 4: Pressure reducer — episodes and summary

**Files:**
- Modify: `ultros-clickhouse/src/undercut_pressure.rs`

**Interfaces:**
- Consumes: Task 3 functions.
- Produces:
  - `pub struct PressureParams { pub world_id: i32, pub from: i64, pub to: i64, pub bucket_seconds: i64, pub now: i64, pub anchor: Option<i64> }`
  - `pub fn pressure(events: &[UndercutEvent], floor_rows: &[WindowChange], p: &PressureParams) -> UndercutPressure`
  - `pub(crate) fn episodes(points: &[(i64, Option<u32>)], from: i64, to: i64) -> (Option<i64>, u32, u32)` — (median secs, left, undercut)

- [ ] **Step 1: Write failing tests** (append inside `mod tests`)

```rust
    #[test]
    fn episodes_exclude_censored_and_classify_outcomes() {
        // (0) is the carried state at the read start: left-censored.
        let points = vec![
            (0, Some(1000)),
            (100, Some(900)),
            (400, Some(880)), // 100..400 ended by undercut
            (700, Some(950)), // 400..700 left (floor rose)
            (800, None),      // 700..800 left (board emptied)
            (900, Some(800)), // still open: right-censored
        ];
        assert_eq!(episodes(&points, 0, 1000), (Some(300), 2, 1));
        assert_eq!(episodes(&points, 500, 1000), (Some(100), 1, 0), "only episodes starting in the window");
    }

    fn params(from: i64, to: i64, bucket_seconds: i64, now: i64) -> PressureParams {
        PressureParams { world_id: 34, from, to, bucket_seconds, now, anchor: Some(0) }
    }

    #[test]
    fn summary_24h_is_hourly_and_independent_of_chart_window() {
        use ultros_api_types::undercut_pressure::WarStatus;
        let now = 10 * DAY + 1800;
        // A quiet day (one trim every 3 hours) then a war in the current hour.
        let mut events: Vec<UndercutEvent> = (1..8).map(|k| ev(now - k * 3 * HOUR, 9, 1000, 999)).collect();
        events.extend((0..4).map(|i| ev(now - 1500 + i, 1 + (i as i32 % 2), 1000, 900)));
        let rows = [fc(0, 0, 1100), fc(now - DAY + 10, 0, 1000), fc(now - 1400, 0, 900)];
        let out = pressure(&events, &rows, &params(0, 2 * DAY, DAY, now));
        assert_eq!(out.buckets.len(), 2, "chart buckets follow the chart window only");
        assert_eq!(out.summary.war, WarStatus::Active);
        assert_eq!(out.summary.last_war.as_ref().map(|w| w.sellers), Some(2));
        let trend = out.summary.floor_trend_24h.unwrap();
        assert!((trend - (900.0 / 1100.0 - 1.0)).abs() < 1e-9);
    }

    #[test]
    fn war_status_ends_and_expires() {
        use ultros_api_types::undercut_pressure::WarStatus;
        let now = 10 * DAY;
        let war_at = now - 5 * HOUR;
        let events: Vec<UndercutEvent> = (0..4).map(|i| ev(war_at + 100 + i, 1 + (i as i32 % 2), 1000, 900)).collect();
        let rows = [fc(0, 0, 1000), fc(war_at + 200, 0, 900)];
        let out = pressure(&events, &rows, &params(now - DAY, now, HOUR, now));
        assert_eq!(out.summary.war, WarStatus::Ended { at: war_at + HOUR });
        let later = pressure(&events, &rows, &params(now - DAY, now, HOUR, now + 2 * DAY));
        assert_eq!(later.summary.war, WarStatus::None);
    }

    #[test]
    fn summary_contested_share_rate_and_episodes() {
        let events: Vec<UndercutEvent> = (0..4).map(|h| ev(h * HOUR + 10, 9, 1000, 999)).collect();
        let rows = [fc(0, 0, 1000), fc(HOUR, 0, 990), fc(HOUR + 600, 0, 1200)];
        let out = pressure(&events, &rows, &params(0, 6 * HOUR, HOUR, 6 * HOUR));
        // Buckets 0-3 hold one undercut (churn), 4-5 none (calm); baseline = median([1,1,1,1,0,0]) = 1.
        assert_eq!(out.summary.contested_share, Some(4.0 / 6.0));
        assert_eq!(out.summary.typical_undercuts_per_hour, Some(1.0));
        // The only closed episode starting in the window: HOUR..HOUR+600, 990 → 1200 (left).
        assert_eq!(out.summary.floor_holds_median_secs, Some(600));
        assert_eq!((out.summary.episodes_left, out.summary.episodes_undercut), (1, 0));
        assert_eq!(out.coverage_from, Some(0));
    }

    #[test]
    fn no_anchor_means_no_coverage_anywhere() {
        let p = PressureParams { anchor: None, ..params(0, 2 * HOUR, HOUR, 2 * HOUR) };
        let out = pressure(&[ev(10, 1, 100, 50)], &[fc(0, 0, 100)], &p);
        assert!(out.buckets.iter().all(|b| b.state == Unknown));
        assert_eq!(out.summary, Default::default());
        assert_eq!((out.baseline, out.coverage_from), (None, None));
    }
```

In `summary_24h_is_hourly_and_independent_of_chart_window`, check the war hour by hand: hourly buckets over the last day hold one trim in every third hour (baseline = median of mostly zeros = 0), the current hour holds 4 cuts from retainers {1, 2}, floor 1000 → 900 (10% ≥ 1%), volume need = max(0, 3) = 3 ≤ 4 → War. The span touches the current hour → `Active`.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p ultros-clickhouse undercut_pressure`
Expected: FAIL to compile — `episodes`, `pressure`, `PressureParams` not found.

- [ ] **Step 3: Implement** (above `mod tests`; widen the api-types `use` to `{PressureBucket, PressureState, PressureSummary, UndercutPressure, WarSpan, WarStatus}`)

```rust
pub struct PressureParams {
    pub world_id: i32,
    /// Chart window; buckets cover `[from, to)`.
    pub from: i64,
    pub to: i64,
    pub bucket_seconds: i64,
    pub now: i64,
    /// The world's floor anchor (`floor_history::anchors`).
    pub anchor: Option<i64>,
}

/// Closed floor episodes that start at a real transition inside
/// `[from, to)`. `points[0]` is the carried state at the read start (its
/// true start is unknown) and the last point is still open, so both are
/// excluded. Returns the median duration and the (left, undercut) counts.
pub(crate) fn episodes(points: &[(i64, Option<u32>)], from: i64, to: i64) -> (Option<i64>, u32, u32) {
    let (mut durations, mut left, mut undercut) = (Vec::new(), 0u32, 0u32);
    for (i, pair) in points.windows(2).enumerate() {
        let ((t0, p0), (t1, p1)) = (pair[0], pair[1]);
        let Some(p0) = p0 else { continue };
        if i == 0 || t0 < from || t0 >= to {
            continue;
        }
        durations.push((t1 - t0) as f64);
        match p1 {
            Some(p1) if p1 < p0 => undercut += 1,
            _ => left += 1,
        }
    }
    (median(durations).map(|m| m.round() as i64), left, undercut)
}

/// Everything the item page pane and cards need for one item on one world.
/// `events` and `floor_rows` must cover `[min(from, now - DAY), max(to, now))`.
pub fn pressure(events: &[UndercutEvent], floor_rows: &[WindowChange], p: &PressureParams) -> UndercutPressure {
    let mut events = events.to_vec();
    events.sort_by_key(|e| e.time);
    let points = floor_points(floor_rows, p.anchor, p.from.min(p.now - DAY), p.to.max(p.now));
    let chart = classify(&events, &points, p.anchor, p.from, p.to, p.bucket_seconds);
    let wars = war_spans(&chart);

    // The 24 h cards read the same at every zoom: their own hourly buckets.
    let day = classify(&events, &points, p.anchor, p.now - DAY, p.now, HOUR);
    let last_war = war_spans(&day).pop();
    let current_hour = p.now.div_euclid(HOUR) * HOUR;
    let war = match &last_war {
        Some(w) if w.end >= current_hour => WarStatus::Active,
        Some(w) => WarStatus::Ended { at: w.end },
        None => WarStatus::None,
    };
    let floor_trend_24h = floor_at(&points, p.now)
        .zip(floor_at(&points, p.now - DAY))
        .filter(|(_, then)| *then > 0)
        .map(|(now, then)| f64::from(now) / f64::from(then) - 1.0);

    let known: Vec<&PressureBucket> = chart.buckets.iter().filter(|b| b.state != PressureState::Unknown).collect();
    let contested_share = (!known.is_empty()).then(|| {
        let busy = known.iter().filter(|b| matches!(b.state, PressureState::Churn | PressureState::War)).count();
        busy as f64 / known.len() as f64
    });
    let (floor_holds_median_secs, episodes_left, episodes_undercut) = episodes(&points, p.from, p.to);

    UndercutPressure {
        world_id: p.world_id,
        from: p.from,
        to: p.to,
        bucket_seconds: p.bucket_seconds,
        coverage_from: p.anchor,
        baseline: chart.baseline,
        summary: PressureSummary {
            floor_trend_24h,
            war,
            last_war,
            contested_share,
            typical_undercuts_per_hour: chart.baseline.map(|b| b / (p.bucket_seconds as f64 / HOUR as f64)),
            floor_holds_median_secs,
            episodes_left,
            episodes_undercut,
        },
        buckets: chart.buckets,
        wars,
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ultros-clickhouse undercut_pressure`
Expected: 15 passed.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-clickhouse/src/undercut_pressure.rs
git commit -m "feat(clickhouse): undercut pressure summary and floor episodes"
```

---

### Task 5: ClickHouse loader and gated smoke

**Files:**
- Modify: `ultros-clickhouse/src/floor_history.rs` (add `exact_changes` right after `window_changes`)
- Modify: `ultros-clickhouse/src/undercut_pressure.rs` (add `events_sql`, `load`)
- Create: `ultros-clickhouse/tests/undercut_pressure_smoke.rs`

**Interfaces:**
- Consumes: `listing_history::{reprice_events_sql, LIMITS}` (Task 2), `pressure`, `PressureParams` (Task 4).
- Produces: `pub async fn load(ch: &ClickHouseClient, item_id: i32, world_id: i32, hq: HqFilter, p: PressureParams) -> Result<UndercutPressure, ClickHouseError>`; `pub(crate) async fn exact_changes(ch: &ClickHouseClient, items: &[i32], worlds: &[i32], hq: HqFilter, from: i64, to: i64) -> Result<Vec<WindowChange>, ClickHouseError>`.

- [ ] **Step 1: Write the failing unit test** (in `undercut_pressure.rs` tests)

```rust
    #[test]
    fn hq_filter_restricts_events_sql() {
        use ultros_api_types::price_series::HqFilter;
        let any = events_sql(7, 34, HqFilter::Any, 100, 200);
        assert!(!any.contains("WHERE hq = "));
        assert!(events_sql(7, 34, HqFilter::Hq, 100, 200).contains("WHERE hq = 1"));
        assert!(events_sql(7, 34, HqFilter::Nq, 100, 200).contains("WHERE hq = 0"));
        assert!(any.starts_with("SELECT toInt64(event_time) AS time, retainer_id, prev_price, price_per_unit AS price"));
    }
```

Run: `cargo test -p ultros-clickhouse hq_filter_restricts_events_sql` → FAIL (`events_sql` missing).

- [ ] **Step 2: Implement**

`floor_history.rs`, after `window_changes`:

```rust
/// Exact transitions for one quality filter (the item page's pressure pane).
pub(crate) async fn exact_changes(
    ch: &ClickHouseClient,
    items: &[i32],
    worlds: &[i32],
    hq: HqFilter,
    from: i64,
    to: i64,
) -> Result<Vec<WindowChange>, ClickHouseError> {
    load_changes(ch, items, worlds, ChangeWindow { from, to, hq, step: None }).await
}
```

`undercut_pressure.rs` (add `use crate::{ClickHouseClient, ClickHouseError, listing_history::{LIMITS, reprice_events_sql}};` and `use ultros_api_types::price_series::HqFilter;`):

```rust
/// One row per undercut event on one item-world, oldest first. Column names
/// match `UndercutEvent`'s fields (the clickhouse crate checks them).
fn events_sql(item_id: i32, world_id: i32, hq: HqFilter, from: i64, to: i64) -> String {
    let quality = match hq {
        HqFilter::Any => "",
        HqFilter::Hq => " WHERE hq = 1",
        HqFilter::Nq => " WHERE hq = 0",
    };
    format!(
        "SELECT toInt64(event_time) AS time, retainer_id, prev_price, price_per_unit AS price
        FROM ({}){quality} ORDER BY time{LIMITS}",
        reprice_events_sql(&item_id.to_string(), &world_id.to_string(), from, to)
    )
}

/// Two bounded reads (undercut events, exact floor transitions) over
/// `[min(from, now - DAY), max(to, now))`, then the pure reducer.
pub async fn load(
    ch: &ClickHouseClient,
    item_id: i32,
    world_id: i32,
    hq: HqFilter,
    p: PressureParams,
) -> Result<UndercutPressure, ClickHouseError> {
    let read_from = p.from.min(p.now - DAY);
    let read_to = p.to.max(p.now);
    let events = ch
        .client()
        .query(&events_sql(item_id, world_id, hq, read_from, read_to))
        .fetch_all::<UndercutEvent>()
        .await?;
    let rows =
        crate::floor_history::exact_changes(ch, &[item_id], &[world_id], hq, read_from, read_to).await?;
    Ok(pressure(&events, &rows, &p))
}
```

Run: `cargo test -p ultros-clickhouse undercut_pressure` → 16 passed.

- [ ] **Step 3: Write the gated smoke** `ultros-clickhouse/tests/undercut_pressure_smoke.rs`

```rust
//! Deterministic fixtures in an explicitly disposable, loopback-only database.
//! ULTROS_CH_INTEGRATION=1 CLICKHOUSE_DATABASE=ultros_t12_* cargo test -p ultros-clickhouse --test undercut_pressure_smoke
use ultros_api_types::{price_series::HqFilter, undercut_pressure::PressureState};
use ultros_clickhouse::{ClickHouseClient, undercut_pressure};

#[tokio::test]
async fn pressure_counts_both_reprice_shapes_and_one_episode() {
    if std::env::var("ULTROS_CH_INTEGRATION").is_err() {
        return;
    }
    assert!(std::env::var("CLICKHOUSE_URL").unwrap().starts_with("http://127.0.0.1:"));
    assert!(std::env::var("CLICKHOUSE_DATABASE").unwrap().starts_with("ultros_t12_"));
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.unwrap();
    let now = ch.client().query("SELECT toInt64(now())").fetch_one::<i64>().await.unwrap();
    let hour = now.div_euclid(3600) * 3600 - 3600; // one full hour in the past
    let item = 800_000 + (std::process::id() % 100_000) as i32;
    let world = 9001;
    let t = |s: i64| hour + s;
    // listing_events columns: event_time, kind, source, item_id, hq, world_id,
    // listing_id, pg_listing_id, retainer_id, price_per_unit, quantity,
    // prev_price, prev_quantity, reviewed_at
    let events = format!(
        "INSERT INTO listing_events VALUES
        ({},'updated','websocket',{item},0,{world},'a',1,11,90,1,100,1,{}),
        ({},'removed','websocket',{item},0,{world},'b',2,12,100,1,0,0,{}),
        ({},'added','websocket',{item},0,{world},'b',2,12,80,1,0,0,{}),
        ({},'removed','websocket',{item},0,{world},'c',3,13,100,1,0,0,{}),
        ({},'added','websocket',{item},0,{world},'c',3,13,120,1,0,0,{}),
        ({},'updated','snapshot',{item},0,{world},'d',4,14,50,1,100,1,{}),
        ({},'updated','websocket',{item},1,{world},'e',5,15,50,1,100,1,{})",
        t(100), t(100), t(200), t(200), t(230), t(230),
        t(300), t(300), t(330), t(330), t(400), t(400), t(500), t(500)
    );
    ch.client().query(&events).execute().await.unwrap();
    ch.client().query(&events).execute().await.unwrap(); // retried batch: DISTINCT must dedupe
    ch.client()
        .query(&format!(
            "INSERT INTO floor_changes VALUES ({},{item},0,{world},100,'listing'),({},{item},0,{world},90,'listing'),({},{item},0,{world},80,'listing')",
            t(0), t(100), t(230)
        ))
        .execute()
        .await
        .unwrap();
    let anchor = hour - 7200;
    ch.client()
        .query(&format!("INSERT INTO floor_anchors VALUES ({anchor},{world})"))
        .execute()
        .await
        .unwrap();

    let out = undercut_pressure::load(
        &ch,
        item,
        world,
        HqFilter::Nq,
        undercut_pressure::PressureParams {
            world_id: world,
            from: hour,
            to: hour + 3600,
            bucket_seconds: 3600,
            now,
            anchor: Some(anchor),
        },
    )
    .await
    .unwrap();
    assert_eq!(out.buckets.len(), 1);
    let b = &out.buckets[0];
    // NQ only: updated 100→90 and removed@100/added@80; not the raise, the snapshot or HQ.
    assert_eq!((b.trims, b.cuts, b.sellers), (0, 2, 2));
    // One known bucket → baseline 2 → war needs 4 undercuts → churn.
    assert_eq!(b.state, PressureState::Churn);
    // The carried start is the anchor (empty board). Then 100 at t(0) → 90
    // at t(100) → 80 at t(230): two closed episodes (100 s, 130 s), both
    // ended by an undercut; 80 is still open.
    assert_eq!((out.summary.episodes_left, out.summary.episodes_undercut), (0, 2));
    assert_eq!(out.summary.floor_holds_median_secs, Some(115));
}
```

- [ ] **Step 4: Run**

Run: `cargo test -p ultros-clickhouse --test undercut_pressure_smoke`
Expected without `ULTROS_CH_INTEGRATION`: passes (returns early). With a disposable loopback ClickHouse and the env vars from the file header: PASS. Record which ran for the PR.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-clickhouse/src/floor_history.rs ultros-clickhouse/src/undercut_pressure.rs ultros-clickhouse/tests/undercut_pressure_smoke.rs
git commit -m "feat(clickhouse): load undercut pressure for one item-world"
```

---

### Task 6: HTTP endpoint

**Files:**
- Modify: `ultros/src/web.rs` — `PressureQuery` next to `PriceSeriesQuery` (~line 370), `fit_pressure_bucket` + `undercut_pressure` after `floor_history` (~line 878), route after the `floor_history` GET route (~line 3682), `mod pressure_route_tests` at the end of the file.

**Interfaces:**
- Consumes: `ultros_clickhouse::undercut_pressure::{load, PressureParams}`, `ultros_clickhouse::floor_history::anchors`, `ultros_charts::data::buckets::{snap_bucket_seconds, widen_bucket}`, `AnySelector`.
- Produces: `GET /api/v1/undercut_pressure/{world}/{itemid}` → `UndercutPressure` JSON.

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod pressure_route_tests {
    use super::fit_pressure_bucket;

    #[test]
    fn fit_pressure_bucket_snaps_to_the_ladder() {
        assert_eq!(fit_pressure_bucket(0, 86_400, 3600), 3600);
        assert_eq!(fit_pressure_bucket(0, 86_400, 5000), super::snap_bucket_seconds(5000));
    }

    #[test]
    fn fit_pressure_bucket_widens_past_cap() {
        let year = 365 * 86_400;
        let bucket = fit_pressure_bucket(0, year, 3600);
        assert!(year / bucket <= 2000, "{bucket}");
        // 1 h → 8760 buckets (too many); 6 h → 1460 fits.
        assert_eq!(bucket, 6 * 3600);
    }
}
```

Run (background, long first build): `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros pressure_route_tests` → FAIL (`fit_pressure_bucket` missing).

- [ ] **Step 2: Implement**

```rust
#[derive(serde::Deserialize, Debug)]
struct PressureQuery {
    from: Option<i64>,
    to: Option<i64>,
    bucket: Option<i64>,
    hq: Option<String>,
}

/// Most buckets one pressure response may carry.
const PRESSURE_MAX_BUCKETS: i64 = 2000;

/// Snap to the chart's bucket ladder, then widen until the window fits the
/// cap. The client asks for the price chart's bucket so bars line up; only a
/// window the chart itself would not draw at that width gets widened.
fn fit_pressure_bucket(from: i64, to: i64, requested: i64) -> i64 {
    let mut bucket = snap_bucket_seconds(requested.max(1));
    while (to - from) / bucket > PRESSURE_MAX_BUCKETS {
        match widen_bucket(bucket) {
            Some(wider) => bucket = wider,
            None => break,
        }
    }
    bucket
}

/// Undercut pressure for one item on one world. Undercuts only compete
/// within a world, so datacenter and region scopes are rejected.
async fn undercut_pressure(
    State(world_cache): State<Arc<WorldCache>>,
    State(ch): State<ClickHouseClient>,
    State(cache): State<crate::web::price_series_cache::PriceSeriesCache>,
    Path((world, item_id)): Path<(String, i32)>,
    axum::extract::Query(query): axum::extract::Query<PressureQuery>,
) -> Result<axum::response::Response, WebError> {
    static QUERIES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
    let now = chrono::Utc::now().timestamp();
    let to = query.to.unwrap_or(now).min(now);
    let from = query.from.unwrap_or(0);
    if from < 0 || from >= to || to > i64::from(u32::MAX) {
        return Err(WebError::BadRequest);
    }
    let hq = match query.hq.as_deref() {
        Some("hq") => HqFilter::Hq,
        Some("nq") => HqFilter::Nq,
        _ => HqFilter::Any,
    };
    let selected = world_cache.lookup_value_by_name(&world)?;
    let AnySelector::World(world_id) = AnySelector::from(&selected) else {
        return Err(WebError::BadRequest);
    };
    let requested_bucket = query.bucket.unwrap_or(3600);
    let ttl = std::time::Duration::from_secs(60);
    let key = crate::web::price_series_cache::CacheKey {
        item_id,
        scope: world.clone(),
        from,
        to: if query.to.is_some() { to } else { open_window_cache_stamp(to, 60) },
        bucket: requested_bucket,
        group: "pressure",
        hq: hq.as_str(),
        bins: 0,
    };
    if let Some(hit) = cache.get(&key) {
        return Ok(cached_json(hit, ttl));
    }
    let Ok(_permit) = QUERIES.try_acquire() else {
        return Err(WebError::TemporarilyUnavailable);
    };
    let work = async {
        let anchor = ultros_clickhouse::floor_history::anchors(&ch, &[world_id])
            .await?
            .get(&world_id)
            .copied();
        // Nothing before the anchor is known; start the buckets there.
        let from = anchor.map_or(from, |a| from.max(a)).min(to - 1);
        let bucket_seconds = fit_pressure_bucket(from, to, requested_bucket);
        ultros_clickhouse::undercut_pressure::load(
            &ch,
            item_id,
            world_id,
            hq,
            ultros_clickhouse::undercut_pressure::PressureParams { world_id, from, to, bucket_seconds, now, anchor },
        )
        .await
    };
    let payload = tokio::time::timeout(std::time::Duration::from_secs(15), work)
        .await
        .map_err(|_| WebError::TemporarilyUnavailable)?
        .map_err(|e| crate::web::error::ClickHouseQueryError::new("undercut_pressure", e))?;
    let body = serde_json::to_string(&payload).map_err(anyhow::Error::from)?;
    cache.insert(key, body.clone(), ttl);
    Ok(cached_json(body, ttl))
}
```

Imports: add `snap_bucket_seconds, widen_bucket` to the existing `use ultros_charts::data::buckets::{…}` (line 71); add `AnySelector` with the same path `ultros/src/web/api/market_pulse.rs` uses if `web.rs` lacks it. If the compiler cannot infer the `async` block's error type, end it with `Ok::<_, ultros_clickhouse::ClickHouseError>(…?)`.

Route, next to the `floor_history` GET route:

```rust
        .route(
            "/api/v1/undercut_pressure/{world}/{itemid}",
            get(undercut_pressure),
        )
```

- [ ] **Step 3: Run tests**

Run: `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros pressure_route_tests`
Expected: 2 passed.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add ultros/src/web.rs
git commit -m "feat(api): world-scoped undercut pressure endpoint"
```

---

### Task 7: Price chart hooks (time domain, hover timestamp, war shading) + client

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/charts/price_history.rs` — `PriceChartOptions` (33, `Default` at 82), `HoverBucket` (152), `PriceChartModel` (191), early return (~421), hover construction (~1067), final model (~1077), test helper (~1913); war spans drawn after the plot geometry/`time` scale exist and before the first series mark.
- Modify: `ultros-frontend/ultros-charts/src/charts/price_density.rs:210`
- Modify: `ultros-frontend/ultros-frontend-core/src/api.rs` (after `get_floor_history`)

**Interfaces:**
- Produces: `PriceChartModel.time_domain: Option<(i64, i64)>`; `HoverBucket.ts: i64` (bucket start, unix seconds); `PriceChartOptions.war_spans: Vec<(i64, i64)>` (default empty); `get_undercut_pressure(item_id: i32, world: &str, hq: HqFilter, range: Option<(i64, i64)>, bucket_seconds: i64) -> AppResult<UndercutPressure>`.

- [ ] **Step 1: Write failing tests** (in `price_history.rs` tests; `test_util` provides `synthetic_price_series` and `world_helper`)

```rust
    #[test]
    fn model_exposes_time_domain_and_hover_timestamps() {
        let helper = crate::test_util::world_helper();
        let series = crate::test_util::synthetic_price_series();
        let model = build_price_history_chart(&helper, &series, &PriceChartOptions::default());
        let (start, end) = model.time_domain.expect("domain");
        assert!(start < end);
        let first_bucket = series.series.iter().flat_map(|s| &s.buckets).map(|b| b.ts.and_utc().timestamp()).min().unwrap();
        assert_eq!(model.hover.buckets.first().unwrap().ts, first_bucket);
    }

    #[test]
    fn war_spans_draw_one_rect_each_and_default_off() {
        let helper = crate::test_util::world_helper();
        let series = crate::test_util::synthetic_price_series();
        let plain = build_price_history_chart(&helper, &series, &PriceChartOptions::default());
        let (start, end) = plain.time_domain.unwrap();
        let shaded = build_price_history_chart(
            &helper,
            &series,
            &PriceChartOptions { war_spans: vec![(start, start + (end - start) / 4)], ..Default::default() },
        );
        let rects = |m: &PriceChartModel| m.scene.nodes.iter().filter(|n| matches!(n, Node::Rect { .. })).count();
        assert_eq!(rects(&shaded), rects(&plain) + 1);
    }
```

(If `test_util`'s helpers have different names or signatures, use the ones the existing `snapshot_tests.rs` calls — the agent map lists `synthetic_price_series()` and `world_helper`.)

Run: `cargo test -p ultros-charts model_exposes_time_domain` → FAIL (no field `time_domain`).

- [ ] **Step 2: Implement**

1. `PriceChartModel`:
   ```rust
       /// `TimeScale` start/end in unix seconds, so panes under the chart can
       /// share its x axis exactly. `None` when nothing was drawn.
       pub time_domain: Option<(i64, i64)>,
   ```
   Early return: `time_domain: None,`. Final model: the unix seconds of the exact `first_ts`/`last_ts` passed to `TimeScale::new` (after the `options.time_range` override). `TimeScale::new` widens a single-instant domain by ±30 min internally; mirror it: `let (a, b) = (first_ts.and_utc().timestamp(), last_ts.and_utc().timestamp()); time_domain: Some(if a == b { (a - 1800, b + 1800) } else { (a, b) }),`.
2. `HoverBucket`: `/// Bucket start, unix seconds.` `pub ts: i64,`. At ~1067 set it from the bucket start used to compute `x`; in `price_density.rs:210` from the density column's start; in the test helper at ~1913 use `ts: 0`.
3. `PriceChartOptions`:
   ```rust
       /// War spans (unix seconds, `[start, end)`) shaded behind the series.
       /// Empty = off, so exports and every other caller are unchanged.
       pub war_spans: Vec<(i64, i64)>,
   ```
   `Default`: `war_spans: Vec::new(),`.
4. Draw, after `time` and the lane geometry exist and before series marks:
   ```rust
    for &(start, end) in &options.war_spans {
        let (Some(a), Some(b)) = (
            chrono::DateTime::from_timestamp(start, 0),
            chrono::DateTime::from_timestamp(end, 0),
        ) else {
            continue;
        };
        let x0 = time.scale(a.naive_utc()).clamp(plot_left, plot_right);
        let x1 = time.scale(b.naive_utc()).clamp(plot_left, plot_right);
        if x1 > x0 {
            scene.nodes.push(Node::Rect {
                x: x0,
                y: plot_top,
                width: x1 - x0,
                height: price_bottom - plot_top,
                rx: 0.0,
                fill: Color::rgb(227, 73, 72).with_alpha(0.12),
            });
        }
    }
   ```
5. `api.rs`:
   ```rust
   /// Undercut pressure for one item on one world, bucketed like the price
   /// chart (`bucket_seconds` from the price series response).
   pub async fn get_undercut_pressure(
       item_id: i32,
       world: &str,
       hq: HqFilter,
       range: Option<(i64, i64)>,
       bucket_seconds: i64,
   ) -> AppResult<ultros_api_types::undercut_pressure::UndercutPressure> {
       let mut url = format!(
           "/api/v1/undercut_pressure/{world}/{item_id}?hq={}&bucket={bucket_seconds}",
           hq.as_str()
       );
       if let Some((from, to)) = range {
           url.push_str(&format!("&from={from}&to={to}"));
       }
       fetch_api(&url).await
   }
   ```

- [ ] **Step 3: Run tests; snapshots must be unchanged**

Run: `cargo test -p ultros-charts` → all pass, `snapshot_tests` byte-equal (war spans default off; `ts` / `time_domain` are not rendered). A snapshot diff means the change leaked into rendering — fix it, do not regenerate.
Run: `cargo check -p ultros-frontend-core` → clean.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-charts/src/charts/price_history.rs ultros-frontend/ultros-charts/src/charts/price_density.rs ultros-frontend/ultros-frontend-core/src/api.rs
git commit -m "feat(charts): expose price chart time domain, hover timestamps and war shading"
```

---

### Task 8: Pressure pane layout

**Files:**
- Create: `ultros-frontend/ultros-charts/src/charts/undercut_pressure.rs`
- Modify: `ultros-frontend/ultros-charts/src/charts/mod.rs` (`pub mod undercut_pressure;`)
- Modify: `ultros-frontend/ultros-charts/src/charts/snapshot_tests.rs` (two cases)

**Interfaces:**
- Consumes: `UndercutPressure` (Task 1); the price chart's `time_domain` (Task 7) is passed in by the component.
- Produces:
  - `pub const PANE_MARGIN_LEFT: f32 = 68.0; pub const PANE_MARGIN_RIGHT: f32 = 16.0;`
  - `pub fn pane_height(width: f32) -> f32`
  - `pub struct PressurePalette { pub cut, trim, sales, baseline, war, churn, calm, unknown, grid: Color }` + `Default`
  - `pub struct PressurePaneOptions { pub width: f32, pub height: f32, pub time_domain: (i64, i64), pub palette: PressurePalette }`
  - `pub struct PressurePaneModel { pub scene: Scene, pub bars_top: f32, pub bars_bottom: f32 }`
  - `pub fn bucket_x(p: &UndercutPressure, o: &PressurePaneOptions, start: i64) -> f32`
  - `pub fn build_undercut_pressure_chart(p: &UndercutPressure, sales: &[(i64, u32)], o: &PressurePaneOptions) -> PressurePaneModel` — `sales` = (bucket start, sale rows), already summed over series.
  - `#[doc(hidden)] pub fn tests_fixture() -> UndercutPressure` (used by this crate's tests and by Task 9's tests)

- [ ] **Step 1: Write failing tests** (bottom of the new file)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> PressurePaneOptions {
        PressurePaneOptions {
            width: 960.0,
            height: pane_height(960.0),
            time_domain: (0, 6 * 3600),
            palette: PressurePalette::default(),
        }
    }

    #[test]
    fn bucket_x_matches_the_price_chart_time_scale() {
        let plot = 960.0 - PANE_MARGIN_LEFT - PANE_MARGIN_RIGHT;
        let expected = PANE_MARGIN_LEFT + plot * 2.5 / 6.0;
        assert!((bucket_x(&tests_fixture(), &options(), 2 * 3600) - expected).abs() < 0.01);
    }

    #[test]
    fn bars_skip_unknown_and_stay_in_the_plot() {
        let m = build_undercut_pressure_chart(&tests_fixture(), &[], &options());
        let p = PressurePalette::default();
        let bar_paths: Vec<&String> = m.scene.nodes.iter().filter_map(|n| match n {
            Node::Path { d, fill: Some(f), .. } if *f == p.cut || *f == p.trim => Some(d),
            _ => None,
        }).collect();
        // Cut path appears once and trim path once, even though the war colour equals the cut colour.
        assert!(!bar_paths.is_empty());
        for d in bar_paths {
            for rect in d.split('M').filter(|s| !s.is_empty()) {
                let mut nums = rect.split(|c: char| c == ' ' || c.is_ascii_alphabetic()).filter(|s| !s.is_empty());
                let x: f32 = nums.next().unwrap().parse().unwrap();
                let y: f32 = nums.next().unwrap().parse().unwrap();
                assert!(x >= bucket_x(&tests_fixture(), &options(), 3600) - 60.0, "unknown bucket 0 drew a bar: {rect}");
                assert!(x <= 960.0 - PANE_MARGIN_RIGHT && y >= m.bars_top - 0.5 && y <= m.bars_bottom, "{rect}");
            }
        }
    }

    #[test]
    fn sales_line_zero_fills_missing_buckets() {
        let m = build_undercut_pressure_chart(&tests_fixture(), &[(3600, 4), (5 * 3600, 1)], &options());
        let line = m.scene.nodes.iter().find_map(|n| match n {
            Node::Polyline { points, .. } => Some(points.clone()),
            _ => None,
        }).expect("sales line");
        assert_eq!(line.len(), 5, "one point per known bucket");
        assert!((line[1].1 - m.bars_bottom).abs() < 0.01, "bucket 2 has no sales → zero");
    }

    #[test]
    fn baseline_is_dashed_and_ribbon_batches_per_state() {
        let m = build_undercut_pressure_chart(&tests_fixture(), &[], &options());
        assert!(m.scene.nodes.iter().any(|n| matches!(n, Node::Line { stroke, .. } if stroke.dash.is_some())));
        // Ribbon: war, churn, calm, unknown → 4 paths; bars: cut, trim → 2 paths.
        let paths = m.scene.nodes.iter().filter(|n| matches!(n, Node::Path { .. })).count();
        assert_eq!(paths, 6);
    }
}
```

Run: `cargo test -p ultros-charts undercut_pressure` → FAIL (module empty).

- [ ] **Step 2: Implement** (top of the new file)

```rust
//! Undercut-pressure pane: a short lane under the price chart that shares
//! its time axis. A state ribbon, stacked cut/trim bars, the item's typical
//! level (dashed) and a sales line. Everything is a count per bucket, so one
//! y axis. Horizontal geometry mirrors `price_history` exactly.
use crate::scale::{LinearScale, TimeScale};
use crate::scene::{Color, Node, Scene, Stroke};
use crate::svg::rects_path_d;
use ultros_api_types::undercut_pressure::{PressureBucket, PressureState, UndercutPressure};

pub const PANE_MARGIN_LEFT: f32 = 68.0;
pub const PANE_MARGIN_RIGHT: f32 = 16.0;
const RIBBON_TOP: f32 = 2.0;
const RIBBON_HEIGHT: f32 = 6.0;
const RIBBON_GAP: f32 = 6.0;
const BOTTOM_PAD: f32 = 4.0;

pub fn pane_height(width: f32) -> f32 {
    (width * 0.16).clamp(90.0, 150.0)
}

#[derive(Clone, Debug, PartialEq)]
pub struct PressurePalette {
    pub cut: Color,
    pub trim: Color,
    pub sales: Color,
    pub baseline: Color,
    pub war: Color,
    pub churn: Color,
    pub calm: Color,
    pub unknown: Color,
    pub grid: Color,
}

impl Default for PressurePalette {
    fn default() -> Self {
        Self {
            cut: Color::hex("#e34948"),
            trim: Color::hex("#eda100"),
            sales: Color::hex("#eb6834"),
            baseline: Color::hex("#898781"),
            // One step darker than `cut` so the ribbon and bars batch into
            // separate paths and `color_attr` can theme them independently.
            war: Color::hex("#d23c3b"),
            churn: Color::hex("#6b6875"),
            calm: Color::hex("#1baf7a"),
            unknown: Color::hex("#3a3644"),
            grid: Color::hex("#2c2c2a"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PressurePaneOptions {
    pub width: f32,
    pub height: f32,
    /// The price chart's `time_domain`, unix seconds.
    pub time_domain: (i64, i64),
    pub palette: PressurePalette,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PressurePaneModel {
    pub scene: Scene,
    pub bars_top: f32,
    pub bars_bottom: f32,
}

fn naive(ts: i64) -> chrono::NaiveDateTime {
    chrono::DateTime::from_timestamp(ts, 0).unwrap_or_default().naive_utc()
}

fn time_scale(o: &PressurePaneOptions) -> TimeScale {
    TimeScale::new(
        naive(o.time_domain.0),
        naive(o.time_domain.1),
        (PANE_MARGIN_LEFT, o.width - PANE_MARGIN_RIGHT),
    )
}

/// Centre x of the bucket starting at `start`.
pub fn bucket_x(p: &UndercutPressure, o: &PressurePaneOptions, start: i64) -> f32 {
    time_scale(o).scale(naive(start + p.bucket_seconds / 2))
}

fn known(b: &PressureBucket) -> bool {
    b.state != PressureState::Unknown
}

pub fn build_undercut_pressure_chart(
    p: &UndercutPressure,
    sales: &[(i64, u32)],
    o: &PressurePaneOptions,
) -> PressurePaneModel {
    let time = time_scale(o);
    let (left, right) = (PANE_MARGIN_LEFT, o.width - PANE_MARGIN_RIGHT);
    let bars_top = RIBBON_TOP + RIBBON_HEIGHT + RIBBON_GAP;
    let bars_bottom = o.height - BOTTOM_PAD;
    let sales_in = |start: i64| -> u32 {
        sales
            .iter()
            .filter(|(ts, _)| *ts >= start && *ts < start + p.bucket_seconds)
            .map(|(_, n)| *n)
            .sum()
    };
    let max = p
        .buckets
        .iter()
        .filter(|b| known(b))
        .map(|b| f64::from((b.trims + b.cuts).max(sales_in(b.start))))
        .fold(p.baseline.unwrap_or(0.0), f64::max)
        .max(1.0);
    let y = LinearScale::new((0.0, max), (bars_bottom, bars_top));
    let edge = |ts: i64| time.scale(naive(ts)).clamp(left, right);

    let mut nodes = vec![Node::Line {
        x1: left,
        y1: bars_bottom,
        x2: right,
        y2: bars_bottom,
        stroke: Stroke { color: o.palette.grid.clone(), width: 1.0, dash: None },
    }];
    let mut ribbon: [Vec<(f32, f32, f32, f32)>; 4] = Default::default();
    let (mut cut_rects, mut trim_rects, mut sales_points) = (Vec::new(), Vec::new(), Vec::new());
    for b in &p.buckets {
        let (x0, x1) = (edge(b.start), edge(b.start + p.bucket_seconds));
        if x1 <= x0 {
            continue;
        }
        let lane = match b.state {
            PressureState::War => 0,
            PressureState::Churn => 1,
            PressureState::Calm => 2,
            PressureState::Unknown => 3,
        };
        ribbon[lane].push((x0, RIBBON_TOP, (x1 - x0 - 1.0).max(0.5), RIBBON_HEIGHT));
        if !known(b) {
            continue;
        }
        let width = ((x1 - x0) * 0.8).max(1.0);
        let bx = x0 + (x1 - x0 - width) / 2.0;
        let cut_top = y.scale(f64::from(b.cuts));
        let trim_top = y.scale(f64::from(b.cuts + b.trims));
        if b.cuts > 0 {
            cut_rects.push((bx, cut_top, width, bars_bottom - cut_top));
        }
        if b.trims > 0 {
            trim_rects.push((bx, trim_top, width, cut_top - trim_top));
        }
        sales_points.push(((x0 + x1) / 2.0, y.scale(f64::from(sales_in(b.start)))));
    }
    let lanes = [&o.palette.war, &o.palette.churn, &o.palette.calm, &o.palette.unknown];
    for (rects, color) in ribbon.iter().zip(lanes) {
        if let Some(d) = rects_path_d(rects) {
            nodes.push(Node::Path { d, fill: Some(color.clone()), stroke: None });
        }
    }
    for (rects, color) in [(&cut_rects, &o.palette.cut), (&trim_rects, &o.palette.trim)] {
        if let Some(d) = rects_path_d(rects) {
            nodes.push(Node::Path { d, fill: Some(color.clone()), stroke: None });
        }
    }
    if let Some(baseline) = p.baseline.filter(|b| *b > 0.0) {
        let by = y.scale(baseline);
        nodes.push(Node::Line {
            x1: left,
            y1: by,
            x2: right,
            y2: by,
            stroke: Stroke { color: o.palette.baseline.clone(), width: 1.5, dash: Some((5.0, 3.0)) },
        });
    }
    if sales_points.len() >= 2 {
        nodes.push(Node::Polyline {
            points: sales_points,
            stroke: Stroke { color: o.palette.sales.clone(), width: 2.0, dash: None },
        });
    }
    PressurePaneModel {
        scene: Scene { width: o.width, height: o.height, background: None, font_family: String::new(), nodes },
        bars_top,
        bars_bottom,
    }
}

/// Six hourly buckets: unknown, churn, war, churn, calm, churn. Shared by
/// this crate's tests/snapshots and `ultros-ui-charts` tests.
#[doc(hidden)]
pub fn tests_fixture() -> UndercutPressure {
    let bucket = |i: i64, trims, cuts, state| PressureBucket {
        start: i * 3600,
        trims,
        cuts,
        sellers: 2,
        floor_open: Some(1000),
        floor_close: Some(990),
        state,
    };
    UndercutPressure {
        world_id: 34,
        from: 0,
        to: 6 * 3600,
        bucket_seconds: 3600,
        coverage_from: Some(3600),
        baseline: Some(3.0),
        buckets: vec![
            bucket(0, 0, 0, PressureState::Unknown),
            bucket(1, 2, 1, PressureState::Churn),
            bucket(2, 1, 6, PressureState::War),
            bucket(3, 3, 0, PressureState::Churn),
            bucket(4, 0, 0, PressureState::Calm),
            bucket(5, 1, 1, PressureState::Churn),
        ],
        wars: vec![],
        summary: Default::default(),
    }
}
```

The palette's `war` is deliberately not equal to `cut` (the path-count test relies on the ribbon and bars being separate nodes, and theme tokens need distinct RGB keys). If `Scene` has a canonical font family (`Theme::site().font_family`), use it instead of `String::new()`. If `Color` is `Copy`, drop the `.clone()`s (clippy `clone_on_copy`).

- [ ] **Step 3: Run tests**

Run: `cargo test -p ultros-charts undercut_pressure` → 4 passed.

- [ ] **Step 4: Snapshots** — in `snapshot_tests.rs` add, following the existing `assert_snapshot(name, &scene_to_svg(&scene))` pattern:

```rust
#[test]
fn undercut_pressure_pane_snapshot() {
    use crate::charts::undercut_pressure::*;
    let o = PressurePaneOptions { width: 960.0, height: pane_height(960.0), time_domain: (0, 6 * 3600), palette: PressurePalette::default() };
    let m = build_undercut_pressure_chart(&tests_fixture(), &[(3600, 2), (2 * 3600, 1), (5 * 3600, 3)], &o);
    assert_snapshot("undercut_pressure_pane", &crate::svg::scene_to_svg(&m.scene));
}
```

Generate: `UPDATE_SNAPSHOTS=1 cargo test -p ultros-charts undercut_pressure_pane_snapshot`; open `charts/snapshots/undercut_pressure_pane.svg` in the browser pane and check: grey unknown ribbon cell on the left, red war cell in the middle, stacked bars under the ribbon, dashed line at 3, orange sales line. Then `cargo test -p ultros-charts` → all pass.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-charts/src/charts/undercut_pressure.rs ultros-frontend/ultros-charts/src/charts/mod.rs ultros-frontend/ultros-charts/src/charts/snapshot_tests.rs ultros-frontend/ultros-charts/src/charts/snapshots/undercut_pressure_pane.svg
git commit -m "feat(charts): undercut pressure pane layout"
```

---

### Task 9: UI components and i18n keys

**Files:**
- Create: `ultros-frontend/ultros-ui-charts/src/components/undercut_pressure.rs`, `ultros-frontend/ultros-ui-charts/src/components/undercut_pressure.css`
- Modify: `ultros-frontend/ultros-ui-charts/src/components/mod.rs` (`pub mod undercut_pressure;`)
- Modify: `ultros-frontend/ultros-ui-charts/src/components/price_history_chart.rs` (`color_attr` tokens, `pressure_scene_view`)
- Modify: `ultros-frontend/ultros-i18n/locales/{en,fr,de,ja,cn,ko,tc}.json`

**Interfaces:**
- Consumes: Task 8 layout and `tests_fixture`, Task 1 types.
- Produces:
  - `#[component] pub fn UndercutPressureCards(#[prop(into)] pressure: Signal<Option<UndercutPressure>>, #[prop(into)] error: Signal<bool>) -> impl IntoView`
  - `#[component] pub fn UndercutPressurePane(#[prop(into)] pressure: Signal<Option<UndercutPressure>>, #[prop(into)] sales: Signal<Vec<(i64, u32)>>, #[prop(into)] time_domain: Signal<Option<(i64, i64)>>, #[prop(into)] width: Signal<f32>, #[prop(into)] hover_x: Signal<Option<f32>>) -> impl IntoView`
  - `pub fn pressure_bucket_at(p: &UndercutPressure, ts: i64) -> Option<&PressureBucket>`
  - `pub(crate) fn pressure_scene_view(scene: &Scene) -> impl IntoView + use<>` in `price_history_chart.rs`
  - pure helpers `duration_parts`, `contested_level`, `trend_word`, `outcome_split` (below)

- [ ] **Step 1: i18n keys** — add to all seven locale files (append after the last `chart_*` key in each). English values below; write real translations for fr, de, ja, cn (Simplified Chinese), ko, tc (Traditional Chinese), keeping `{{…}}` placeholders verbatim.

| key | en |
|---|---|
| `undercut_pressure_title` | `Undercut pressure` |
| `undercut_pressure_pane_label` | `Undercuts per time bucket on this world` |
| `undercut_pressure_legend_cuts` | `Cuts (1% or more)` |
| `undercut_pressure_legend_trims` | `Trims (under 1%)` |
| `undercut_pressure_legend_sales` | `Sales` |
| `undercut_pressure_legend_baseline` | `Typical level` |
| `undercut_pressure_state_war` | `War` |
| `undercut_pressure_state_churn` | `Churn` |
| `undercut_pressure_state_calm` | `Calm` |
| `undercut_pressure_tooltip_undercuts` | `{{total}} undercuts ({{cuts}} cuts, {{trims}} trims)` |
| `undercut_pressure_tooltip_sellers` | `{{sellers}} sellers cutting` |
| `undercut_pressure_card_trend` | `Floor trend (24h)` |
| `undercut_pressure_trend_falling` | `Falling` |
| `undercut_pressure_trend_steady` | `Steady` |
| `undercut_pressure_trend_rising` | `Rising` |
| `undercut_pressure_card_war` | `Price war` |
| `undercut_pressure_war_active` | `Active` |
| `undercut_pressure_war_ended` | `Ended {{hours}}h ago` |
| `undercut_pressure_war_none` | `None in 24h` |
| `undercut_pressure_war_detail` | `{{sellers}} sellers · floor {{change}}` |
| `undercut_pressure_contested_always` | `Always contested` |
| `undercut_pressure_contested_often` | `Often contested` |
| `undercut_pressure_contested_quiet` | `Mostly quiet` |
| `undercut_pressure_typical_rate` | `~{{rate}} undercuts/hr typical` |
| `undercut_pressure_card_holds` | `Floor holds` |
| `undercut_pressure_holds_detail` | `Time at the floor, this period` |
| `undercut_pressure_card_outcome` | `Sold vs undercut` |
| `undercut_pressure_outcome_detail` | `How floor listings left` |
| `undercut_pressure_minutes` | `~{{n}} min` |
| `undercut_pressure_hours` | `~{{n}} h` |
| `undercut_pressure_days` | `~{{n}} d` |
| `undercut_pressure_unavailable` | `Undercut data is temporarily unavailable.` |

Verify: `python -c "import json,glob;[json.load(open(f,encoding='utf-8')) for f in glob.glob('ultros-frontend/ultros-i18n/locales/*.json')]"` → no error; `grep -c '"undercut_pressure_' ultros-frontend/ultros-i18n/locales/*.json` → 32 in every file.

- [ ] **Step 2: Failing tests for the pure helpers** (bottom of `undercut_pressure.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_humanize_by_magnitude() {
        assert_eq!(duration_parts(90), (DurationUnit::Minutes, 2));
        assert_eq!(duration_parts(35 * 60), (DurationUnit::Minutes, 35));
        assert_eq!(duration_parts(2 * 3600 + 1000), (DurationUnit::Hours, 2));
        assert_eq!(duration_parts(3 * 86_400), (DurationUnit::Days, 3));
    }

    #[test]
    fn contested_and_trend_thresholds() {
        assert_eq!(contested_level(0.8), ContestedLevel::Always);
        assert_eq!(contested_level(0.3), ContestedLevel::Often);
        assert_eq!(contested_level(0.29), ContestedLevel::Quiet);
        assert_eq!(trend_word(-0.021), TrendWord::Falling);
        assert_eq!(trend_word(0.02), TrendWord::Steady);
        assert_eq!(trend_word(0.021), TrendWord::Rising);
    }

    #[test]
    fn outcome_split_sums_to_100() {
        assert_eq!(outcome_split(0, 0), None);
        assert_eq!(outcome_split(2, 1), Some((67, 33)));
        assert_eq!(outcome_split(5, 3), Some((63, 37)));
    }

    #[test]
    fn bucket_lookup_contains_the_timestamp() {
        let p = ultros_charts::charts::undercut_pressure::tests_fixture();
        assert_eq!(pressure_bucket_at(&p, 2 * 3600 + 5).map(|b| b.start), Some(2 * 3600));
        assert!(pressure_bucket_at(&p, 99 * 3600).is_none());
    }
}
```

Run: `cargo test -p ultros-ui-charts undercut_pressure` → FAIL.

- [ ] **Step 3: Implement**

```rust
//! Item page undercut pressure: the pane under the price chart and the four
//! stat cards. World scope only — the route gates the fetch.
use crate::i18n::{t, t_string, use_i18n};
use leptos::prelude::*;
use ultros_api_types::undercut_pressure::{PressureBucket, UndercutPressure, WarStatus};
use ultros_charts::charts::undercut_pressure::{
    PressurePaneOptions, PressurePalette, build_undercut_pressure_chart, pane_height,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DurationUnit {
    Minutes,
    Hours,
    Days,
}

pub(crate) fn duration_parts(secs: i64) -> (DurationUnit, i64) {
    if secs < 90 * 60 {
        (DurationUnit::Minutes, ((secs + 30) / 60).max(1))
    } else if secs < 36 * 3600 {
        (DurationUnit::Hours, (secs + 1800) / 3600)
    } else {
        (DurationUnit::Days, (secs + 43_200) / 86_400)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContestedLevel {
    Always,
    Often,
    Quiet,
}

pub(crate) fn contested_level(share: f64) -> ContestedLevel {
    if share >= 0.8 {
        ContestedLevel::Always
    } else if share >= 0.3 {
        ContestedLevel::Often
    } else {
        ContestedLevel::Quiet
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrendWord {
    Falling,
    Steady,
    Rising,
}

pub(crate) fn trend_word(fraction: f64) -> TrendWord {
    if fraction < -0.02 {
        TrendWord::Falling
    } else if fraction > 0.02 {
        TrendWord::Rising
    } else {
        TrendWord::Steady
    }
}

/// Rounded (left, undercut) percentages that always sum to 100.
pub(crate) fn outcome_split(left: u32, undercut: u32) -> Option<(u32, u32)> {
    let total = left + undercut;
    (total > 0).then(|| {
        let left_pct = (f64::from(left) / f64::from(total) * 100.0).round() as u32;
        (left_pct, 100 - left_pct)
    })
}

pub fn pressure_bucket_at(p: &UndercutPressure, ts: i64) -> Option<&PressureBucket> {
    p.buckets.iter().find(|b| ts >= b.start && ts < b.start + p.bucket_seconds)
}

fn percent(fraction: f64) -> String {
    format!("{:+.1}%", fraction * 100.0)
}

#[component]
pub fn UndercutPressureCards(
    #[prop(into)] pressure: Signal<Option<UndercutPressure>>,
    #[prop(into)] error: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let summary = Memo::new(move |_| pressure.get().map(|p| p.summary));
    let duration = move |secs: i64| match duration_parts(secs) {
        (DurationUnit::Minutes, n) => t_string!(i18n, undercut_pressure_minutes, n = n).to_string(),
        (DurationUnit::Hours, n) => t_string!(i18n, undercut_pressure_hours, n = n).to_string(),
        (DurationUnit::Days, n) => t_string!(i18n, undercut_pressure_days, n = n).to_string(),
    };
    let war_value = move || match summary.get().map(|s| s.war) {
        Some(WarStatus::Active) => t_string!(i18n, undercut_pressure_war_active).to_string(),
        Some(WarStatus::Ended { at }) => {
            let hours = ((chrono::Utc::now().timestamp() - at).max(0) + 1800) / 3600;
            t_string!(i18n, undercut_pressure_war_ended, hours = hours).to_string()
        }
        _ => t_string!(i18n, undercut_pressure_war_none).to_string(),
    };
    let war_detail = move || {
        let s = summary.get()?;
        if s.war != WarStatus::None
            && let Some(w) = s.last_war.as_ref()
        {
            let change = w.floor_change.map(percent).unwrap_or_else(|| "—".into());
            return Some(t_string!(i18n, undercut_pressure_war_detail, sellers = w.sellers, change = change).to_string());
        }
        let level = match contested_level(s.contested_share?) {
            ContestedLevel::Always => t_string!(i18n, undercut_pressure_contested_always).to_string(),
            ContestedLevel::Often => t_string!(i18n, undercut_pressure_contested_often).to_string(),
            ContestedLevel::Quiet => t_string!(i18n, undercut_pressure_contested_quiet).to_string(),
        };
        Some(match s.typical_undercuts_per_hour {
            Some(rate) => {
                let rate = t_string!(i18n, undercut_pressure_typical_rate, rate = format!("{rate:.1}")).to_string();
                format!("{level} · {rate}")
            }
            None => level,
        })
    };
    let trend_detail = move || {
        summary.get().and_then(|s| s.floor_trend_24h).map(|f| match trend_word(f) {
            TrendWord::Falling => t_string!(i18n, undercut_pressure_trend_falling).to_string(),
            TrendWord::Steady => t_string!(i18n, undercut_pressure_trend_steady).to_string(),
            TrendWord::Rising => t_string!(i18n, undercut_pressure_trend_rising).to_string(),
        })
    };
    view! {
        <style>{include_str!("undercut_pressure.css")}</style>
        <Show when=move || error.get()>
            <p class="mh-pressure-unavailable" role="status">{t!(i18n, undercut_pressure_unavailable)}</p>
        </Show>
        <Show when=move || summary.with(Option::is_some)>
            <div class="mh-stats mh-pressure-stats">
                <div class="mh-stat">
                    <span>{t!(i18n, undercut_pressure_card_trend)}</span>
                    <strong>{move || summary.get().and_then(|s| s.floor_trend_24h).map(percent).unwrap_or_else(|| "—".into())}</strong>
                    <p>{trend_detail}</p>
                </div>
                <div class="mh-stat" class:mh-pressure-war=move || summary.get().is_some_and(|s| s.war == WarStatus::Active)>
                    <span>{t!(i18n, undercut_pressure_card_war)}</span>
                    <strong>{war_value}</strong>
                    <p>{war_detail}</p>
                </div>
                <div class="mh-stat">
                    <span>{t!(i18n, undercut_pressure_card_holds)}</span>
                    <strong>{move || summary.get().and_then(|s| s.floor_holds_median_secs).map(duration).unwrap_or_else(|| "—".into())}</strong>
                    <p>{t!(i18n, undercut_pressure_holds_detail)}</p>
                </div>
                <div class="mh-stat">
                    <span>{t!(i18n, undercut_pressure_card_outcome)}</span>
                    <strong>{move || summary.get().and_then(|s| outcome_split(s.episodes_left, s.episodes_undercut)).map(|(l, u)| format!("{l}% / {u}%")).unwrap_or_else(|| "—".into())}</strong>
                    <p>{t!(i18n, undercut_pressure_outcome_detail)}</p>
                </div>
            </div>
        </Show>
    }
}

#[component]
pub fn UndercutPressurePane(
    #[prop(into)] pressure: Signal<Option<UndercutPressure>>,
    /// (bucket start, sale rows) summed over the price series.
    #[prop(into)] sales: Signal<Vec<(i64, u32)>>,
    #[prop(into)] time_domain: Signal<Option<(i64, i64)>>,
    #[prop(into)] width: Signal<f32>,
    /// Crosshair x in scene units (the price chart's hovered bucket).
    #[prop(into)] hover_x: Signal<Option<f32>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let model = Memo::new(move |_| {
        let p = pressure.get()?;
        let domain = time_domain.get()?;
        let width = width.get();
        Some(build_undercut_pressure_chart(
            &p,
            &sales.get(),
            &PressurePaneOptions { width, height: pane_height(width), time_domain: domain, palette: PressurePalette::default() },
        ))
    });
    move || {
        let m = model.get()?;
        let (top, bottom) = (m.bars_top, m.bars_bottom);
        Some(view! {
            <div class="mh-pressure-pane">
                <div class="mh-pressure-legend">
                    <span class="mh-pressure-title">{t!(i18n, undercut_pressure_title)}</span>
                    <span><i class="mh-swatch mh-swatch-cut"></i>{t!(i18n, undercut_pressure_legend_cuts)}</span>
                    <span><i class="mh-swatch mh-swatch-trim"></i>{t!(i18n, undercut_pressure_legend_trims)}</span>
                    <span><i class="mh-swatch mh-swatch-sales"></i>{t!(i18n, undercut_pressure_legend_sales)}</span>
                    <span><i class="mh-swatch mh-swatch-baseline"></i>{t!(i18n, undercut_pressure_legend_baseline)}</span>
                    <span><i class="mh-swatch mh-swatch-war"></i>{t!(i18n, undercut_pressure_state_war)}</span>
                    <span><i class="mh-swatch mh-swatch-churn"></i>{t!(i18n, undercut_pressure_state_churn)}</span>
                    <span><i class="mh-swatch mh-swatch-calm"></i>{t!(i18n, undercut_pressure_state_calm)}</span>
                </div>
                <svg
                    class="block w-full h-auto"
                    role="img"
                    aria-label=move || t_string!(i18n, undercut_pressure_pane_label).to_string()
                    viewBox=format!("0 0 {:.0} {:.0}", m.scene.width, m.scene.height)
                    preserveAspectRatio="xMidYMid meet"
                >
                    {crate::components::price_history_chart::pressure_scene_view(&m.scene)}
                    {move || hover_x.get().map(|x| view! {
                        <line x1=x x2=x y1=top y2=bottom class="mh-pressure-crosshair" />
                    })}
                </svg>
            </div>
        })
    }
}
```

In `price_history_chart.rs`: add below the local `scene_view`

```rust
/// The pane under the chart renders through the same theme-token mapping.
pub(crate) fn pressure_scene_view(scene: &ultros_charts::scene::Scene) -> impl IntoView + use<> {
    scene_view(scene)
}
```

and extend `color_attr`'s match with the pane palette (keys = the `PressurePalette::default()` RGB values):

```rust
        (227, 73, 72) => "var(--mh-cut)",
        (237, 161, 0) => "var(--mh-trim)",
        (235, 104, 52) => "var(--mh-sales)",
        (137, 135, 129) => "var(--mh-baseline)",
        (210, 60, 59) => "var(--mh-war)",
        (107, 104, 117) => "var(--mh-churn)",
        (27, 175, 122) => "var(--mh-calm)",
        (58, 54, 68) => "var(--mh-unknown)",
        (44, 44, 42) => "var(--color-outline)",
```

(The price chart's war shading from Task 7 uses `(227, 73, 72)` at alpha 0.12 and so resolves through `--mh-cut` with `color-mix` — intended.)

`undercut_pressure.css`:

```css
/* Pressure tokens: the pane's scene colors resolve to these via color_attr. */
.market-history { --mh-cut: #e34948; --mh-trim: #eda100; --mh-sales: #eb6834; --mh-baseline: #898781; --mh-war: #d23c3b; --mh-churn: #6b6875; --mh-calm: #1baf7a; --mh-unknown: #3a3644; }
[data-theme="light"] .market-history { --mh-cut: #c62f2f; --mh-trim: #a86f00; --mh-sales: #c24e1f; --mh-baseline: #6b6962; --mh-war: #b32626; --mh-churn: #b9b6c2; --mh-calm: #0f8a5f; --mh-unknown: #e4e1ea; }
.mh-pressure-stats { margin-top: -8px; }
.mh-pressure-war>strong { color: var(--mh-war); }
.mh-pressure-unavailable { font-size: 12px; color: var(--mh-muted); margin: 0 0 12px; }
.mh-pressure-pane { margin-top: 10px; }
.mh-pressure-legend { display: flex; flex-wrap: wrap; gap: 6px 14px; font-size: 11px; color: var(--mh-muted); margin-bottom: 4px; align-items: center; }
.mh-pressure-title { color: var(--color-text); font-weight: 600; }
.mh-swatch { display: inline-block; width: 9px; height: 9px; border-radius: 2px; margin-right: 5px; vertical-align: -1px; }
.mh-swatch-cut { background: var(--mh-cut); }
.mh-swatch-war { background: var(--mh-war); }
.mh-swatch-trim { background: var(--mh-trim); }
.mh-swatch-sales { height: 2px; width: 14px; background: var(--mh-sales); }
.mh-swatch-baseline { height: 0; width: 14px; border-top: 2px dashed var(--mh-baseline); }
.mh-swatch-churn { background: var(--mh-churn); }
.mh-swatch-calm { background: var(--mh-calm); }
.mh-pressure-crosshair { stroke: var(--color-text-muted); stroke-width: 1; stroke-dasharray: 3 3; }
```

- [ ] **Step 4: Run tests and check**

Run: `cargo test -p ultros-ui-charts undercut_pressure` → 4 passed. Then `cargo check -p ultros-ui-charts --features <ssr feature>` and `--features <hydrate feature>` (names from its `Cargo.toml` `[features]`) → clean.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-ui-charts/src/components/ ultros-frontend/ultros-i18n/locales/
git commit -m "feat(ui-charts): undercut pressure pane, cards and translations"
```

---

### Task 10: Wire into the chart and the item page

**Files:**
- Modify: `ultros-frontend/ultros-ui-charts/src/components/price_history_chart.rs` — `PriceHistoryChart` props (931), `model` memo options (1231), overlay render (~1971), `HoverTooltip` (859) and its instantiation
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs` — `ChartWrapper` resources (after `floor_error`, ~1087) and the view (~1225); `api::{…}` import (line 2)

**Interfaces:**
- Consumes: Tasks 7-9.
- Produces: `PriceHistoryChart` prop `#[prop(into, default = Signal::derive(|| None))] pressure: Signal<Option<UndercutPressure>>`.

- [ ] **Step 1: Chart component**
  1. Add the `pressure` prop after `floor`.
  2. In the `model` memo options: `war_spans: pressure.with(|p| p.as_ref().map(|p| p.wars.iter().map(|w| (w.start, w.end)).collect()).unwrap_or_default()),`
  3. Next to `hover_index`:
     ```rust
    let pane_sales = Memo::new(move |_| {
        resolved_series.with(|s| {
            let mut out: Vec<(i64, u32)> = s
                .series
                .iter()
                .flat_map(|e| &e.buckets)
                .map(|b| (b.ts.and_utc().timestamp(), b.sales))
                .collect();
            out.sort_unstable_by_key(|(ts, _)| *ts);
            out
        })
    });
    let pane_domain = Signal::derive(move || model.with(|m| m.time_domain));
    let pane_width = Signal::derive(move || model.with(|m| m.scene.width));
    let pane_hover_x = Signal::derive(move || {
        hover_index.get().and_then(|i| model.with(|m| m.hover.buckets.get(i).map(|b| b.x)))
    });
     ```
     (`resolved_series` is what `model` reads; if it is an `Option<PriceSeries>`, add `.as_ref().map(…).unwrap_or_default()`.)
  4. In the overlay branch (`view! { <svg …>{scene_view(&m.scene)}<HoverLayer …/></svg> }`), render the pane right after `</svg>` inside the same pointer container, so the container's pointer handlers (which map x by width fraction) drive the shared `hover_index` over the pane too:
     ```rust
                        <Show when=move || mode.get() != ChartMode::Density>
                            <crate::components::undercut_pressure::UndercutPressurePane
                                pressure=pressure
                                sales=pane_sales
                                time_domain=pane_domain
                                width=pane_width
                                hover_x=pane_hover_x
                            />
                        </Show>
     ```
     The branch is already overlay-only (grid view renders elsewhere). Wrap the two siblings in a fragment `<>…</>` if the `view!` needs a single root.
  5. `HoverTooltip`: add `#[prop(into)] pressure: Signal<Option<UndercutPressure>>`; after the quantity row:
     ```rust
                        {move || pressure.with(|p| {
                            let p = p.as_ref()?;
                            let b = crate::components::undercut_pressure::pressure_bucket_at(p, bucket.ts)?;
                            let state = match b.state {
                                PressureState::War => t_string!(i18n, undercut_pressure_state_war),
                                PressureState::Churn => t_string!(i18n, undercut_pressure_state_churn),
                                PressureState::Calm => t_string!(i18n, undercut_pressure_state_calm),
                                PressureState::Unknown => return None,
                            }
                            .to_string();
                            let total = b.trims + b.cuts;
                            Some(view! {
                                <div class="mt-1 border-t border-[color:var(--color-outline)]/60 pt-1 text-[color:var(--color-text-muted)]">
                                    <div class="font-semibold text-[color:var(--color-text)]">{state}</div>
                                    <div>{t_string!(i18n, undercut_pressure_tooltip_undercuts, total = total, cuts = b.cuts, trims = b.trims).to_string()}</div>
                                    {(b.sellers > 0).then(|| view! {
                                        <div>{t_string!(i18n, undercut_pressure_tooltip_sellers, sellers = b.sellers).to_string()}</div>
                                    })}
                                </div>
                            })
                        })}
     ```
     and pass `pressure=pressure` where `<HoverTooltip …/>` is used. Import `PressureState` and `UndercutPressure` from `ultros_api_types::undercut_pressure`.

- [ ] **Step 2: Item page** — in `ChartWrapper`, after `floor_error`:

```rust
    // Undercut pressure is per world (retainers only compete on their own
    // world) and its bars share the price chart's time axis, so it is
    // fetched only at world scope, in time-axis modes, once the price series
    // has told us its bucket width.
    let world_data_scope = world_data.clone();
    let is_world_scope = Memo::new(move |_| {
        world.with(|w| {
            world_data_scope
                .lookup_world_by_name(&Url::unescape(w))
                .is_some_and(|scope| scope.as_world().is_some())
        })
    });
    let pressure_resource = LocalResource::new(move || {
        let active = is_world_scope.get() && mode.get() != ChartMode::Density;
        let bucket = series.with(|s| s.as_ref().map(|s| s.bucket_seconds));
        let id = item_id.get();
        let world_name = world.get();
        let quality = hq.get();
        let decision = debounced_decision.get();
        async move {
            let (true, Some(bucket), RangeDecision::Resolved(range)) = (active, bucket, decision) else {
                return None;
            };
            Some(get_undercut_pressure(id, &world_name, quality, range, bucket).await)
        }
    });
    // Gated again on read so a world → datacenter navigation drops the stale
    // world payload before the resource re-runs.
    let pressure = Signal::derive(move || {
        is_world_scope
            .get()
            .then(|| pressure_resource.get().flatten().and_then(|r| r.ok()))
            .flatten()
    });
    let pressure_error = Signal::derive(move || {
        is_world_scope.get() && pressure_resource.get().flatten().is_some_and(|r| r.is_err())
    });
```

Add `get_undercut_pressure` to the `api::{…}` import. In the view:

```rust
                                <MarketHistory sales=series floor=floor floor_error=floor_error scope=world>
                                <UndercutPressureCards pressure=pressure error=pressure_error />
                                <PriceHistoryChart
                                    series=series
                                    floor=floor
                                    pressure=pressure
                                    density=density
                                    ...the rest unchanged...
                                />
                                </MarketHistory>
```

with `use crate::components::undercut_pressure::UndercutPressureCards;` — `ultros-app` imports `PriceHistoryChart` via `crate::components::price_history_chart`; mirror however that module re-exports ui-charts components (add a `pub use ultros_ui_charts::components::undercut_pressure;` next to the existing re-export if needed).

- [ ] **Step 3: Build**

Run: `cargo check -p ultros-app --features hydrate` and `cargo check -p ultros-app --features ssr` → clean.

- [ ] **Step 4: Run the app and look** (the `run` skill / AGENTS.md local-serve recipe; `LEPTOS_SITE_ROOT` relative, set `METRICS_PORT`). In the browser pane, open `/item/<local world>/<item with listing events>`:
  - pane under the chart, bars aligned with the price chart's buckets; hovering shows the crosshair in both and the tooltip's pressure lines;
  - cards row directly under the existing stat row;
  - Density mode → pane gone, no `undercut_pressure` request in the network log;
  - the world's datacenter URL → no pane, no cards, no request;
  - HQ toggle → request carries `hq=hq`;
  - `data-theme="light"` → pane colors follow the light tokens.
  If local ClickHouse has no events (memory `reference_local_clickhouse_sales_broken_parts`), verify with Task 11's fixture probe and say so in the PR.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-ui-charts/src/components/price_history_chart.rs ultros-frontend/ultros-app/src/routes/item_view.rs ultros-frontend/ultros-app/src/components/
git commit -m "feat(item): show undercut pressure under the price chart at world scope"
```

---

### Task 11: E2E probe, calibration, CI, PR

**Files:**
- Create: `integration/undercut-pressure.cjs`
- Modify: `integration/package.json` (`"test:undercut-pressure": "node ./undercut-pressure.cjs"`), `scripts/run_e2e.sh` (run it next to `test:market-window`, same exit-code pattern)
- Create: `ultros-changelog/changes/<ship date>-undercut-pressure.json` (copy the newest file's shape; name it with the date it actually ships — a backdated entry never lights what's-new)

- [ ] **Step 1: Write the probe**

```js
// Undercut pressure renders at world scope and is never requested at datacenter scope.
const assert = require('node:assert/strict');
const puppeteer = require('puppeteer');
const BASE = process.env.BASE_URL || 'http://127.0.0.1:8080';
const ITEM = process.env.PRESSURE_ITEM || '5057';
const WORLD = process.env.PRESSURE_WORLD || 'Gilgamesh';
const DC = process.env.PRESSURE_DC || 'Aether';

const pressureBody = (url) => {
  const bucket = Number(url.searchParams.get('bucket') || 3600);
  const to = Number(url.searchParams.get('to') || Math.floor(Date.now() / 1000));
  const from = Number(url.searchParams.get('from') || to - 48 * 3600);
  const first = Math.floor(from / bucket) * bucket;
  const buckets = [];
  for (let start = first, i = 0; start < to; start += bucket, i++) {
    const war = i % 7 === 3;
    buckets.push({ start, trims: war ? 2 : 1, cuts: war ? 6 : 1, sellers: war ? 3 : 1,
      floor_open: 1000, floor_close: war ? 900 : 1000, state: war ? 'war' : 'churn' });
  }
  return { world_id: 0, from, to, bucket_seconds: bucket, coverage_from: first, baseline: 2, buckets, wars: [],
    summary: { floor_trend_24h: -0.12, war: { kind: 'active' },
      last_war: { start: to - 3600, end: to, undercuts: 8, sellers: 3, floor_change: -0.1 },
      contested_share: 1, typical_undercuts_per_hour: 2, floor_holds_median_secs: 2100,
      episodes_left: 5, episodes_undercut: 3 } };
};

async function main() {
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  page.setDefaultTimeout(30000);
  await page.setCookie({ name: 'HIDE_ADS', value: 'true', url: BASE });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  const requests = [];
  await page.setRequestInterception(true);
  page.on('request', request => {
    const url = new URL(request.url());
    if (url.pathname.startsWith('/api/v1/undercut_pressure/')) {
      requests.push(url.pathname + url.search);
      return request.respond({ status: 200, contentType: 'application/json', body: JSON.stringify(pressureBody(url)) });
    }
    return request.continue();
  });

  await page.goto(`${BASE}/item/${WORLD}/${ITEM}`, { waitUntil: 'networkidle2' });
  await page.waitForSelector('.mh-pressure-pane svg');
  await page.waitForSelector('.mh-pressure-stats');
  const cards = await page.$$eval('.mh-pressure-stats .mh-stat strong', els => els.map(e => e.textContent.trim()));
  assert.equal(cards.length, 4);
  assert.equal(cards[0], '-12.0%');
  assert.equal(cards[3], '63% / 37%');
  assert.ok(requests.some(r => r.startsWith(`/api/v1/undercut_pressure/${WORLD}/${ITEM}?`) && r.includes('bucket=')), requests.join('\n'));

  const before = requests.length;
  await page.goto(`${BASE}/item/${DC}/${ITEM}`, { waitUntil: 'networkidle2' });
  assert.equal(await page.$('.mh-pressure-pane'), null, 'no pane at datacenter scope');
  assert.equal(await page.$('.mh-pressure-stats'), null, 'no cards at datacenter scope');
  assert.equal(requests.length, before, `no pressure request at DC scope: ${requests.slice(before).join(', ')}`);

  assert.deepEqual(errors, []);
  await browser.close();
  console.log('undercut-pressure: ok');
}
main().catch(e => { console.error(e); process.exit(1); });
```

`cards[0]`: `percent(-0.12)` = `"-12.0%"` (Rust `{:+.1}` prints `-12.0%`). `cards[3]`: `outcome_split(5, 3)` = (63, 37).

- [ ] **Step 2: Run the probe** against a local server (`./scripts/run_e2e.sh`, or run the app and `cd integration && BASE_URL=http://127.0.0.1:8080 npm run test:undercut-pressure`), with `PRESSURE_ITEM` / `PRESSURE_WORLD` / `PRESSURE_DC` set to ones the local DB renders. Expected: `undercut-pressure: ok`. Environmental failures (memory `reference_e2e_driver_environmental_failures`; try `E2E_BLOCK_EXTERNAL=1`) go in the PR — do not weaken assertions.

- [ ] **Step 3: Calibrate thresholds read-only against prod** — ask Aaron before connecting. On the prod box (`ssh chew@boxbox`, read-only `clickhouse-client` SELECTs only), run Task 5's `events_sql` and the `exact_changes` SQL for 5 hot and 5 slow items on one busy world over the last 7 days; feed the rows through `pressure()` with a throwaway scratchpad test/bin (not committed). Record per item: share of War / Churn / Calm hourly buckets, war span count, floor-holds median. Change constants only if hot items show war in more than ~15% of hourly buckets, or known wars (visible as floor cliffs) are missed. Table goes in the PR body. If prod is not reachable, say so and keep the specced values.

- [ ] **Step 4: Full CI**

```bash
SCRATCH="C:/Users/chw11/AppData/Local/Temp/claude/C--Users-chw11-code-ultros--claude-worktrees-adoring-heyrovsky-a5002b/d64b4719-efe4-4a66-9d67-73e26da5a86b/scratchpad"
./check_ci.sh > "$SCRATCH/ci.log" 2>&1; echo "REAL_EXIT=$?"; tail -30 "$SCRATCH/ci.log"
```
Expected: `REAL_EXIT=0`. Fix clippy findings in code (no `#[allow]` without a justified comment). Exit 137 = OOM → `cargo clippy --all-targets -j 2 -- -D warnings`.

- [ ] **Step 5: Changelog, commit, PR**

```bash
git add integration/undercut-pressure.cjs integration/package.json scripts/run_e2e.sh ultros-changelog/changes/
git commit -m "test(e2e): undercut pressure at world scope, absent at datacenter scope"
git fetch origin main
git rebase origin/main
git push -u origin claude/undercut-pressure
gh pr create --title "Undercut pressure on the item page (world scope)" --body-file "$SCRATCH/pr-body.md"
```

PR body: what/why (link the spec), screenshots (world page dark + light, DC page without pane), the calibration table, which gated ClickHouse smokes ran, the E2E result, "phase 2 (market board) follows". Ends with `🤖 Generated with [Claude Code](https://claude.com/claude-code)`. Every commit message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

---

## Phase 2 (follow-on PR): per-world market board

Planned in detail after phase 1 merges (it reuses the calibrated classifier). Outline:

1. **`world_rows` reducer** in `undercut_pressure.rs`: per world, `classify` over `[now - DAY, now)` at `HOUR` with that world's anchor and floor points; `state_24h` = max bucket state; `war_sellers` = sellers of the last war span; `floor_holds_median_secs` from `episodes` over the same 24 h. Unit tests mirroring Task 4.
2. **Scope loader**: one `reprice_events_sql` read and one `exact_changes` read over all worlds in scope (`reprice_events_sql` already projects `world_id`; partition in Rust), plus `SELECT world_id, toInt64(max(event_time)) FROM listing_events WHERE kind = 'added' AND source != 'snapshot' AND item_id = ? AND world_id IN (…) GROUP BY world_id`. Gated smoke with two worlds.
3. **Endpoint** `GET /api/v1/undercut_pressure/scope/{scope}/{itemid}?hq=` → `ScopePressure`; same cache/semaphore/timeout; any scope accepted.
4. **Board component** replacing `WorldMarketShare` in `item_view.rs`: pure helpers (`board_rows`, `group_by_datacenter`, `default_expanded(home_world, groups)`, sort by floor with empty boards last) unit-tested; listing-derived columns (floor, supply, sellers) render immediately, pressure columns fill in; region scope groups by datacenter with the per-world state strip and "_n_ at war"; rows link to `/item/<World>/<id>` keeping `hq`/`range`; `< 640px` folds Sellers and Floor holds into the row tooltip; `world_board_*` keys in all seven locales; unused `market_share_*` keys removed from all seven.
5. **E2E**: DC and region pages render the board, region groups start collapsed except the home (or cheapest) DC, clicking a row navigates to the world page.
