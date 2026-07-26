# Server-side Sale Aggregation and LOD Rendering — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Feed the item-page price chart from a pre-bucketed ClickHouse aggregate instead of up to 10,000 raw sales, so full history is reachable and scene node count stops scaling with sale count.

**Architecture:** A new `/api/v1/price_series/{world}/{itemid}` endpoint runs a `GROUP BY` over the full-history `sales` table in ClickHouse and returns per-series OHLC + VWAP inputs + quantiles per time bucket. The chart layout in `ultros-charts` consumes that payload instead of `&[SaleHistory]`. A new batched `Node::Path` scene primitive collapses per-sale marks into one node per series. Raw sales still ship, but only when the window is small enough to draw them.

**Tech Stack:** Rust, axum, ClickHouse (`clickhouse` crate), Leptos 0.8 (SSR + WASM hydration), `ultros-charts` scene graph.

**Spec:** `docs/superpowers/specs/2026-07-26-chart-server-aggregation-design.md`

**Before committing anything:** run `./check_ci.sh` from the repo root (`cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`). If the `xiv-gen/ffxiv-datamining` submodule is not initialized, clippy cannot compile the workspace — at minimum run `cargo fmt --all -- --check` and note it. See `CLAUDE.md`.

---

## File Structure

**Create:**
- `ultros-api-types/src/price_series.rs` — wire types shared by server and WASM chart
- `ultros-clickhouse/tests/price_series_smoke.rs` — integration tests behind `ULTROS_CH_INTEGRATION`

**Modify:**
- `ultros-frontend/ultros-charts/src/scene.rs` — add `Node::Path`
- `ultros-frontend/ultros-charts/src/svg.rs` — serialize `Path`, add `dots_path_d`
- `ultros-frontend/ultros-charts/src/components.rs` — render `Path`
- `ultros-frontend/ultros-charts/src/data/buckets.rs` — bucket ladder helpers
- `ultros-frontend/ultros-charts/src/data/grouping.rs` — `GroupLevel` ⇄ `SeriesGroup`
- `ultros-frontend/ultros-charts/src/charts/price_history.rs` — build from `PriceSeries`
- `ultros-api-types/src/lib.rs` — module + re-exports
- `ultros-clickhouse/src/queries.rs` — `price_series` query
- `ultros/src/web.rs` — handler + route
- `ultros/src/web/state.rs` — cache handle
- `ultros-frontend/ultros-app/src/api.rs` — client fetch
- `ultros-frontend/ultros-app/src/components/price_history_chart.rs` — consume the resource
- `ultros-frontend/ultros-app/src/routes/item_view.rs` — drop the extended-sales workaround

---

### Task 1: `Node::Path` scene primitive

Batched marks: one node carrying many subpaths sharing a fill/stroke.

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/scene.rs`
- Modify: `ultros-frontend/ultros-charts/src/svg.rs:197-295` (tests), `:95-192` (match)
- Modify: `ultros-frontend/ultros-charts/src/components.rs:45-148`

- [ ] **Step 1: Write the failing test**

In `ultros-frontend/ultros-charts/src/svg.rs`, inside `mod tests`:

```rust
#[test]
fn serializes_path_with_fill_and_stroke() {
    let scene = Scene {
        width: 10.0,
        height: 10.0,
        background: None,
        font_family: "sans-serif".to_string(),
        nodes: vec![
            Node::Path {
                d: "M0 0L5 5".to_string(),
                fill: Some(Color::rgb(1, 2, 3).with_alpha(0.5)),
                stroke: None,
            },
            Node::Path {
                d: "M1 1L2 2".to_string(),
                fill: None,
                stroke: Some(Stroke {
                    color: Color::rgb(4, 5, 6),
                    width: 2.0,
                    dash: None,
                }),
            },
        ],
    };
    let svg = scene_to_svg(&scene);
    assert!(svg.contains(r##"<path d="M0 0L5 5" fill="#010203" fill-opacity="0.500""##));
    assert!(svg.contains(r##"<path d="M1 1L2 2" fill="none" stroke="#040506""##));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ultros-charts serializes_path_with_fill_and_stroke`
Expected: FAIL — `no variant named 'Path' found for enum 'Node'`

- [ ] **Step 3: Add the variant**

In `ultros-frontend/ultros-charts/src/scene.rs`, add to `enum Node` after the `Area` variant:

```rust
    /// Pre-serialized path data. Lets a layout emit N marks that share one
    /// fill or stroke as a single node instead of N nodes — the difference
    /// between 2,000 SVG elements and 1 for a dense chart.
    Path {
        d: String,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
```

- [ ] **Step 4: Add the SVG serialization arm**

In `ultros-frontend/ultros-charts/src/svg.rs`, in `scene_to_svg`'s match, after the `Node::Area` arm:

```rust
            Node::Path { d, fill, stroke } => {
                let _ = write!(out, r#"<path d="{d}""#);
                match fill {
                    Some(fill) => push_fill(&mut out, fill),
                    None => out.push_str(r#" fill="none""#),
                }
                if let Some(stroke) = stroke {
                    push_stroke(&mut out, stroke);
                }
                out.push_str("/>");
            }
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p ultros-charts serializes_path_with_fill_and_stroke`
Expected: PASS

- [ ] **Step 6: Write the failing Leptos renderer test**

In `ultros-frontend/ultros-charts/src/components.rs`, inside `mod tests`:

```rust
#[test]
fn renders_path_nodes() {
    let scene = Scene {
        width: 10.0,
        height: 10.0,
        background: None,
        font_family: "sans-serif".to_string(),
        nodes: vec![Node::Path {
            d: "M0 0L5 5".to_string(),
            fill: Some(Color::rgb(1, 2, 3)),
            stroke: None,
        }],
    };
    let html = scene_view(&scene).to_html();
    assert!(html.contains(r#"d="M0 0L5 5""#), "{html}");
    assert!(html.contains("#010203"), "{html}");
}
```

- [ ] **Step 7: Run to verify it fails**

Run: `cargo test -p ultros-charts --features leptos renders_path_nodes`
Expected: FAIL — non-exhaustive match in `node_view`

- [ ] **Step 8: Add the Leptos arm**

In `ultros-frontend/ultros-charts/src/components.rs`, in `node_view`'s match, after the `Node::Area` arm:

```rust
        Node::Path { d, fill, stroke } => view! {
            <path
                d=d.clone()
                fill=fill
                    .as_ref()
                    .map(color_attr)
                    .unwrap_or_else(|| "none".to_string())
                stroke=stroke.as_ref().map(|s| color_attr(&s.color))
                stroke-width=stroke.as_ref().map(|s| px(s.width))
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-dasharray=stroke.as_ref().and_then(dash_attr)
            />
        }
        .into_any(),
```

- [ ] **Step 9: Run both tests**

Run: `cargo test -p ultros-charts --features leptos`
Expected: PASS, all tests

- [ ] **Step 10: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-charts/src/scene.rs ultros-frontend/ultros-charts/src/svg.rs ultros-frontend/ultros-charts/src/components.rs
git commit -m "feat(charts): add batched Node::Path scene primitive"
```

---

### Task 2: `dots_path_d` helper

Turn a point list into one path of circles, so the raw-sale layer is one node.

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/svg.rs`

- [ ] **Step 1: Write the failing test**

In `ultros-frontend/ultros-charts/src/svg.rs`, inside `mod tests`:

```rust
#[test]
fn dots_path_emits_one_subpath_per_point() {
    let d = dots_path_d(&[(10.0, 20.0), (30.0, 40.0)], 2.0).unwrap();
    assert_eq!(d.matches('M').count(), 2, "one move per dot: {d}");
    assert!(d.starts_with("M8.0 20.0a2.0,2.0"), "{d}");
    assert_eq!(dots_path_d(&[], 2.0), None);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ultros-charts dots_path_emits_one_subpath_per_point`
Expected: FAIL — `cannot find function 'dots_path_d'`

- [ ] **Step 3: Implement**

In `ultros-frontend/ultros-charts/src/svg.rs`, after `area_path_d`:

```rust
/// Path data drawing a filled circle of radius `r` at each point, as one
/// path with one subpath per dot. Two half-arcs per circle is the standard
/// way to express a circle in path syntax, and usvg handles it fine.
/// `None` for an empty input (an empty `d` attribute is invalid SVG).
pub(crate) fn dots_path_d(points: &[(f32, f32)], r: f32) -> Option<String> {
    if points.is_empty() {
        return None;
    }
    let mut d = String::with_capacity(points.len() * 40);
    for (x, y) in points {
        let left = x - r;
        let diameter = r * 2.0;
        let _ = write!(
            d,
            "M{left:.1} {y:.1}a{r:.1},{r:.1} 0 1,0 {diameter:.1},0a{r:.1},{r:.1} 0 1,0 -{diameter:.1},0"
        );
    }
    Some(d)
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ultros-charts dots_path_emits_one_subpath_per_point`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-charts/src/svg.rs
git commit -m "feat(charts): add dots_path_d for batched sale dots"
```

---

### Task 3: `PriceSeries` wire types

**Files:**
- Create: `ultros-api-types/src/price_series.rs`
- Modify: `ultros-api-types/src/lib.rs:1-28`

- [ ] **Step 1: Write the failing test**

Create `ultros-api-types/src/price_series.rs` with only this test module at the bottom (implementation comes in step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn bucket(gil: i64, units: i64) -> PriceBucket {
        PriceBucket {
            ts: chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc(),
            open: 1,
            high: 1,
            low: 1,
            close: 1,
            gil,
            units,
            sales: 1,
            p25: 1,
            p50: 1,
            p75: 1,
        }
    }

    #[test]
    fn vwap_divides_gil_by_units() {
        assert_eq!(bucket(1000, 4).vwap(), Some(250.0));
    }

    #[test]
    fn vwap_is_none_without_units() {
        assert_eq!(bucket(1000, 0).vwap(), None);
    }

    #[test]
    fn series_group_round_trips_through_json() {
        let json = serde_json::to_string(&SeriesGroup::Datacenter).unwrap();
        assert_eq!(json, "\"datacenter\"");
        assert_eq!(
            serde_json::from_str::<SeriesGroup>(&json).unwrap(),
            SeriesGroup::Datacenter
        );
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ultros-api-types price_series`
Expected: FAIL — `file not found for module 'price_series'` (module not declared yet)

- [ ] **Step 3: Write the types**

At the top of `ultros-api-types/src/price_series.rs`, above the test module:

```rust
//! Pre-bucketed price/volume series — the chart's data source.
//!
//! Buckets carry `gil` and `units` rather than a precomputed VWAP so a
//! consumer can re-derive VWAP over any subset of buckets with exact
//! integer arithmetic (the timeline slicer needs this).
//!
//! `open`/`high`/`low`/`close` and the quantiles are computed server-side at
//! the requested [`SeriesGroup`]. Quantiles are deliberately *not*
//! re-aggregatable client-side: a datacenter's p50 is not any function of
//! its worlds' p50s, which is why grouping is a request parameter rather
//! than a client-side transform.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::CompactSale;

/// Which level of the world hierarchy the server aggregated at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeriesGroup {
    Region,
    Datacenter,
    World,
}

impl SeriesGroup {
    /// Stable identifier for query strings and cache keys.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Region => "region",
            Self::Datacenter => "datacenter",
            Self::World => "world",
        }
    }
}

/// Quality filter applied server-side, replacing the old client-side
/// `retain(|s| s.hq)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HqFilter {
    #[default]
    Any,
    Hq,
    Nq,
}

impl HqFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Hq => "hq",
            Self::Nq => "nq",
        }
    }
}

/// One time bucket for one series.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceBucket {
    /// Bucket start, naive UTC, aligned to absolute time.
    pub ts: NaiveDateTime,
    /// Price of the earliest sale in the bucket.
    pub open: i32,
    pub high: i32,
    pub low: i32,
    /// Price of the latest sale in the bucket.
    pub close: i32,
    /// Sum of `price_per_item * quantity`.
    pub gil: i64,
    /// Sum of `quantity`.
    pub units: i64,
    /// Number of sale rows, *not* units. Drives sparse-bucket handling.
    pub sales: u32,
    pub p25: i32,
    pub p50: i32,
    pub p75: i32,
}

impl PriceBucket {
    /// Volume-weighted average price. `None` when the bucket moved no units,
    /// which the caller should render as a gap rather than a zero.
    pub fn vwap(&self) -> Option<f64> {
        (self.units > 0).then(|| self.gil as f64 / self.units as f64)
    }
}

/// All buckets for one series, keyed by the selector id at the response's
/// [`SeriesGroup`] (world id, datacenter id, or region id).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceSeriesEntry {
    pub id: i32,
    /// Sorted by `ts` ascending. Buckets with no sales are absent, not zero —
    /// consumers must handle gaps.
    pub buckets: Vec<PriceBucket>,
}

/// Response payload for `/api/v1/price_series/{world}/{itemid}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceSeries {
    /// Bucket width the server actually chose, so the client labels axes
    /// without re-deriving it.
    pub bucket_seconds: i64,
    pub group: SeriesGroup,
    /// Time domain actually covered by the data, not the requested range.
    pub from: NaiveDateTime,
    pub to: NaiveDateTime,
    pub series: Vec<PriceSeriesEntry>,
    /// Raw sales, present only when the window holds few enough of them to
    /// draw individually. See `RAW_SALE_LIMIT` in the web handler.
    pub raw: Option<Vec<CompactSale>>,
}

impl PriceSeries {
    /// True when every series is empty — the "No recent sales" case.
    pub fn is_empty(&self) -> bool {
        self.series.iter().all(|s| s.buckets.is_empty())
    }
}
```

- [ ] **Step 4: Declare the module and re-export**

In `ultros-api-types/src/lib.rs`, add to the module list (alphabetical, after `pub mod market_pulse;`):

```rust
pub mod price_series;
```

and add to the re-export block after the `sale_history` re-export:

```rust
pub use price_series::{HqFilter, PriceBucket, PriceSeries, PriceSeriesEntry, SeriesGroup};
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p ultros-api-types price_series`
Expected: PASS, 3 tests

- [ ] **Step 6: Commit**

```bash
./check_ci.sh
git add ultros-api-types/src/price_series.rs ultros-api-types/src/lib.rs
git commit -m "feat(api-types): add PriceSeries wire types"
```

---

### Task 4: Bucket ladder helpers

The server and the client must agree on bucket widths exactly, or bucket boundaries drift between the axis labels and the data.

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/data/buckets.rs:14-28`

- [ ] **Step 1: Write the failing test**

In `ultros-frontend/ultros-charts/src/data/buckets.rs`, inside `mod tests`:

```rust
#[test]
fn snap_rounds_to_the_nearest_ladder_step_not_below_it() {
    assert_eq!(snap_bucket_seconds(1), HOUR);
    assert_eq!(snap_bucket_seconds(HOUR), HOUR);
    assert_eq!(snap_bucket_seconds(2 * HOUR), 6 * HOUR);
    assert_eq!(snap_bucket_seconds(DAY), DAY);
    assert_eq!(snap_bucket_seconds(i64::MAX), 30 * DAY);
}

#[test]
fn widen_walks_up_the_ladder_and_stops_at_the_top() {
    assert_eq!(widen_bucket(HOUR), Some(6 * HOUR));
    assert_eq!(widen_bucket(6 * HOUR), Some(DAY));
    assert_eq!(widen_bucket(30 * DAY), None);
}

#[test]
fn span_picks_the_same_width_as_the_days_based_helper() {
    // 30 days of data with no explicit range: both paths must agree, or the
    // server and client bucket differently.
    assert_eq!(
        bucket_seconds_for_span(30 * DAY),
        bucket_seconds(None, 30)
    );
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ultros-charts buckets::`
Expected: FAIL — `cannot find function 'snap_bucket_seconds'`

- [ ] **Step 3: Implement**

In `ultros-frontend/ultros-charts/src/data/buckets.rs`, after the existing `bucket_seconds` function:

```rust
/// The only bucket widths this system produces. The server snaps requested
/// widths onto this ladder so a hand-crafted request cannot ask for a
/// million buckets, and so client-side axis labelling always matches the
/// server's bucketing.
pub const BUCKET_LADDER: [i64; 5] = [HOUR, 6 * HOUR, DAY, 7 * DAY, 30 * DAY];

/// Snap an arbitrary width up to the next ladder step. Values above the top
/// clamp to the widest bucket.
pub fn snap_bucket_seconds(requested: i64) -> i64 {
    BUCKET_LADDER
        .iter()
        .copied()
        .find(|step| *step >= requested)
        .unwrap_or(30 * DAY)
}

/// Next ladder step up, or `None` at the top. Used to widen rather than
/// truncate when a response would exceed the bucket cap.
pub fn widen_bucket(current: i64) -> Option<i64> {
    let snapped = snap_bucket_seconds(current);
    BUCKET_LADDER.iter().copied().find(|step| *step > snapped)
}

/// Bucket width for a time span expressed in seconds — the server's entry
/// point. Delegates to [`bucket_seconds`] so both callers share one ladder.
pub fn bucket_seconds_for_span(span_secs: i64) -> i64 {
    bucket_seconds(None, (span_secs / DAY).max(1))
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ultros-charts buckets::`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-charts/src/data/buckets.rs
git commit -m "feat(charts): share the bucket ladder with the server"
```

---

### Task 5: `GroupLevel` ⇄ `SeriesGroup` conversions

`GroupLevel` stays the chart's vocabulary (`available_group_levels` returns it); `SeriesGroup` is the wire vocabulary. One conversion, not two enums used interchangeably.

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/data/grouping.rs:29-47`

- [ ] **Step 1: Write the failing test**

In `ultros-frontend/ultros-charts/src/data/grouping.rs`, inside `mod tests`:

```rust
#[test]
fn group_level_round_trips_through_series_group() {
    for level in [GroupLevel::Region, GroupLevel::Datacenter, GroupLevel::World] {
        assert_eq!(GroupLevel::from(SeriesGroup::from(level)), level);
    }
}
```

and add to the test module's imports:

```rust
    use ultros_api_types::price_series::SeriesGroup;
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ultros-charts group_level_round_trips_through_series_group`
Expected: FAIL — `the trait bound 'SeriesGroup: From<GroupLevel>' is not satisfied`

- [ ] **Step 3: Implement**

In `ultros-frontend/ultros-charts/src/data/grouping.rs`, after the `impl GroupLevel` block:

```rust
impl From<GroupLevel> for ultros_api_types::price_series::SeriesGroup {
    fn from(level: GroupLevel) -> Self {
        match level {
            GroupLevel::Region => Self::Region,
            GroupLevel::Datacenter => Self::Datacenter,
            GroupLevel::World => Self::World,
        }
    }
}

impl From<ultros_api_types::price_series::SeriesGroup> for GroupLevel {
    fn from(group: ultros_api_types::price_series::SeriesGroup) -> Self {
        use ultros_api_types::price_series::SeriesGroup;
        match group {
            SeriesGroup::Region => Self::Region,
            SeriesGroup::Datacenter => Self::Datacenter,
            SeriesGroup::World => Self::World,
        }
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ultros-charts group_level_round_trips_through_series_group`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-charts/src/data/grouping.rs
git commit -m "feat(charts): convert between GroupLevel and SeriesGroup"
```

---

### Task 6: ClickHouse `price_series` query

**Files:**
- Modify: `ultros-clickhouse/src/queries.rs`

- [ ] **Step 1: Write the failing unit test for the grouping expression**

In `ultros-clickhouse/src/queries.rs`, add a `mod tests` at the bottom of the file (create it if absent):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::price_series::SeriesGroup;

    #[test]
    fn world_group_selects_the_column_directly() {
        assert_eq!(group_expr(SeriesGroup::World, &[(1, 10), (2, 10)]), "world_id");
    }

    #[test]
    fn coarser_groups_build_a_transform_map() {
        assert_eq!(
            group_expr(SeriesGroup::Datacenter, &[(1, 10), (2, 10), (3, 20)]),
            "transform(world_id, [1,2,3], [10,10,20], 0)"
        );
    }

    #[test]
    fn hq_predicate_is_empty_for_any() {
        assert_eq!(hq_predicate(HqFilter::Any), "");
        assert_eq!(hq_predicate(HqFilter::Hq), " AND hq = 1");
        assert_eq!(hq_predicate(HqFilter::Nq), " AND hq = 0");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ultros-clickhouse queries::tests`
Expected: FAIL — `cannot find function 'group_expr'`

- [ ] **Step 3: Implement the row type and SQL builders**

In `ultros-clickhouse/src/queries.rs`, add near the top imports:

```rust
use ultros_api_types::price_series::{HqFilter, SeriesGroup};
```

and add at the end of the file, above the test module:

```rust
/// One aggregated bucket for one series. Column order matches the SELECT.
#[derive(Debug, Clone, Row, Deserialize)]
pub struct PriceSeriesRow {
    /// Selector id at the requested grouping.
    pub series_id: i32,
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub bucket: chrono::DateTime<chrono::Utc>,
    pub open: u32,
    pub high: u32,
    pub low: u32,
    pub close: u32,
    pub gil: u64,
    pub units: u64,
    pub sales: u64,
    pub p25: u32,
    pub p50: u32,
    pub p75: u32,
}

/// SQL expression producing the series key.
///
/// For coarser groupings we inline a `transform()` map from the caller's
/// world list rather than joining a lookup table — the world set is small
/// (at most a region's worth) and a join would take this query off the
/// `sales` primary-key prefix.
fn group_expr(group: SeriesGroup, world_to_group: &[(i32, i32)]) -> String {
    if group == SeriesGroup::World {
        return "world_id".to_string();
    }
    let from = world_to_group
        .iter()
        .map(|(w, _)| w.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let to = world_to_group
        .iter()
        .map(|(_, g)| g.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("transform(world_id, [{from}], [{to}], 0)")
}

fn hq_predicate(hq: HqFilter) -> &'static str {
    match hq {
        HqFilter::Any => "",
        HqFilter::Hq => " AND hq = 1",
        HqFilter::Nq => " AND hq = 0",
    }
}

/// Aggregate `sales` into fixed-width buckets for one item.
///
/// `world_to_group` maps every world in scope to its series key at `group`;
/// for `SeriesGroup::World` the mapped value is ignored.
///
/// Deliberately no `FINAL`. `sales` is a `ReplacingMergeTree` whose duplicates
/// are exact repeats of the same sale, and at aggregate scale an unmerged
/// duplicate shifts a bucket's VWAP imperceptibly — whereas `FINAL` over a
/// full-history scan is expensive. This is an accuracy-for-cost trade.
///
/// Deliberately no join: `item_id` is filtered first so the read stays on the
/// table's `(item_id, hq, world_id, sold_date, pg_id)` prefix. See the comment
/// in [`sparklines_batch`] for what happens when a large table is joined
/// unfiltered.
#[allow(clippy::too_many_arguments)]
pub async fn price_series(
    ch: &ClickHouseClient,
    item_id: i32,
    world_to_group: &[(i32, i32)],
    group: SeriesGroup,
    hq: HqFilter,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    bucket_seconds: i64,
) -> Result<Vec<PriceSeriesRow>, ClickHouseError> {
    if world_to_group.is_empty() {
        return Ok(Vec::new());
    }
    let worlds = world_to_group
        .iter()
        .map(|(w, _)| w.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let key = group_expr(group, world_to_group);
    let hq_filter = hq_predicate(hq);

    let sql = format!(
        r#"
        SELECT
            toInt32({key})                              AS series_id,
            toStartOfInterval(sold_date, INTERVAL {bucket_seconds} SECOND) AS bucket,
            toUInt32(argMin(price_per_item, sold_date)) AS open,
            toUInt32(max(price_per_item))               AS high,
            toUInt32(min(price_per_item))               AS low,
            toUInt32(argMax(price_per_item, sold_date)) AS close,
            toUInt64(sum(total_gil))                    AS gil,
            toUInt64(sum(quantity))                     AS units,
            toUInt64(count())                           AS sales,
            toUInt32(quantileExact(0.25)(price_per_item)) AS p25,
            toUInt32(quantileExact(0.50)(price_per_item)) AS p50,
            toUInt32(quantileExact(0.75)(price_per_item)) AS p75
        FROM sales
        WHERE item_id = {item_id}
          AND world_id IN ({worlds})
          AND sold_date >= toDateTime({from_ts})
          AND sold_date <  toDateTime({to_ts}){hq_filter}
        GROUP BY series_id, bucket
        ORDER BY series_id, bucket
        "#,
        from_ts = from.timestamp(),
        to_ts = to.timestamp(),
    );

    Ok(ch.client().query(&sql).fetch_all::<PriceSeriesRow>().await?)
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p ultros-clickhouse queries::tests`
Expected: PASS, 3 tests

- [ ] **Step 5: Commit**

```bash
./check_ci.sh
git add ultros-clickhouse/src/queries.rs
git commit -m "feat(clickhouse): add price_series bucketed aggregate query"
```

---

### Task 7: ClickHouse integration smoke tests

These are the tests that prove the migration does not shift prices. They run only with `ULTROS_CH_INTEGRATION` set, matching every other test in this crate.

**Files:**
- Create: `ultros-clickhouse/tests/price_series_smoke.rs`

- [ ] **Step 1: Write the tests**

Create `ultros-clickhouse/tests/price_series_smoke.rs`:

```rust
//! Integration tests for the price_series aggregate.
//!
//! Run with a throwaway ClickHouse:
//!   docker run --rm -d -p 8123:8123 -e CLICKHOUSE_DB=ultros \
//!     -e CLICKHOUSE_USER=ultros -e CLICKHOUSE_PASSWORD= \
//!     --name ch-test clickhouse/clickhouse-server
//!   ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test price_series_smoke

use ultros_api_types::price_series::{HqFilter, SeriesGroup};
use ultros_clickhouse::{ClickHouseClient, queries, rows::SaleRow};

fn integration_enabled() -> bool {
    std::env::var("ULTROS_CH_INTEGRATION").is_ok()
}

fn load_env() {
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();
}

/// Item id far outside the real range so fixtures never collide with
/// backfilled production data in a shared dev ClickHouse.
const FIXTURE_ITEM: i32 = 999_000_001;

fn ts(secs: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(secs, 0).unwrap()
}

/// Base timestamp aligned to a day boundary, so bucket assignment in the
/// assertions is unambiguous.
const T0: i64 = 1_700_006_400; // 2023-11-15 00:00:00 UTC

async fn seed(ch: &ClickHouseClient) {
    ch.client()
        .query("DELETE FROM sales WHERE item_id = ?")
        .bind(FIXTURE_ITEM)
        .execute()
        .await
        .expect("clear fixtures");

    // Deliberately inserted out of chronological order so argMin/argMax are
    // proven to key on sold_date rather than on insertion order.
    let rows = [
        // (pg_id, offset_secs, world_id, price, qty, hq)
        (1, 3_600, 1, 300u32, 2u16, 0u8),
        (2, 0, 1, 100, 1, 0),
        (3, 7_200, 1, 200, 1, 1),
        (4, 1_800, 2, 500, 4, 0),
    ];
    let mut insert = ch.client().insert::<SaleRow>("sales").await.expect("insert");
    for (pg_id, offset, world_id, price, quantity, hq) in rows {
        insert
            .write(&SaleRow {
                pg_id,
                sold_date: ts(T0 + offset),
                item_id: FIXTURE_ITEM,
                hq,
                world_id,
                price_per_item: price,
                quantity,
                buying_character_id: 0,
                buyer_name: String::new(),
            })
            .await
            .expect("write");
    }
    insert.end().await.expect("end insert");
}

#[tokio::test]
async fn ohlc_keys_on_sold_date_not_insertion_order() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    seed(&ch).await;

    let rows = queries::price_series(
        &ch,
        FIXTURE_ITEM,
        &[(1, 10), (2, 10)],
        SeriesGroup::World,
        HqFilter::Any,
        ts(T0),
        ts(T0 + 86_400),
        86_400,
    )
    .await
    .expect("query");

    let world1 = rows.iter().find(|r| r.series_id == 1).expect("world 1");
    assert_eq!(world1.open, 100, "earliest sale by sold_date");
    assert_eq!(world1.close, 200, "latest sale by sold_date");
    assert_eq!(world1.high, 300);
    assert_eq!(world1.low, 100);
    assert_eq!(world1.sales, 3);
}

#[tokio::test]
async fn gil_and_units_reproduce_vwap() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    seed(&ch).await;

    let rows = queries::price_series(
        &ch,
        FIXTURE_ITEM,
        &[(1, 10), (2, 10)],
        SeriesGroup::World,
        HqFilter::Any,
        ts(T0),
        ts(T0 + 86_400),
        86_400,
    )
    .await
    .expect("query");

    // World 1: 100*1 + 300*2 + 200*1 = 900 gil over 4 units = 225.
    let world1 = rows.iter().find(|r| r.series_id == 1).expect("world 1");
    assert_eq!(world1.gil, 900);
    assert_eq!(world1.units, 4);
    assert_eq!(world1.gil as f64 / world1.units as f64, 225.0);
}

#[tokio::test]
async fn datacenter_grouping_merges_worlds_and_recomputes_quantiles() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    seed(&ch).await;

    let rows = queries::price_series(
        &ch,
        FIXTURE_ITEM,
        &[(1, 10), (2, 10)],
        SeriesGroup::Datacenter,
        HqFilter::Any,
        ts(T0),
        ts(T0 + 86_400),
        86_400,
    )
    .await
    .expect("query");

    assert_eq!(rows.len(), 1, "both worlds collapse into one datacenter");
    let dc = &rows[0];
    assert_eq!(dc.series_id, 10);
    assert_eq!(dc.high, 500, "max across both worlds");
    assert_eq!(dc.sales, 4);
    // p50 over [100,200,300,500] is 300 — not derivable from either world's
    // own median (200 and 500). This is the regression guard for grouping
    // being a server-side concern.
    assert_eq!(dc.p50, 300);
}

#[tokio::test]
async fn hq_filter_narrows_the_result() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    seed(&ch).await;

    let rows = queries::price_series(
        &ch,
        FIXTURE_ITEM,
        &[(1, 10), (2, 10)],
        SeriesGroup::World,
        HqFilter::Hq,
        ts(T0),
        ts(T0 + 86_400),
        86_400,
    )
    .await
    .expect("query");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].sales, 1);
    assert_eq!(rows[0].open, 200);
}
```

- [ ] **Step 2: Run without the env var to confirm the skip path**

Run: `cargo test -p ultros-clickhouse --test price_series_smoke`
Expected: PASS, 4 tests, each printing "skipped: set ULTROS_CH_INTEGRATION=1 to run"

- [ ] **Step 3: Run against a throwaway ClickHouse**

```bash
docker run --rm -d -p 8123:8123 -e CLICKHOUSE_DB=ultros -e CLICKHOUSE_USER=ultros -e CLICKHOUSE_PASSWORD= --name ch-test clickhouse/clickhouse-server
```

Then: `ULTROS_CH_INTEGRATION=1 CLICKHOUSE_URL=http://localhost:8123 cargo test -p ultros-clickhouse --test price_series_smoke`
Expected: PASS, 4 tests

If `argMin`/`argMax` return unexpected values, the likely cause is the `sales` `ReplacingMergeTree` holding an unmerged duplicate from a previous run — the `DELETE` in `seed` should prevent that, but `OPTIMIZE TABLE sales FINAL` will force a merge while debugging.

- [ ] **Step 4: Tear down and commit**

```bash
docker stop ch-test
./check_ci.sh
git add ultros-clickhouse/tests/price_series_smoke.rs
git commit -m "test(clickhouse): integration coverage for price_series"
```

---

### Task 8: Web endpoint

**Files:**
- Modify: `ultros/src/web.rs` (handler near `extended_sale_history` at `:256`; route near `:1515`)

- [ ] **Step 1: Add the handler**

In `ultros/src/web.rs`, after the `ExtendedHistoryQuery` struct:

```rust
#[derive(serde::Deserialize, Debug)]
struct PriceSeriesQuery {
    from: Option<i64>,
    to: Option<i64>,
    bucket: Option<i64>,
    group: Option<String>,
    hq: Option<String>,
}

/// Above this many sales in the window we stop shipping raw rows and the
/// chart draws buckets only. Raw dots become a zoomed-in affordance rather
/// than a default — which is the entire point of this endpoint.
const RAW_SALE_LIMIT: u64 = 2_000;

/// Hard ceiling on buckets across all series in one response. Exceeding it
/// widens the bucket a ladder step and retries rather than truncating —
/// truncation would silently drop the oldest data, which is the data the
/// caller asked for.
const MAX_BUCKETS: usize = 20_000;

/// Pre-bucketed price/volume series for the item chart. Aggregated in
/// ClickHouse so payload size tracks the requested window rather than the
/// item's popularity.
#[tracing::instrument(skip(db, ch, world_cache))]
async fn price_series(
    State(db): State<UltrosDb>,
    State(ch): State<ultros_clickhouse::ClickHouseClient>,
    State(world_cache): State<Arc<WorldCache>>,
    Path((world, item_id)): Path<(String, i32)>,
    axum::extract::Query(query): axum::extract::Query<PriceSeriesQuery>,
) -> Result<axum::Json<ultros_api_types::price_series::PriceSeries>, WebError> {
    use ultros_api_types::price_series::{
        HqFilter, PriceBucket, PriceSeries, PriceSeriesEntry, SeriesGroup,
    };
    use ultros_charts::data::buckets::{bucket_seconds_for_span, snap_bucket_seconds, widen_bucket};

    let selected = world_cache.lookup_value_by_name(&world)?;
    let worlds = world_cache
        .get_all_worlds_in(&selected)
        .ok_or_else(|| Error::msg("Unable to get worlds"))?;

    let group = match query.group.as_deref() {
        Some("region") => SeriesGroup::Region,
        Some("datacenter") => SeriesGroup::Datacenter,
        _ => SeriesGroup::World,
    };
    let hq = match query.hq.as_deref() {
        Some("hq") => HqFilter::Hq,
        Some("nq") => HqFilter::Nq,
        _ => HqFilter::Any,
    };

    let now = chrono::Utc::now();
    let to = query
        .to
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .unwrap_or(now);
    // Default `from` reaches back far enough to cover any plausible history;
    // the response's own `from`/`to` report what the data actually spans.
    let from = query
        .from
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .unwrap_or_else(|| now - chrono::TimeDelta::days(365 * 12));
    if from >= to {
        return Err(Error::msg("from must be before to").into());
    }

    let world_to_group = world_group_map(&world_cache, &worlds, group);

    let span = (to - from).num_seconds();
    let mut bucket = match query.bucket {
        Some(requested) if requested > 0 => snap_bucket_seconds(requested),
        _ => bucket_seconds_for_span(span),
    };

    let rows = loop {
        let rows = ultros_clickhouse::queries::price_series(
            &ch,
            item_id,
            &world_to_group,
            group,
            hq,
            from,
            to,
            bucket,
        )
        .await
        .inspect_err(|e| tracing::error!(error = ?e, "price_series query failed"))?;
        if rows.len() <= MAX_BUCKETS {
            break rows;
        }
        match widen_bucket(bucket) {
            Some(wider) => {
                tracing::debug!(from = bucket, to = wider, "widening bucket to fit cap");
                bucket = wider;
            }
            None => break rows,
        }
    };

    // Group rows into series, preserving the query's ORDER BY.
    let mut series: Vec<PriceSeriesEntry> = Vec::new();
    for row in &rows {
        let entry = match series.last_mut() {
            Some(entry) if entry.id == row.series_id => entry,
            _ => {
                series.push(PriceSeriesEntry {
                    id: row.series_id,
                    buckets: Vec::new(),
                });
                series.last_mut().expect("just pushed")
            }
        };
        entry.buckets.push(PriceBucket {
            ts: row.bucket.naive_utc(),
            open: row.open as i32,
            high: row.high as i32,
            low: row.low as i32,
            close: row.close as i32,
            gil: row.gil as i64,
            units: row.units as i64,
            sales: row.sales as u32,
            p25: row.p25 as i32,
            p50: row.p50 as i32,
            p75: row.p75 as i32,
        });
    }

    let total_sales: u64 = rows.iter().map(|r| r.sales).sum();
    let raw = if total_sales <= RAW_SALE_LIMIT && total_sales > 0 {
        let sales = db
            .get_compact_sale_history(worlds.iter().copied(), item_id, RAW_SALE_LIMIT)
            .await
            .inspect_err(|e| tracing::error!(error = ?e, "raw sales fetch failed"))?;
        Some(
            sales
                .into_iter()
                .filter(|s| {
                    let ts = s.sold_date.and_utc();
                    ts >= from && ts < to
                })
                .map(|s| ultros_api_types::CompactSale {
                    quantity: s.quantity,
                    price_per_item: s.price_per_item,
                    hq: s.hq,
                    sold_date: s.sold_date,
                    world_id: s.world_id,
                })
                .collect(),
        )
    } else {
        None
    };

    let data_from = series
        .iter()
        .filter_map(|s| s.buckets.first().map(|b| b.ts))
        .min()
        .unwrap_or_else(|| from.naive_utc());
    let data_to = series
        .iter()
        .filter_map(|s| s.buckets.last().map(|b| b.ts))
        .max()
        .unwrap_or_else(|| to.naive_utc());

    Ok(axum::Json(PriceSeries {
        bucket_seconds: bucket,
        group,
        from: data_from,
        to: data_to,
        series,
        raw,
    }))
}

/// Map every world in scope to its series key at `group`.
///
/// `WorldCache::get_all_worlds_in` yields bare world ids, so coarser groupings
/// resolve each one through the cache. Worlds that fail to resolve are dropped
/// rather than mapped to a sentinel — a sentinel would silently merge them into
/// one bogus series.
fn world_group_map(
    world_cache: &WorldCache,
    world_ids: &[i32],
    group: ultros_api_types::price_series::SeriesGroup,
) -> Vec<(i32, i32)> {
    use ultros_api_types::price_series::SeriesGroup;
    use ultros_db::world_data::world_cache::AnySelector;

    if group == SeriesGroup::World {
        return world_ids.iter().map(|&id| (id, id)).collect();
    }
    world_ids
        .iter()
        .filter_map(|&world_id| {
            let result = world_cache
                .lookup_selector(&AnySelector::World(world_id))
                .ok()?;
            let world = result.as_world().ok()?;
            let key = match group {
                SeriesGroup::World => world.id,
                SeriesGroup::Datacenter => world.datacenter_id,
                SeriesGroup::Region => world_cache
                    .lookup_selector(&AnySelector::Datacenter(world.datacenter_id))
                    .ok()?
                    .as_datacenter()
                    .ok()?
                    .region_id,
            };
            Some((world_id, key))
        })
        .collect()
}
```

**Types confirmed against the source, do not re-derive them:**
`WorldCache::get_all_worlds_in(&AnyResult) -> Option<Vec<i32>>`
(`ultros-db/src/world_data/world_cache.rs:344`),
`WorldCache::lookup_selector(&AnySelector) -> Result<AnyResult, WorldCacheError>`
(`:304`), and `AnyResult::as_world() -> Result<&world::Model, WorldCacheError>`
(`:83`). Note `lookup_selector` takes the selector **by reference** and returns
`Result`, unlike `WorldHelper`'s same-named method in `ultros-api-types` which
takes by value and returns `Option`. Both appear in this codebase; the chart
crate uses the `WorldHelper` one.

Adjust the `world_to_group` call in the handler accordingly:

```rust
    let world_to_group = world_group_map(&world_cache, &worlds, group);
```

where `worlds` is the `Vec<i32>` from `get_all_worlds_in` — the same value
`extended_sale_history` passes to `get_compact_sale_history` at `:271`.

- [ ] **Step 2: Register the route**

In `ultros/src/web.rs`, next to the `extended_history` route:

```rust
        .route(
            "/api/v1/price_series/{world}/{itemid}",
            get(price_series),
        )
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p ultros`
Expected: clean

- [ ] **Step 4: Smoke test by hand**

Start the server, then:

```bash
curl -s 'http://localhost:8080/api/v1/price_series/Gilgamesh/5057?group=world' | head -c 400
```

Expected: JSON with `bucket_seconds`, `group":"world"`, and a `series` array.

- [ ] **Step 5: Commit**

```bash
./check_ci.sh
git add ultros/src/web.rs
git commit -m "feat(api): add /api/v1/price_series endpoint"
```

---

### Task 9: Response caching

**Files:**
- Create: `ultros/src/web/price_series_cache.rs`
- Modify: `ultros/src/web/state.rs`, `ultros/src/web.rs`

- [ ] **Step 1: Write the failing test**

Create `ultros/src/web/price_series_cache.rs` with only this test module (implementation in step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn key(item: i32) -> CacheKey {
        CacheKey {
            item_id: item,
            scope: "Gilgamesh".to_string(),
            from: 0,
            to: 100,
            bucket: 3600,
            group: "world",
            hq: "any",
        }
    }

    #[test]
    fn returns_a_stored_value_within_ttl() {
        let cache = PriceSeriesCache::new(4);
        cache.insert(key(1), "a".to_string(), Duration::from_secs(60));
        assert_eq!(cache.get(&key(1)), Some("a".to_string()));
        assert_eq!(cache.get(&key(2)), None);
    }

    #[test]
    fn expired_entries_are_not_returned() {
        let cache = PriceSeriesCache::new(4);
        cache.insert(key(1), "a".to_string(), Duration::from_secs(0));
        assert_eq!(cache.get(&key(1)), None);
    }

    #[test]
    fn insert_past_capacity_evicts_rather_than_growing_forever() {
        let cache = PriceSeriesCache::new(2);
        for i in 0..5 {
            cache.insert(key(i), i.to_string(), Duration::from_secs(60));
        }
        assert!(cache.len() <= 2, "capacity must bound the map");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ultros price_series_cache`
Expected: FAIL — module not declared

- [ ] **Step 3: Implement**

At the top of `ultros/src/web/price_series_cache.rs`:

```rust
//! Small in-process TTL cache for `/api/v1/price_series` responses.
//!
//! Values are already-serialized JSON strings: the endpoint's cost is the
//! ClickHouse scan plus serialization, and caching the string skips both.
//!
//! Deliberately not an LRU. Eviction on overflow clears expired entries first
//! and then drops arbitrary ones — for a cache whose job is absorbing bursts
//! of identical requests, exact recency ordering is not worth the bookkeeping.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CacheKey {
    pub item_id: i32,
    pub scope: String,
    pub from: i64,
    pub to: i64,
    pub bucket: i64,
    pub group: &'static str,
    pub hq: &'static str,
}

#[derive(Clone)]
pub(crate) struct PriceSeriesCache {
    inner: Arc<Mutex<HashMap<CacheKey, (Instant, String)>>>,
    capacity: usize,
}

impl PriceSeriesCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            capacity,
        }
    }

    pub fn get(&self, key: &CacheKey) -> Option<String> {
        let map = self.inner.lock().ok()?;
        let (expires_at, value) = map.get(key)?;
        (*expires_at > Instant::now()).then(|| value.clone())
    }

    pub fn insert(&self, key: CacheKey, value: String, ttl: Duration) {
        let Ok(mut map) = self.inner.lock() else {
            return;
        };
        if map.len() >= self.capacity {
            let now = Instant::now();
            map.retain(|_, (expires_at, _)| *expires_at > now);
            while map.len() >= self.capacity {
                let Some(victim) = map.keys().next().cloned() else {
                    break;
                };
                map.remove(&victim);
            }
        }
        map.insert(key, (Instant::now() + ttl, value));
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.lock().map(|m| m.len()).unwrap_or(0)
    }
}

impl Default for PriceSeriesCache {
    fn default() -> Self {
        Self::new(512)
    }
}
```

- [ ] **Step 4: Declare the module**

In `ultros/src/web.rs` (or `ultros/src/web/mod.rs`, wherever the `web` submodules are declared — check how `country_code_decoder` is declared and follow it):

```rust
mod price_series_cache;
```

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p ultros price_series_cache`
Expected: PASS, 3 tests

- [ ] **Step 6: Wire into `WebState`**

In `ultros/src/web/state.rs`, add the field next to `ch_client`:

```rust
    /// Absorbs bursts of identical chart requests. See
    /// [`crate::web::price_series_cache`].
    pub(crate) price_series_cache: crate::web::price_series_cache::PriceSeriesCache,
```

Initialize it with `PriceSeriesCache::default()` wherever `WebState` is
constructed, and add a `FromRef<WebState> for PriceSeriesCache` impl mirroring
the existing `FromRef` impl for `ClickHouseClient` at `:118`.

- [ ] **Step 7: Use it in the handler**

In `ultros/src/web.rs`, in `price_series`: add
`State(cache): State<crate::web::price_series_cache::PriceSeriesCache>` to the
handler arguments. After resolving `group`, `hq`, `from`, `to` and the initial
`bucket`, normalise and check:

```rust
    // Snap an open-ended `to` down to the current bucket boundary so live
    // views share a cache entry instead of minting a unique key per second.
    let to = if query.to.is_none() {
        let secs = to.timestamp() - to.timestamp().rem_euclid(bucket);
        chrono::DateTime::from_timestamp(secs, 0).unwrap_or(to)
    } else {
        to
    };

    let cache_key = crate::web::price_series_cache::CacheKey {
        item_id,
        scope: world.clone(),
        from: from.timestamp(),
        to: to.timestamp(),
        bucket,
        group: group.as_str(),
        hq: hq.as_str(),
    };
    // A closed window is immutable; an open one only changes when the current
    // bucket rolls over.
    let ttl = if query.to.is_some() {
        std::time::Duration::from_secs(3_600)
    } else {
        std::time::Duration::from_secs((bucket as u64).clamp(60, 3_600))
    };
    if let Some(hit) = cache.get(&cache_key) {
        return Ok(cached_json(hit, ttl));
    }
```

Change the return type to `Result<axum::response::Response, WebError>`, and at
the end serialize once, store, and return:

```rust
    let body = serde_json::to_string(&payload).map_err(Error::from)?;
    cache.insert(cache_key, body.clone(), ttl);
    Ok(cached_json(body, ttl))
```

with this helper beside the handler:

```rust
/// JSON response carrying a `Cache-Control` matching the in-process TTL, so
/// the browser and any CDN absorb repeats too.
fn cached_json(body: String, ttl: std::time::Duration) -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        [
            (axum::http::header::CONTENT_TYPE, "application/json".to_string()),
            (
                axum::http::header::CACHE_CONTROL,
                format!("public, max-age={}", ttl.as_secs()),
            ),
        ],
        body,
    )
        .into_response()
}
```

`snap_bucket_seconds` must run before the cache key is built, so the widening
loop cannot produce two keys for one logical request. If the loop widens the
bucket, store under the *widened* key as well as returning it — the response
reports `bucket_seconds`, so the client re-requests consistently thereafter.

- [ ] **Step 8: Verify**

Run: `cargo check -p ultros && cargo test -p ultros price_series_cache`
Expected: clean, 3 tests pass

Then by hand: request the same URL twice and confirm the second is materially
faster and carries `Cache-Control: public, max-age=...`.

```bash
curl -s -D- -o /dev/null 'http://localhost:8080/api/v1/price_series/Gilgamesh/5057' | grep -i cache-control
```

- [ ] **Step 9: Commit**

```bash
./check_ci.sh
git add ultros/src/web/price_series_cache.rs ultros/src/web/state.rs ultros/src/web.rs
git commit -m "feat(api): cache price_series responses in-process and via Cache-Control"
```

---

### Task 10: Chart layout consumes `PriceSeries`

The largest task. `build_price_history_chart` keeps its name, options and return
type; only its data input changes.

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/charts/price_history.rs:168-534` and its tests
- Modify: `ultros-frontend/ultros-charts/src/test_util.rs`

- [ ] **Step 1: Add a `PriceSeries` fixture**

In `ultros-frontend/ultros-charts/src/test_util.rs`:

```rust
use ultros_api_types::price_series::{PriceBucket, PriceSeries, PriceSeriesEntry, SeriesGroup};

pub(crate) fn bucket(ts_secs: i64, open: i32, high: i32, low: i32, close: i32, units: i64) -> PriceBucket {
    PriceBucket {
        ts: ts(ts_secs),
        open,
        high,
        low,
        close,
        gil: i64::from(close) * units,
        units,
        sales: 3,
        p25: low,
        p50: (low + high) / 2,
        p75: high,
    }
}

/// Two worlds of one datacenter, 10 daily buckets each, gently trending up.
pub(crate) fn two_world_series() -> PriceSeries {
    let entry = |id: i32, base: i32| PriceSeriesEntry {
        id,
        buckets: (0..10)
            .map(|i| {
                let p = base + i * 10;
                bucket(1_700_006_400 + i as i64 * 86_400, p, p + 20, p - 10, p + 5, 2)
            })
            .collect(),
    };
    PriceSeries {
        bucket_seconds: 86_400,
        group: SeriesGroup::World,
        from: ts(1_700_006_400),
        to: ts(1_700_006_400 + 9 * 86_400),
        series: vec![entry(1, 1_000), entry(2, 1_200)],
        raw: None,
    }
}
```

- [ ] **Step 2: Write the failing tests**

Replace the body of the existing tests in
`ultros-frontend/ultros-charts/src/charts/price_history.rs` that construct
`Vec<SaleHistory>` with the fixture. Start with one:

```rust
#[test]
fn renders_one_vwap_line_per_series() {
    let model = build_price_history_chart(
        &world_helper(),
        &two_world_series(),
        &PriceChartOptions::default(),
    );
    let polylines = model
        .scene
        .nodes
        .iter()
        .filter(|n| matches!(n, Node::Polyline { .. }))
        .count();
    assert_eq!(polylines, 2);
    assert_eq!(model.series.len(), 2);
    assert_eq!(
        model.series.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        vec!["Gilgamesh", "Adamantoise"]
    );
}

#[test]
fn raw_dots_are_one_path_node_per_series_when_present() {
    let mut series = two_world_series();
    series.raw = Some(vec![
        ultros_api_types::CompactSale {
            quantity: 1,
            price_per_item: 1_050,
            hq: false,
            sold_date: ts(1_700_006_400),
            world_id: 1,
        };
        500
    ]);
    let model = build_price_history_chart(
        &world_helper(),
        &series,
        &PriceChartOptions::default(),
    );
    let paths = model
        .scene
        .nodes
        .iter()
        .filter(|n| matches!(n, Node::Path { .. }))
        .count();
    assert_eq!(paths, 1, "500 sales for one world collapse into one node");
    assert_eq!(
        model.scene.nodes.iter().filter(|n| matches!(n, Node::Circle { .. })).count(),
        0,
        "no per-sale circles"
    );
}

#[test]
fn absent_raw_sales_draw_no_dot_layer() {
    let model = build_price_history_chart(
        &world_helper(),
        &two_world_series(),
        &PriceChartOptions::default(),
    );
    assert_eq!(
        model.scene.nodes.iter().filter(|n| matches!(n, Node::Path { .. })).count(),
        0
    );
}

#[test]
fn empty_series_renders_the_no_data_card() {
    let series = PriceSeries {
        bucket_seconds: 86_400,
        group: SeriesGroup::World,
        from: ts(0),
        to: ts(0),
        series: Vec::new(),
        raw: None,
    };
    let model =
        build_price_history_chart(&world_helper(), &series, &PriceChartOptions::default());
    assert!(model.hover.buckets.is_empty());
    assert!(model.stats.is_none());
    assert!(
        model.scene.nodes.iter().any(
            |n| matches!(n, Node::Text { content, .. } if content == "No recent sales")
        )
    );
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p ultros-charts price_history`
Expected: FAIL — `build_price_history_chart` expects `&[SaleHistory]`

- [ ] **Step 4: Change the signature and series resolution**

In `ultros-frontend/ultros-charts/src/charts/price_history.rs`, change:

```rust
pub fn build_price_history_chart(
    world_helper: &WorldHelper,
    series: &PriceSeries,
    options: &PriceChartOptions,
) -> PriceChartModel {
```

Replace the `filter_outliers` / `auto_group_level` / `group_sales_by_level`
block (currently `:182-207`) with:

```rust
    let theme = &options.theme;
    let level = GroupLevel::from(series.group);
    // Resolve each series id to its display name via the world hierarchy.
    // Ids that no longer resolve (a world removed from the world list between
    // the sale and now) are dropped rather than rendered as a bare number.
    let named: Vec<(&PriceSeriesEntry, String)> = series
        .series
        .iter()
        .filter_map(|entry| {
            let selector = match series.group {
                SeriesGroup::World => AnySelector::World(entry.id),
                SeriesGroup::Datacenter => AnySelector::Datacenter(entry.id),
                SeriesGroup::Region => AnySelector::Region(entry.id),
            };
            let name = world_helper.lookup_selector(selector)?.get_name().to_string();
            Some((entry, name))
        })
        .collect();

    let is_hidden = |name: &str| options.hidden_series.iter().any(|h| h == name);
    let series_info: Vec<SeriesInfo> = named
        .iter()
        .enumerate()
        .map(|(index, (_, name))| SeriesInfo {
            name: name.clone(),
            color: theme.palette[index % theme.palette.len()],
            hidden: is_hidden(name),
        })
        .collect();
    let visible = || {
        named
            .iter()
            .enumerate()
            .filter(|(index, _)| !series_info[*index].hidden)
    };
    let visible_count = visible().count();
```

Derive the domains from buckets rather than points:

```rust
    let Some((first_ts, last_ts)) = visible()
        .flat_map(|(_, (entry, _))| entry.buckets.iter().map(|b| b.ts))
        .minmax()
        .into_option()
    else {
        /* existing "No recent sales" early return, with group_level: level */
    };
    let (min_price, max_price) = visible()
        .flat_map(|(_, (entry, _))| entry.buckets.iter().flat_map(|b| [b.low, b.high]))
        .minmax()
        .into_option()
        .expect("non-empty by the timestamp check above");
```

Stats come from bucket aggregates:

```rust
    let stats = {
        let gil: i64 = visible()
            .flat_map(|(_, (entry, _))| entry.buckets.iter())
            .map(|b| b.gil)
            .sum();
        let units: i64 = visible()
            .flat_map(|(_, (entry, _))| entry.buckets.iter())
            .map(|b| b.units)
            .sum();
        let n: usize = visible()
            .flat_map(|(_, (entry, _))| entry.buckets.iter())
            .map(|b| b.sales as usize)
            .sum();
        // Median of per-bucket medians. The exact per-sale median is no
        // longer computable client-side; each bucket's p50 is exact, and this
        // rolls those up. Documented as an approximation because it is one —
        // buckets are weighted equally regardless of how many sales they hold.
        let medians: Vec<i32> = visible()
            .flat_map(|(_, (entry, _))| entry.buckets.iter())
            .map(|b| b.p50)
            .collect();
        Some(ChartStats {
            n,
            market_average: (units > 0).then(|| (gil / units) as i32),
            median: median(&medians),
            min: min_price,
            max: max_price,
        })
    };
```

`median` in `data/stats.rs:19` takes `&[i32]` and sorts a copy internally via
`select_nth_unstable`, so `medians` needs no pre-sorting and no `mut`.

Replace `vwap_buckets(&group.points, bucket_secs)` in the line-drawing loop with
the entry's own buckets, using `bucket.vwap()` and skipping `None`. Bucket
centres for x are `b.ts + bucket_seconds / 2`, matching the old `VwapPoint`
semantics. Volume bars come from summing `units` per bucket across visible
series instead of `volume_buckets_from_points`.

Replace the raw-dot loop (currently `:350-365`) with:

```rust
    if let Some(raw) = &series.raw {
        for (index, (entry, _)) in visible() {
            let color = series_color(index);
            // Raw sales carry world_id; at coarser groupings match through
            // the same selector the series was resolved from.
            let points: Vec<(f32, f32)> = raw
                .iter()
                .filter(|sale| series_id_for(world_helper, series.group, sale.world_id) == Some(entry.id))
                .map(|sale| {
                    (
                        time.scale(sale.sold_date),
                        price.scale(sale.price_per_item as f64),
                    )
                })
                .collect();
            if let Some(d) = crate::svg::dots_path_d(&points, 2.0) {
                scene.nodes.push(Node::Path {
                    d,
                    fill: Some(color.with_alpha(0.35)),
                    stroke: None,
                });
            }
        }
    }
```

with this helper above `build_price_history_chart`:

```rust
/// Which series a raw sale belongs to at the given grouping.
fn series_id_for(
    world_helper: &WorldHelper,
    group: SeriesGroup,
    world_id: i32,
) -> Option<i32> {
    let world = world_helper
        .lookup_selector(AnySelector::World(world_id))
        .and_then(|r| r.as_world())?;
    Some(match group {
        SeriesGroup::World => world.id,
        SeriesGroup::Datacenter => world.datacenter_id,
        SeriesGroup::Region => world_helper
            .lookup_selector(AnySelector::Datacenter(world.datacenter_id))
            .and_then(|dc| dc.as_datacenter().map(|dc| dc.region_id))?,
    })
}
```

`dots_path_d` is `pub(crate)` — that is sufficient, both modules are in this
crate.

Finally, drop `remove_outliers` handling from this function. The option stays on
`PriceChartOptions` (the frontend still owns the toggle) but is applied by the
caller to `raw` only; add a doc comment on the field saying so.

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p ultros-charts`
Expected: PASS. Tests that referenced `sale()`/`SaleHistory` in this module must
all be migrated to the fixture; delete any that tested grouping behaviour now
owned by the server (`renders_lines_dots_volume_and_labels`'s legend assertions
stay, its dot-count assertion becomes the `Path` assertion above).

- [ ] **Step 6: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-charts/src/charts/price_history.rs ultros-frontend/ultros-charts/src/test_util.rs
git commit -m "refactor(charts): build price history from PriceSeries"
```

---

### Task 11: Fix the item card PNG path

Task 10 changed a signature the PNG path calls. It must keep working.

**Files:**
- Modify: `ultros/src/web.rs` (the `item_card` handler)

- [ ] **Step 1: Find the call site**

Run: `grep -n "build_price_history_scene\|build_price_history_chart" ultros/src/web.rs`

- [ ] **Step 2: Feed it from the same aggregate**

The card renders a bounded recent window, so it can call the same helper the
endpoint does. Extract the body of `price_series` from "resolve worlds" through
"build `PriceSeries`" into:

```rust
/// Shared by the JSON endpoint and the item-card PNG so the two can never
/// disagree about what the chart shows.
async fn build_price_series(
    db: &UltrosDb,
    ch: &ultros_clickhouse::ClickHouseClient,
    world_cache: &WorldCache,
    world: &str,
    item_id: i32,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    group: ultros_api_types::price_series::SeriesGroup,
    hq: ultros_api_types::price_series::HqFilter,
    bucket: Option<i64>,
) -> Result<ultros_api_types::price_series::PriceSeries, WebError>
```

Have `price_series` call it, and have `item_card` call it with a 30-day window
and `SeriesGroup::World`, passing the result to `build_price_history_scene`.

- [ ] **Step 3: Verify the card still renders**

Run: `cargo check -p ultros`, then request `/itemcard/Gilgamesh/5057` and confirm
a PNG comes back with a plotted line.

- [ ] **Step 4: Commit**

```bash
./check_ci.sh
git add ultros/src/web.rs
git commit -m "refactor(api): render the item card from the shared price series"
```

---

### Task 12: Frontend API client

**Files:**
- Modify: `ultros-frontend/ultros-app/src/api.rs:53-67`

- [ ] **Step 1: Add the fetch function**

In `ultros-frontend/ultros-app/src/api.rs`, after `get_extended_sale_history`:

```rust
/// Pre-bucketed price series for the item chart. Unlike
/// [`get_extended_sale_history`] the payload size tracks the requested window
/// rather than the item's sale count, so this is safe at full history.
pub(crate) async fn get_price_series(
    item_id: i32,
    world: &str,
    group: SeriesGroup,
    hq: HqFilter,
    range: Option<(i64, i64)>,
) -> AppResult<PriceSeries> {
    if item_id == 0 {
        return Err(AppError::NoItem);
    }
    let mut url = format!(
        "/api/v1/price_series/{world}/{item_id}?group={}&hq={}",
        group.as_str(),
        hq.as_str()
    );
    if let Some((from, to)) = range {
        url.push_str(&format!("&from={from}&to={to}"));
    }
    fetch_api(&url).await
}
```

and add to the `ultros_api_types` import block:

```rust
    price_series::{HqFilter, PriceSeries, SeriesGroup},
```

- [ ] **Step 2: Verify**

Run: `cargo check -p ultros-app`
Expected: clean

- [ ] **Step 3: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/api.rs
git commit -m "feat(app): add price_series API client"
```

---

### Task 13: Rewire the chart component

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs:1236-1290, 1400-1415`

- [ ] **Step 1: Change the component's props**

In `ultros-frontend/ultros-app/src/components/price_history_chart.rs`, change the
signature to take the resolved series plus the controls it still owns:

```rust
#[component]
pub fn PriceHistoryChart(
    #[prop(into)] series: Signal<Option<PriceSeries>>,
    #[prop(into)] scope_name: Signal<String>,
    #[prop(into)] hq: Signal<HqFilter>,
    set_group: WriteSignal<GroupLevel>,
    #[prop(into)] group: Signal<GroupLevel>,
) -> impl IntoView {
```

The `filter_outliers` prop is removed; outlier filtering now applies only to
`series.raw` and is handled by the caller.

- [ ] **Step 2: Replace the model memo**

```rust
    let model = Memo::new(move |_| {
        let width = chart_width.get();
        let height = (width * 0.56).clamp(300.0, 540.0);
        let series = series.get().unwrap_or_else(empty_price_series);
        build_price_history_chart(
            &helper_for_model,
            &series,
            &PriceChartOptions {
                width,
                height,
                remove_outliers: false,
                show_market_average: show_market_average.get(),
                show_trendline: show_trend.get(),
                show_volume: show_quantity.get(),
                show_legend: false,
                title: None,
                icon_data_uri: None,
                days_range: None,
                group_level: None,
                utc_offset_minutes: utc_offset.get(),
                hidden_series: hidden_series.get(),
                theme: Theme::site(),
            },
        )
    });
```

`group_level` becomes `None` because the payload's `group` field is now
authoritative — the chart no longer chooses.

Add beside it:

```rust
/// Stand-in while the series resource is loading, so the chart renders its
/// own empty state rather than the component unmounting and remounting.
fn empty_price_series() -> PriceSeries {
    PriceSeries {
        bucket_seconds: 3_600,
        group: SeriesGroup::World,
        from: chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc(),
        to: chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc(),
        series: Vec::new(),
        raw: None,
    }
}
```

- [ ] **Step 3: Rewrite `TimelineSlicer`'s histogram**

Replace `timeline_quantity_buckets(&sales, domain, 64)` with a version summing
`units` across all series' buckets. Replace the function:

```rust
fn timeline_quantity_buckets(
    series: &PriceSeries,
    domain: (i64, i64),
    bucket_count: usize,
) -> Vec<f64> {
    if bucket_count == 0 {
        return Vec::new();
    }
    let span = (domain.1 - domain.0).max(1) as f64;
    let mut buckets = vec![0.0; bucket_count];
    for entry in &series.series {
        for b in &entry.buckets {
            let ts = b.ts.and_utc().timestamp();
            if ts < domain.0 || ts > domain.1 {
                continue;
            }
            let offset = ((ts - domain.0) as f64 / span).clamp(0.0, 1.0);
            let index = ((offset * bucket_count as f64).floor() as usize).min(bucket_count - 1);
            buckets[index] += b.units.max(0) as f64;
        }
    }
    buckets
}
```

Update its unit test accordingly, replacing the `sale()` helper with two
single-bucket `PriceSeriesEntry` values whose `units` are 3 and 7.

- [ ] **Step 4: Move the range selection to the resource**

`selected_range` stays local state, but instead of filtering `visible_sales` it
becomes an input to the caller's resource. Export it via a prop callback:

```rust
    #[prop(into)] on_range_change: Callback<Option<(i64, i64)>>,
```

and call `on_range_change.run(next)` where `set_selected_range.set(...)` is
called today. Debounce in the caller, not here.

Delete `visible_sales` and `sales_time_domain`; `available_domain` becomes
`series.get().map(|s| (s.from.and_utc().timestamp(), s.to.and_utc().timestamp()))`.

- [ ] **Step 5: Rewire `item_view.rs`**

Delete the `extended_sales` / `extended_loading` / `extended_error` signals and
the `Effect` that populates them (`:1236-1285`). Replace with:

```rust
    let (selected_range, set_selected_range) = signal::<Option<(i64, i64)>>(None);
    let (group, set_group) = signal(GroupLevel::World);
    let hq = Signal::derive(move || if hq_only.get() { HqFilter::Hq } else { HqFilter::Any });

    let series_resource = LocalResource::new(move || {
        let id = item_id.get();
        let world_name = world.get();
        let group = SeriesGroup::from(group.get());
        let hq = hq.get();
        let range = selected_range.get();
        async move { get_price_series(id, &world_name, group, hq, range).await }
    });
```

Pass `series=Signal::derive(move || series_resource.get().and_then(|r| r.ok()))`
to `PriceHistoryChart`, along with `group`, `set_group`, `hq`, and
`on_range_change=Callback::new(move |r| set_selected_range.set(r))`.

`LocalResource` is correct here for the same reason it is used for
`item_stats_resource` — client-only, avoiding a hydration mismatch when the
resource resolves at different times on server and client.

- [ ] **Step 6: Verify**

Run: `cargo check -p ultros-app --target wasm32-unknown-unknown`
Expected: clean

- [ ] **Step 7: Manual check**

Build and run the app. On an item page confirm: the chart renders; the timeline
slicer redraws after a drag; switching HQ-only refetches; on a datacenter page
the group-by control offers two levels and switching refetches.

- [ ] **Step 8: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/components/price_history_chart.rs ultros-frontend/ultros-app/src/routes/item_view.rs
git commit -m "feat(app): drive the price chart from the price_series endpoint"
```

---

### Task 14: Debounce the timeline slicer

Dragging a handle currently fires a request per pointer move.

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs`

- [ ] **Step 1: Add the debounce**

`leptos-use` is already a dependency (`use_element_size` in the chart component).
Use its debounce:

```rust
    use leptos_use::signal_debounced;
    let debounced_range = signal_debounced(selected_range, 300.0);
```

and read `debounced_range` in the resource instead of `selected_range`. The
slicer's own rendering keeps reading `selected_range`, so the handles track the
pointer at full rate while only the fetch is debounced.

- [ ] **Step 2: Verify**

Run: `cargo check -p ultros-app --target wasm32-unknown-unknown`

Manually: drag a slicer handle across the track and confirm in the network tab
that one request fires after the drag settles, not dozens during it.

- [ ] **Step 3: Commit**

```bash
./check_ci.sh
git add ultros-frontend/ultros-app/src/routes/item_view.rs
git commit -m "perf(app): debounce timeline slicer refetches"
```

---

### Task 15: Remove the dead grouping path

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/data/grouping.rs`, `ultros-frontend/ultros-charts/src/data/buckets.rs`

- [ ] **Step 1: Find what is now unused**

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | grep -i "never used"`

Expect `group_sales_by_level`, `group_sales_by_scope`, `auto_group_level`,
`vwap_buckets`, `volume_buckets_from_points` and `filter_outliers` to be
flagged, depending on what Task 11 left calling them.

- [ ] **Step 2: Delete what is genuinely dead, keep what is not**

Keep: `available_group_levels` (drives which grouping the UI may request),
`bucket_seconds` and the ladder helpers (shared with the server), `Series` and
`SalePoint` if `sparkline.rs` still uses them.

Delete the functions no caller reaches, along with their tests. Do not add
`#[allow(dead_code)]` to keep them — per `CLAUDE.md`, silencing clippy is not the
fix.

- [ ] **Step 3: Verify**

Run: `cargo test --workspace` and `./check_ci.sh`
Expected: clean

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-charts/src/
git commit -m "refactor(charts): drop client-side grouping now owned by the server"
```

---

## Verification

Before considering the plan complete:

- [ ] `./check_ci.sh` passes from the repo root
- [ ] `cargo test --workspace` passes
- [ ] `ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse` passes against a real ClickHouse
- [ ] An item page renders the chart with the same visual output as before the change
- [ ] A four-year window on a busy item returns in reasonable time and does not
      produce more than a few hundred scene nodes — check with
      `document.querySelectorAll('.price-history-chart svg *').length` in the console
- [ ] `/itemcard/{world}/{id}` still returns a PNG with a plotted line
- [ ] `./scripts/run_e2e.sh` passes

## Notes for the implementer

- **The submodule.** `./check_ci.sh` runs clippy over the whole workspace, which
  needs `xiv-gen/ffxiv-datamining` initialized recursively. If it is not and you
  cannot initialize it, run `cargo fmt --all -- --check` at minimum and say so in
  the PR. See `CLAUDE.md`.
- **No new user-facing strings** in this plan. If you find yourself adding one,
  it needs a key in all seven locale files — which is a signal you have drifted
  into spec 2's scope.
- **Task 10 is the risky one.** It rewrites the core of a 853-line file with
  fifteen existing tests. Migrate the tests first, one at a time, and let them
  drive the rewrite rather than rewriting and then fixing tests.
