# ultros_charts PR 2 — Web Price Chart Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render the item-page price-history chart from the ultros_charts scene graph with full interactivity (crosshair, tooltip, legend, toggles), and remove leptos-chartistry from the workspace.

**Architecture:** The core gains `build_price_history_chart` returning a `PriceChartModel` (the existing `Scene` plus a `HoverModel` of bucket positions/values, series metadata, and stats). A new `leptos` feature in ultros-charts renders any `Scene` as SVG view nodes (`scene_view`). The app's `price_history_chart.rs` shrinks to wiring: it builds the model in a memo (responsive width, viewer-timezone labels via a post-hydration offset signal — the established hydration-gate pattern), embeds `scene_view`, and adds the hover overlay, HTML tooltip, controls, stats strip, and legend with the existing i18n keys. Spec: `docs/superpowers/specs/2026-06-09-ultros-charts-design.md`.

**Tech Stack:** Rust edition 2024, leptos 0.8 (workspace, nightly feature), leptos-use (`use_element_bounding`), chrono. No new third-party deps.

---

## Context for the implementer

- Workspace root: `C:\Users\chw11\code\ultros`. PR 1 already landed: `ultros-frontend/ultros-charts` is the scene-graph crate (36 tests; `charts/price_history.rs` builds the `Scene`, `svg.rs` serializes it for the server PNG path in `ultros/src/web/item_card.rs` — that path must keep working untouched).
- The current web chart is `ultros-frontend/ultros-app/src/components/price_history_chart.rs` (~1050 lines on leptos-chartistry). Its only instantiation is `ultros-frontend/ultros-app/src/routes/item_view.rs` (ChartWrapper, which owns the 7/30/90/All window selector, the outlier-filter toggle, and the `hydrated`-gated time cutoff — **item_view.rs does not change in this PR**; the component props stay identical).
- **Hydration safety is a hard requirement.** This codebase has a history of tachys `hydration.rs:227` panics from SSR/CSR divergence. The rules this plan follows: anything viewer-dependent (timezone, measured container width) starts at a deterministic default (offset 0 / width 960) for both SSR and first client render, and only changes via `Effect`/leptos-use signals after hydration. Do not introduce `Local::now()`/`Utc::now()`/measurements into the initial render path.
- **i18n:** this PR introduces NO new user-visible strings — every label reuses existing keys (`chart_toggle_market_avg`, `chart_legend_trend`, `chart_legend_quantity`, `chart_legend_market_avg`, `chart_color_*`, `chart_stat_*`, `chart_no_sales_in_window`, `chart_aria_label`, `chart_range_*`). If you find yourself adding a new string, STOP: per CLAUDE.md it must be added to all 7 locale files with real translations — report it instead of improvising.
- Tests per task: `cargo test -p ultros-charts` (fast). Full `./check_ci.sh` only at the end.
- The working tree has unrelated dirt (`.jules/palette.md`, `.mcp.json`, the universalis-assets submodule pointer). NEVER `git add -A`; stage explicit paths. Commit messages end with `-m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"`.
- Branch for this PR: `ultros-charts-web` (created in Task 1).

### Task 1: Grouping levels in core

The web chart lets the user override the grouping level (Region/Datacenter/World); the core currently only auto-picks. Move the level-based grouping (and the scope-based availability rule) from the app into `data/grouping.rs`, refactoring `group_sales_by_scope` to be the auto wrapper.

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/data/grouping.rs`

- [ ] **Step 1: Create the branch**

```bash
git checkout -b ultros-charts-web
```

- [ ] **Step 2: Write the failing tests**

Add to the test module in `data/grouping.rs`:

```rust
    #[test]
    fn explicit_level_overrides_auto() {
        // Sales on one DC would auto-group by world; force datacenter level.
        let sales = vec![sale(100, 1, 1, ts(0)), sale(200, 1, 2, ts(10))];
        let series = group_sales_by_level(&world_helper(), &sales, GroupLevel::Datacenter);
        assert_eq!(names(&series), vec!["Aether"]);
        assert_eq!(series[0].points.len(), 2);
        let series = group_sales_by_level(&world_helper(), &sales, GroupLevel::Region);
        assert_eq!(names(&series), vec!["North-America"]);
    }

    #[test]
    fn auto_level_matches_scope_cascade() {
        let h = world_helper();
        // one DC → world level
        assert_eq!(
            auto_group_level(&h, &[sale(1, 1, 1, ts(0)), sale(1, 1, 2, ts(0))]),
            GroupLevel::World
        );
        // two DCs, one region → datacenter level
        assert_eq!(
            auto_group_level(&h, &[sale(1, 1, 1, ts(0)), sale(1, 1, 3, ts(0))]),
            GroupLevel::Datacenter
        );
        // two regions → region level
        assert_eq!(
            auto_group_level(&h, &[sale(1, 1, 1, ts(0)), sale(1, 1, 4, ts(0))]),
            GroupLevel::Region
        );
    }

    #[test]
    fn available_levels_follow_the_viewed_scope() {
        let h = world_helper();
        assert_eq!(available_group_levels(&h, "Gilgamesh"), vec![GroupLevel::World]);
        assert_eq!(
            available_group_levels(&h, "Aether"),
            vec![GroupLevel::Datacenter, GroupLevel::World]
        );
        assert_eq!(
            available_group_levels(&h, "North-America"),
            vec![GroupLevel::Region, GroupLevel::Datacenter, GroupLevel::World]
        );
        assert_eq!(
            available_group_levels(&h, "Not A Scope"),
            vec![GroupLevel::Region, GroupLevel::Datacenter, GroupLevel::World]
        );
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p ultros-charts grouping`
Expected: FAIL to compile (`GroupLevel` etc. not found).

- [ ] **Step 4: Write the implementation**

In `data/grouping.rs`, add below the `Series` struct (new imports: `std::collections::BTreeMap`):

```rust
/// Which level of the world hierarchy to roll sales up to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupLevel {
    Region,
    Datacenter,
    World,
}

impl GroupLevel {
    /// Stable identifier (list keys / debugging); user-facing names come
    /// from the app's i18n layer.
    pub fn label(self) -> &'static str {
        match self {
            Self::Region => "Region",
            Self::Datacenter => "Datacenter",
            Self::World => "World",
        }
    }
}

/// Group sales at an explicit hierarchy level. Sales whose world id isn't in
/// the helper are dropped. Series sort by name; points by timestamp.
pub fn group_sales_by_level(
    world_helper: &WorldHelper,
    sales: &[SaleHistory],
    level: GroupLevel,
) -> Vec<Series> {
    let mut groups = BTreeMap::<AnySelector, Series>::new();
    for sale in sales {
        let Some(world) = world_helper
            .lookup_selector(AnySelector::World(sale.world_id))
            .and_then(|r| r.as_world())
        else {
            continue;
        };
        let selector = match level {
            GroupLevel::World => AnySelector::World(world.id),
            GroupLevel::Datacenter => AnySelector::Datacenter(world.datacenter_id),
            GroupLevel::Region => {
                let Some(datacenter) = world_helper
                    .lookup_selector(AnySelector::Datacenter(world.datacenter_id))
                    .and_then(|r| r.as_datacenter())
                else {
                    continue;
                };
                AnySelector::Region(datacenter.region_id)
            }
        };
        let Some(result) = world_helper.lookup_selector(selector) else {
            continue;
        };
        groups
            .entry(selector)
            .or_insert_with(|| Series {
                name: result.get_name().to_string(),
                points: Vec::new(),
            })
            .points
            .push(SalePoint {
                ts: sale.sold_date,
                price: sale.price_per_item,
                quantity: sale.quantity,
            });
    }
    let mut series: Vec<Series> = groups
        .into_values()
        .sorted_by_cached_key(|series| series.name.clone())
        .collect();
    for series in &mut series {
        series.points.sort_by_key(|p| p.ts);
    }
    series
}

/// The narrowest level that still yields multiple groups — the old
/// `group_sales_by_scope` cascade.
pub fn auto_group_level(world_helper: &WorldHelper, sales: &[SaleHistory]) -> GroupLevel {
    let world_ids: HashSet<_> = sales
        .iter()
        .map(|s| AnySelector::World(s.world_id))
        .collect();
    let datacenters: HashSet<_> = world_ids
        .iter()
        .flat_map(|world| {
            world_helper
                .lookup_selector(*world)
                .and_then(|s| s.as_world())
                .map(|w| AnySelector::Datacenter(w.datacenter_id))
        })
        .collect();
    let regions: HashSet<_> = datacenters
        .iter()
        .flat_map(|dc| {
            world_helper
                .lookup_selector(*dc)
                .and_then(|dc| dc.as_datacenter())
                .map(|dc| AnySelector::Region(dc.region_id))
        })
        .collect();
    if datacenters.len() <= 1 {
        GroupLevel::World
    } else if regions.len() <= 1 {
        GroupLevel::Datacenter
    } else {
        GroupLevel::Region
    }
}

/// Which grouping levels make sense for the scope page being viewed —
/// ported from the web UI (a world page only offers World; a DC page offers
/// DC + World; a region page or unknown scope offers everything).
pub fn available_group_levels(world_helper: &WorldHelper, scope_name: &str) -> Vec<GroupLevel> {
    match world_helper.lookup_world_by_name(scope_name) {
        Some(result) if result.as_world().is_some() => vec![GroupLevel::World],
        Some(result) if result.as_datacenter().is_some() => {
            vec![GroupLevel::Datacenter, GroupLevel::World]
        }
        _ => vec![
            GroupLevel::Region,
            GroupLevel::Datacenter,
            GroupLevel::World,
        ],
    }
}
```

Then REPLACE the existing `group_sales_by_scope` and DELETE the now-unused private `series_for` helper:

```rust
/// Auto-picked grouping (the PNG path): the narrowest level that still
/// yields multiple groups.
pub fn group_sales_by_scope(world_helper: &WorldHelper, sales: &[SaleHistory]) -> Vec<Series> {
    group_sales_by_level(world_helper, sales, auto_group_level(world_helper, sales))
}
```

(Note: the old region-page arm matched `result.as_region().is_some()` separately; since the fallback arm returns the same vec, the match above folds them — behavior is identical.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ultros-charts`
Expected: PASS — all pre-existing grouping tests (including `unknown_worlds_are_dropped` and the scope-cascade tests) must still pass against the refactored implementation, plus the 3 new ones.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-charts/src/data/grouping.rs
git commit -m "feat(charts): explicit grouping levels and scope-based availability" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 2: PriceChartModel — hover data, stats, label offset

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/charts/price_history.rs`
- Modify: `ultros-frontend/ultros-charts/src/scale.rs` (ticks signature gains a label offset)
- Modify: `ultros-frontend/ultros-charts/src/data/buckets.rs` (volume from SalePoints, so hidden series drop out of the volume lane too)

- [ ] **Step 1: Write the failing tests**

Add to the test module of `charts/price_history.rs`:

```rust
    #[test]
    fn model_exposes_hover_buckets_series_and_stats() {
        let model = build_price_history_chart(
            &world_helper(),
            &two_world_sales(),
            &PriceChartOptions::default(),
        );
        assert_eq!(model.series.len(), 2);
        assert!(!model.hover.buckets.is_empty());
        for bucket in &model.hover.buckets {
            assert_eq!(bucket.series_values.len(), 2);
            assert!(!bucket.label.is_empty());
        }
        // sorted by x
        assert!(
            model
                .hover
                .buckets
                .windows(2)
                .all(|w| w[0].x <= w[1].x)
        );
        let stats = model.stats.expect("stats for non-empty sales");
        assert_eq!(stats.n, 20);
        assert!(stats.min <= stats.max);
        assert!(stats.market_average.is_some());
    }

    #[test]
    fn nearest_index_snaps_to_the_closest_bucket() {
        let hover = HoverModel {
            plot_top: 0.0,
            plot_bottom: 100.0,
            buckets: [10.0_f32, 20.0, 30.0]
                .iter()
                .map(|x| HoverBucket {
                    x: *x,
                    label: String::new(),
                    series_values: Vec::new(),
                    volume: 0,
                })
                .collect(),
        };
        assert_eq!(hover.nearest_index(-5.0), Some(0));
        assert_eq!(hover.nearest_index(14.0), Some(0));
        assert_eq!(hover.nearest_index(16.0), Some(1));
        assert_eq!(hover.nearest_index(99.0), Some(2));
        let empty = HoverModel {
            plot_top: 0.0,
            plot_bottom: 0.0,
            buckets: Vec::new(),
        };
        assert_eq!(empty.nearest_index(10.0), None);
    }

    #[test]
    fn scene_function_delegates_to_the_model() {
        let scene =
            build_price_history_scene(&world_helper(), &two_world_sales(), &PriceChartOptions::default());
        let model = build_price_history_chart(
            &world_helper(),
            &two_world_sales(),
            &PriceChartOptions::default(),
        );
        assert_eq!(scene, model.scene);
    }

    #[test]
    fn hidden_series_are_excluded_from_drawing_but_kept_in_metadata() {
        let model = build_price_history_chart(
            &world_helper(),
            &two_world_sales(),
            &PriceChartOptions {
                hidden_series: vec!["Gilgamesh".to_string()],
                ..Default::default()
            },
        );
        // Both series stay in metadata (the legend needs the hidden one to
        // offer un-hiding), flagged appropriately.
        assert_eq!(model.series.len(), 2);
        assert!(model.series.iter().any(|s| s.hidden));
        assert!(model.series.iter().any(|s| !s.hidden));
        // Only the visible series draws — and a single visible series gets
        // the area fill.
        let polylines = model
            .scene
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Polyline { .. }))
            .count();
        assert_eq!(polylines, 1);
        let areas = model
            .scene
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Area { .. }))
            .count();
        assert_eq!(areas, 1);
        // Hover keeps full-length series_values with None at the hidden slot
        // (series sort by name: Adamantoise=0, Gilgamesh=1).
        for bucket in &model.hover.buckets {
            assert_eq!(bucket.series_values.len(), 2);
            assert!(bucket.series_values[1].is_none());
        }
    }

    #[test]
    fn hiding_every_series_yields_the_no_data_card_but_keeps_metadata() {
        let model = build_price_history_chart(
            &world_helper(),
            &two_world_sales(),
            &PriceChartOptions {
                hidden_series: vec!["Gilgamesh".to_string(), "Adamantoise".to_string()],
                ..Default::default()
            },
        );
        assert!(model.hover.buckets.is_empty());
        assert_eq!(model.series.len(), 2, "legend must still offer un-hiding");
    }

    #[test]
    fn empty_sales_yield_empty_model_with_no_data_scene() {
        let model =
            build_price_history_chart(&world_helper(), &[], &PriceChartOptions::default());
        assert!(model.hover.buckets.is_empty());
        assert!(model.stats.is_none());
        assert!(model.scene.nodes.iter().any(
            |n| matches!(n, Node::Text { content, .. } if content == "No recent sales")
        ));
    }
```

And to the test module of `scale.rs`:

```rust
    #[test]
    fn tick_labels_shift_with_offset_but_positions_do_not() {
        let scale = TimeScale::new(ts(1_700_000_000), ts(1_700_000_000 + 2 * 3600), (0.0, 100.0));
        let utc = scale.ticks(6, 0);
        let shifted = scale.ticks(6, 60);
        assert_eq!(utc.len(), shifted.len());
        assert_eq!(utc[0].ts, shifted[0].ts);
        assert_ne!(utc[0].label, shifted[0].label);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-charts`
Expected: FAIL to compile.

- [ ] **Step 3: Change the TimeScale ticks signature**

In `scale.rs`: add `TimeDelta` to the chrono import, and change `ticks` to:

```rust
    /// At most `target` ticks aligned to step boundaries. `label_offset_minutes`
    /// shifts the LABEL text only (viewer-local display); tick positions stay
    /// UTC-aligned so SSR and client geometry agree.
    pub fn ticks(&self, target: usize, label_offset_minutes: i32) -> Vec<TimeTick> {
        let span = self.end - self.start;
        let step = TIME_STEPS
            .iter()
            .copied()
            .find(|step| span / step <= target as i64)
            .unwrap_or(365 * DAY);
        let format = if step < HOUR {
            "%H:%M"
        } else if step < DAY {
            "%m-%d %H:%M"
        } else if step < 30 * DAY {
            "%m-%d"
        } else {
            "%Y-%m"
        };
        let mut tick = self.start.div_euclid(step) * step;
        if tick < self.start {
            tick += step;
        }
        let mut out = Vec::new();
        while tick <= self.end {
            if let Some(ts) = chrono::DateTime::from_timestamp(tick, 0) {
                let ts = ts.naive_utc();
                let display = ts + TimeDelta::minutes(label_offset_minutes as i64);
                out.push(TimeTick {
                    ts,
                    label: display.format(format).to_string(),
                });
            }
            tick += step;
        }
        out
    }
```

Update the two existing `ticks(6)` calls in scale.rs's tests to `ticks(6, 0)`.

- [ ] **Step 4: Extend charts/price_history.rs**

New imports at the top of the file:

```rust
use std::collections::BTreeMap;
```

and extend the existing import lines: add `NaiveDateTime` is NOT needed; add `Color` to the `crate::scene` import, `median, vwap` (replacing the lone `vwap`) on `crate::data::stats`, and replace the grouping import with:

```rust
use crate::data::grouping::{GroupLevel, auto_group_level, group_sales_by_level};
```

Add to `PriceChartOptions` (and its `Default`: `group_level: None, utc_offset_minutes: 0`):

```rust
    /// Grouping level for series; `None` = pick automatically from the data
    /// scope (what the PNG path wants).
    pub group_level: Option<GroupLevel>,
    /// Shift applied to axis/tooltip LABELS so the browser can show
    /// viewer-local times. Bucket boundaries and geometry stay UTC-aligned;
    /// keep 0 for SSR and PNG so server and first client render agree.
    pub utc_offset_minutes: i32,
    /// Series names the user hid via the legend. They stay in the model's
    /// `series` metadata (flagged `hidden`) but draw nothing, feed no hover
    /// values, and don't influence the axes.
    pub hidden_series: Vec<String>,
```

(Default adds `hidden_series: Vec::new()`.)

Add the model types above `build_price_history_chart`:

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct SeriesInfo {
    pub name: String,
    pub color: Color,
    /// True when the user hid this series via the legend; it stays listed so
    /// the legend can offer un-hiding.
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChartStats {
    pub n: usize,
    pub market_average: Option<i32>,
    pub median: Option<i32>,
    pub min: i32,
    pub max: i32,
}

/// One hoverable time bucket: pixel x of the bucket center, a display label
/// (already offset to viewer time), per-series `(y_px, vwap)` (None where a
/// series has no sales in the bucket), and total volume.
#[derive(Clone, Debug, PartialEq)]
pub struct HoverBucket {
    pub x: f32,
    pub label: String,
    pub series_values: Vec<Option<(f32, f64)>>,
    pub volume: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HoverModel {
    /// Vertical extent for the crosshair line.
    pub plot_top: f32,
    pub plot_bottom: f32,
    /// Sorted by x ascending.
    pub buckets: Vec<HoverBucket>,
}

impl HoverModel {
    /// Index of the bucket whose center is closest to pixel `x`.
    pub fn nearest_index(&self, x: f32) -> Option<usize> {
        if self.buckets.is_empty() {
            return None;
        }
        let i = self.buckets.partition_point(|b| b.x < x);
        if i == 0 {
            return Some(0);
        }
        if i >= self.buckets.len() {
            return Some(self.buckets.len() - 1);
        }
        if (x - self.buckets[i - 1].x) <= (self.buckets[i].x - x) {
            Some(i - 1)
        } else {
            Some(i)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PriceChartModel {
    pub scene: Scene,
    pub hover: HoverModel,
    pub series: Vec<SeriesInfo>,
    pub stats: Option<ChartStats>,
    /// The level actually used (resolves `group_level: None`).
    pub group_level: GroupLevel,
}
```

Rename `build_price_history_scene` to `build_price_history_chart` returning `PriceChartModel`, then add a thin delegate so the PNG path (`item_card.rs`, `png_smoke.rs`, the example) compiles unchanged:

```rust
pub fn build_price_history_scene(
    world_helper: &WorldHelper,
    sales: &[SaleHistory],
    options: &PriceChartOptions,
) -> Scene {
    build_price_history_chart(world_helper, sales, options).scene
}
```

Inside `build_price_history_chart`, make these changes to the existing body:

a) Grouping — replace the `group_sales_by_scope` call. Colors index the FULL series list so a series keeps its color while hidden, and `all_points` (which drives the axes, stats, and overlays) iterates only visible series:

```rust
    let level = options
        .group_level
        .unwrap_or_else(|| auto_group_level(world_helper, &sales));
    let series = group_sales_by_level(world_helper, &sales, level);
    let is_hidden = |name: &str| options.hidden_series.iter().any(|h| h == name);
    let series_info: Vec<SeriesInfo> = series
        .iter()
        .enumerate()
        .map(|(index, group)| SeriesInfo {
            name: group.name.clone(),
            color: theme.palette[index % theme.palette.len()],
            hidden: is_hidden(&group.name),
        })
        .collect();
    let all_points = || {
        series
            .iter()
            .filter(|s| !is_hidden(&s.name))
            .flat_map(|s| s.points.iter())
    };
    let visible_count = series.iter().filter(|s| !is_hidden(&s.name)).count();
```

(This REPLACES the existing `let all_points = ...` closure — delete the old one.)

b) The empty-data early return becomes:

```rust
    let Some((first_ts, last_ts)) = all_points().map(|p| p.ts).minmax().into_option() else {
        scene.nodes.push(Node::Text {
            x: options.width / 2.0,
            y: options.height / 2.0,
            content: "No recent sales".to_string(),
            size: 22.0,
            color: theme.text_muted,
            anchor: TextAnchor::Middle,
            bold: false,
        });
        return PriceChartModel {
            scene,
            hover: HoverModel {
                plot_top: 0.0,
                plot_bottom: 0.0,
                buckets: Vec::new(),
            },
            series: series_info,
            stats: None,
            group_level: level,
        };
    };
```

c) After `(min_price, max_price)` is computed, build the stats:

```rust
    let stats = {
        let prices: Vec<i32> = all_points().map(|p| p.price).collect();
        let pairs: Vec<(i32, i32)> = all_points().map(|p| (p.price, p.quantity)).collect();
        Some(ChartStats {
            n: prices.len(),
            market_average: vwap(&pairs),
            median: median(&prices),
            min: min_price,
            max: max_price,
        })
    };
```

d) The x-tick loop adapts its density to the width and passes the offset:

```rust
    let x_tick_target = ((options.width / 150.0) as usize).clamp(3, 8);
    for tick in time.ticks(x_tick_target, options.utc_offset_minutes) {
```

(960px keeps the current 6 ticks.)

e) Volume buckets move OUT of the `if options.show_volume` block (they feed hover regardless) and now aggregate the VISIBLE series' points instead of the raw sales (so hidden series drop out of the volume lane, and unknown-world sales — already absent from every series — no longer sneak into volume). First add to `data/buckets.rs`:

```rust
/// Total quantity per bucket over grouped sale points (the chart feeds the
/// visible series' points here so hidden series don't count).
pub fn volume_buckets_from_points<'a>(
    points: impl Iterator<Item = &'a SalePoint>,
    bucket_secs: i64,
) -> Vec<VolumeBucket> {
    if bucket_secs <= 0 {
        return Vec::new();
    }
    let mut sums: BTreeMap<i64, i64> = BTreeMap::new();
    for point in points {
        let bucket = point.ts.and_utc().timestamp().div_euclid(bucket_secs) * bucket_secs;
        *sums.entry(bucket).or_default() += point.quantity as i64;
    }
    sums.into_iter()
        .filter_map(|(bucket, quantity)| {
            chrono::DateTime::from_timestamp(bucket, 0).map(|ts| VolumeBucket {
                ts: ts.naive_utc(),
                quantity,
            })
        })
        .collect()
}
```

DELETE the old `volume_buckets` (its only caller was this chart) and rewrite its test to the new function:

```rust
    #[test]
    fn volume_buckets_sum_quantities() {
        let points = vec![
            SalePoint { ts: ts(0), price: 100, quantity: 2 },
            SalePoint { ts: ts(60), price: 100, quantity: 3 },
            SalePoint { ts: ts(86_400), price: 100, quantity: 5 },
        ];
        let buckets = volume_buckets_from_points(points.iter(), 86_400);
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].quantity, 5);
        assert_eq!(buckets[1].quantity, 5);
    }
```

Then in the chart (the `use crate::data::buckets::...` import swaps `volume_buckets` for `volume_buckets_from_points`):

```rust
    let volumes = volume_buckets_from_points(all_points(), bucket_secs);
    if options.show_volume {
        if let Some(max_volume) = volumes.iter().map(|v| v.quantity).max() {
            // ... existing bar-drawing code unchanged ...
        }
    }
```

f) The VWAP-line loop skips hidden series and fills a hover map; the raw-dots loop above it gets the same `if series_info[index].hidden { continue; }` guard. Replace the line loop with:

```rust
    let mut hover_map: BTreeMap<i64, Vec<Option<(f32, f64)>>> = BTreeMap::new();
    for (index, group) in series.iter().enumerate() {
        if series_info[index].hidden {
            continue;
        }
        let color = series_color(index);
        let buckets = vwap_buckets(&group.points, bucket_secs);
        for point in &buckets {
            // key by bucket START so it aligns with the volume buckets
            let key = point.ts.and_utc().timestamp() - bucket_secs / 2;
            hover_map.entry(key).or_insert_with(|| vec![None; series.len()])[index] =
                Some((price.scale(point.vwap), point.vwap));
        }
        let line: Vec<(f32, f32)> = buckets
            .into_iter()
            .map(|p| (time.scale(p.ts), price.scale(p.vwap)))
            .collect();
        if line.len() > 1 {
            if visible_count == 1 {
                scene.nodes.push(Node::Area {
                    points: line.clone(),
                    baseline_y: price_bottom,
                    fill: color.with_alpha(0.08),
                });
            }
            scene.nodes.push(Node::Polyline {
                points: line,
                stroke: Stroke {
                    color,
                    width: 2.0,
                    dash: None,
                },
            });
        }
    }
```

(Note `visible_count == 1` replaces the old `series.len() == 1` for the area fill, and the legend block near the end keeps using the full `series` list — hidden chips must still render in the PNG legend? No: the PNG path never sets `hidden_series`, so the legend block is unaffected; leave it untouched.)

g) At the end of the function (after the title/legend block), assemble and return:

```rust
    let mut volume_by_bucket: BTreeMap<i64, i64> = volumes
        .iter()
        .map(|v| (v.ts.and_utc().timestamp(), v.quantity))
        .collect();
    let label_format = if bucket_secs < 86_400 {
        "%m-%d %H:%M"
    } else {
        "%Y-%m-%d"
    };
    let hover_buckets: Vec<HoverBucket> = hover_map
        .into_iter()
        .filter_map(|(start, series_values)| {
            let center = chrono::DateTime::from_timestamp(start + bucket_secs / 2, 0)?.naive_utc();
            let display = center + TimeDelta::minutes(options.utc_offset_minutes as i64);
            Some(HoverBucket {
                x: time.scale(center),
                label: display.format(label_format).to_string(),
                series_values,
                volume: volume_by_bucket.remove(&start).unwrap_or(0),
            })
        })
        .collect();

    PriceChartModel {
        scene,
        hover: HoverModel {
            plot_top,
            plot_bottom,
            buckets: hover_buckets,
        },
        series: series_info,
        stats,
        group_level: level,
    }
```

(The `series_color` closure and everything else stays as is. `TimeDelta` is already imported in this file from PR 1.)

- [ ] **Step 5: Run the full crate tests**

Run: `cargo test -p ultros-charts --features image`
Expected: PASS — all existing tests (including png_smoke and the structural scene tests) plus the 5 new ones. The scene snapshot behavior must be unchanged: `scene_function_delegates_to_the_model` plus the untouched `renders_lines_dots_volume_and_labels` prove it.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-charts/src/charts/price_history.rs ultros-frontend/ultros-charts/src/scale.rs
git commit -m "feat(charts): PriceChartModel with hover buckets, stats, and label offset" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 3: `leptos` feature — scene_view renderer

**Files:**
- Modify: `ultros-frontend/ultros-charts/Cargo.toml`
- Modify: `ultros-frontend/ultros-charts/src/svg.rs` (share two helpers)
- Create: `ultros-frontend/ultros-charts/src/components.rs`
- Modify: `ultros-frontend/ultros-charts/src/lib.rs`

- [ ] **Step 1: Wire the feature**

In `ultros-frontend/ultros-charts/Cargo.toml`:

```toml
[features]
image = ["dep:ultros-xiv-icons", "dep:image", "dep:base64"]
leptos = ["dep:leptos"]
```

Add to `[dependencies]`:

```toml
leptos = { workspace = true, optional = true }
```

Add to `[dev-dependencies]` (SSR rendering for the unit test; feature-unified only for this crate's tests):

```toml
leptos = { workspace = true, features = ["ssr"] }
```

- [ ] **Step 2: Share the geometry serializers**

In `svg.rs`: change `fn points_attr` to `pub(crate) fn points_attr`, and extract the Area path construction into a shared helper (replace the body of the `Node::Area` match arm to use it):

```rust
/// Path data for a filled area: the polyline plus a closing run along the
/// baseline. `None` for fewer than 2 points.
pub(crate) fn area_path_d(points: &[(f32, f32)], baseline_y: f32) -> Option<String> {
    if points.len() < 2 {
        return None;
    }
    let mut d = String::new();
    for (i, (x, y)) in points.iter().enumerate() {
        let _ = write!(d, "{}{x:.1} {y:.1}", if i == 0 { "M" } else { "L" });
    }
    let first_x = points[0].0;
    let last_x = points[points.len() - 1].0;
    let _ = write!(d, "L{last_x:.1} {baseline_y:.1}L{first_x:.1} {baseline_y:.1}Z");
    Some(d)
}
```

New Area arm in `scene_to_svg`:

```rust
            Node::Area {
                points,
                baseline_y,
                fill,
            } => {
                let Some(d) = area_path_d(points, *baseline_y) else {
                    continue;
                };
                let _ = write!(out, r#"<path d="{d}""#);
                push_fill(&mut out, fill);
                out.push_str("/>");
            }
```

Run `cargo test -p ultros-charts svg` — the existing serializer tests must still pass byte-for-byte.

- [ ] **Step 3: Write the failing component test**

Create `src/components.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Color, Node, Scene, Stroke, TextAnchor};
    use leptos::prelude::*;

    #[test]
    fn renders_scene_nodes_as_svg_markup() {
        let scene = Scene {
            width: 100.0,
            height: 50.0,
            background: Some(Color::hex("#202124")),
            font_family: "sans-serif".to_string(),
            nodes: vec![
                Node::Circle {
                    cx: 5.0,
                    cy: 5.0,
                    r: 2.0,
                    fill: Color::rgb(9, 9, 9).with_alpha(0.5),
                },
                Node::Polyline {
                    points: vec![(0.0, 0.0), (5.0, 5.0)],
                    stroke: Stroke {
                        color: Color::rgb(0, 0, 255),
                        width: 2.0,
                        dash: Some((2.0, 4.0)),
                    },
                },
                Node::Area {
                    points: vec![(0.0, 10.0), (5.0, 5.0)],
                    baseline_y: 20.0,
                    fill: Color::rgb(1, 2, 3),
                },
                Node::Text {
                    x: 1.0,
                    y: 1.0,
                    content: "hi".to_string(),
                    size: 13.0,
                    color: Color::rgb(0, 0, 0),
                    anchor: TextAnchor::Middle,
                    bold: true,
                },
            ],
        };
        let html = scene_view(&scene).to_html();
        assert!(html.contains("<rect"), "background rect: {html}");
        assert!(html.contains("rgba(9,9,9,0.500)"));
        assert!(html.contains("<polyline"));
        assert!(html.contains("stroke-dasharray=\"2.0 4.0\""));
        assert!(html.contains("<path"));
        assert!(html.contains("text-anchor=\"middle\""));
        assert!(html.contains(">hi</text>"));
    }
}
```

Add to `lib.rs` (below the icon block):

```rust
#[cfg(feature = "leptos")]
pub mod components;
```

- [ ] **Step 4: Run test to verify it fails**

Run: `cargo test -p ultros-charts --features leptos components`
Expected: FAIL to compile (`scene_view` missing).

- [ ] **Step 5: Write the implementation**

Prepend to `components.rs`:

```rust
//! Leptos renderer over [`Scene`] — the browser counterpart of [`crate::svg`].
//!
//! [`scene_view`] maps display-list nodes to SVG view nodes. It renders a
//! snapshot of the scene; reactivity comes from the caller rebuilding the
//! scene inside a memo/closure. Hover layers are drawn by the app on top.

use leptos::prelude::*;

use crate::scene::{Color, Node, Scene, Stroke, TextAnchor};
use crate::svg::{area_path_d, points_attr};

/// CSS color string. Browsers accept `rgba()`, so unlike the resvg-bound
/// serializer this needs no separate `*-opacity` attributes.
pub fn color_attr(c: &Color) -> String {
    if c.a >= 1.0 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("rgba({},{},{},{:.3})", c.r, c.g, c.b, c.a)
    }
}

fn px(v: f32) -> String {
    format!("{v:.1}")
}

fn dash_attr(stroke: &Stroke) -> Option<String> {
    stroke.dash.map(|(dash, gap)| format!("{dash:.1} {gap:.1}"))
}

/// Render the scene's nodes (plus its background) as SVG children. Embed
/// inside an `<svg viewBox="0 0 {scene.width} {scene.height}">`.
pub fn scene_view(scene: &Scene) -> impl IntoView {
    let background = scene.background.as_ref().map(|bg| {
        view! {
            <rect x="0" y="0" width=px(scene.width) height=px(scene.height) fill=color_attr(bg) />
        }
    });
    let nodes = scene.nodes.iter().map(node_view).collect_view();
    view! {
        {background}
        <g font-family=scene.font_family.clone()>{nodes}</g>
    }
}

fn node_view(node: &Node) -> AnyView {
    match node {
        Node::Rect {
            x,
            y,
            width,
            height,
            rx,
            fill,
        } => view! {
            <rect
                x=px(*x)
                y=px(*y)
                width=px(*width)
                height=px(*height)
                rx=(*rx > 0.0).then(|| px(*rx))
                fill=color_attr(fill)
            />
        }
        .into_any(),
        Node::Line {
            x1,
            y1,
            x2,
            y2,
            stroke,
        } => view! {
            <line
                x1=px(*x1)
                y1=px(*y1)
                x2=px(*x2)
                y2=px(*y2)
                stroke=color_attr(&stroke.color)
                stroke-width=px(stroke.width)
                stroke-linecap="round"
                stroke-dasharray=dash_attr(stroke)
            />
        }
        .into_any(),
        Node::Polyline { points, stroke } => view! {
            <polyline
                points=points_attr(points)
                fill="none"
                stroke=color_attr(&stroke.color)
                stroke-width=px(stroke.width)
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-dasharray=dash_attr(stroke)
            />
        }
        .into_any(),
        Node::Area {
            points,
            baseline_y,
            fill,
        } => match area_path_d(points, *baseline_y) {
            Some(d) => view! { <path d=d fill=color_attr(fill) /> }.into_any(),
            None => ().into_any(),
        },
        Node::Circle { cx, cy, r, fill } => view! {
            <circle cx=px(*cx) cy=px(*cy) r=px(*r) fill=color_attr(fill) />
        }
        .into_any(),
        Node::Text {
            x,
            y,
            content,
            size,
            color,
            anchor,
            bold,
        } => {
            let anchor = match anchor {
                TextAnchor::Start => "start",
                TextAnchor::Middle => "middle",
                TextAnchor::End => "end",
            };
            view! {
                <text
                    x=px(*x)
                    y=px(*y)
                    font-size=px(*size)
                    text-anchor=anchor
                    font-weight=bold.then_some("bold")
                    fill=color_attr(color)
                >
                    {content.clone()}
                </text>
            }
            .into_any()
        }
        Node::Image {
            x,
            y,
            width,
            height,
            href,
        } => view! {
            <image x=px(*x) y=px(*y) width=px(*width) height=px(*height) href=href.clone() />
        }
        .into_any(),
    }
}
```

If `().into_any()` doesn't satisfy the type checker on this leptos version, use `view! { <g></g> }.into_any()` for the empty-Area arm and note it.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p ultros-charts --features leptos` and then `cargo test -p ultros-charts --features image` (the non-leptos config must stay green).
Expected: PASS both.

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-charts/Cargo.toml ultros-frontend/ultros-charts/src/components.rs ultros-frontend/ultros-charts/src/svg.rs ultros-frontend/ultros-charts/src/lib.rs Cargo.lock
git commit -m "feat(charts): leptos feature with scene_view SVG renderer" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 4: Rewrite the app component, remove leptos-chartistry

**Files:**
- Modify: `ultros-frontend/ultros-app/Cargo.toml`
- Rewrite: `ultros-frontend/ultros-app/src/components/price_history_chart.rs`
- Maybe modify: `style/tailwind.css` (remove chartistry-only CSS)
- DO NOT TOUCH: `ultros-frontend/ultros-app/src/routes/item_view.rs` (props are unchanged), any locale file.

- [ ] **Step 1: Cargo changes**

In `ultros-frontend/ultros-app/Cargo.toml`: delete the `leptos-chartistry = "0.2.3"` line and change the ultros-charts dep to:

```toml
ultros-charts = { path = "../ultros-charts", features = ["leptos"] }
```

- [ ] **Step 2: Rewrite price_history_chart.rs**

Replace the entire file. The controls/markup below intentionally reproduce the current UI (same classes, same i18n keys); the chartistry plumbing, the `CATEGORY_PALETTE` const, the local `ColorBy` enum, the duplicated math helpers (`vwap`/`median`/`iqr_band`/`short_number`/`bucket_quantities`/`quantity_bucket_seconds`/`x_axis_periods`/`group_sales_by_level`), the `marker_css` `<style>` hack, and the in-file tests (their coverage moved to ultros-charts in Tasks 1–2) all go away.

```rust
use leptos::prelude::*;
use leptos_use::{UseElementBoundingReturn, use_element_bounding};
use ultros_api_types::SaleHistory;
use ultros_charts::charts::price_history::{
    ChartStats, PriceChartModel, PriceChartOptions, build_price_history_chart,
};
use ultros_charts::components::{color_attr, scene_view};
use ultros_charts::data::grouping::{GroupLevel, available_group_levels};
use ultros_charts::scale::short_number;
use ultros_charts::theme::Theme;

use crate::global_state::LocalWorldData;
use crate::i18n::{t, t_string, use_i18n};

fn px(v: f32) -> String {
    format!("{v:.1}")
}

// ── Sub-components ────────────────────────────────────────────────────────────

#[component]
fn StatsStrip(stats: Signal<Option<ChartStats>>) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        {move || {
            stats
                .get()
                .map(|s| {
                    let n_label = t_string!(i18n, chart_stat_n_sales)
                        .to_string()
                        .replace("{n}", &s.n.to_string());
                    let market_average_label =
                        t_string!(i18n, chart_stat_market_avg).to_string();
                    let median_label = t_string!(i18n, chart_stat_median).to_string();
                    let min_label = t_string!(i18n, chart_stat_min).to_string();
                    let max_label = t_string!(i18n, chart_stat_max).to_string();
                    view! {
                        <div class="flex flex-wrap gap-x-4 gap-y-1 text-sm tabular-nums text-[color:var(--color-text)]/70 mb-3">
                            <span>{n_label}</span>
                            {s
                                .market_average
                                .map(|v| {
                                    view! {
                                        <span>
                                            {market_average_label.clone()} " " {short_number(v)}
                                        </span>
                                    }
                                })}
                            {s
                                .median
                                .map(|v| {
                                    view! {
                                        <span>
                                            {median_label} " " {short_number(v)}
                                        </span>
                                    }
                                })}
                            <span>{min_label} " " {short_number(s.min)}</span>
                            <span>{max_label} " " {short_number(s.max)}</span>
                        </div>
                    }
                        .into_any()
                })
        }}
    }
}

#[component]
fn ChartOverlayToggle(
    label: String,
    #[prop(into)] checked: Signal<bool>,
    set_checked: WriteSignal<bool>,
) -> impl IntoView {
    view! {
        <label
            class=move || {
                [
                    "inline-flex cursor-pointer select-none items-center gap-1.5 rounded-md border px-2.5 py-1 transition-colors",
                    if checked.get() {
                        "border-brand-500/60 bg-brand-700/30 text-brand-100"
                    } else {
                        "border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--color-text)_4%,_transparent)] text-[color:var(--color-text-muted)]"
                    },
                ]
                    .join(" ")
            }
        >
            <input
                class="sr-only"
                type="checkbox"
                prop:checked=checked
                on:change=move |event| set_checked.set(event_target_checked(&event))
            />
            <span
                class=move || {
                    [
                        "h-2 w-2 rounded-full",
                        if checked.get() { "bg-brand-300" } else { "bg-[color:var(--color-text-muted)]/45" },
                    ]
                        .join(" ")
                }
            ></span>
            {label}
        </label>
    }
}

#[component]
fn ColorByControl(
    #[prop(into)] options: Signal<Vec<GroupLevel>>,
    #[prop(into)] selected: Signal<GroupLevel>,
    set_selected: WriteSignal<GroupLevel>,
) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <Show when=move || options.with(|options| options.len() > 1)>
            <div class="flex flex-wrap items-center gap-2 text-xs">
                <span class="font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]">
                    {t!(i18n, chart_color_by)}
                </span>
                <div class="inline-flex overflow-hidden rounded-md border border-[color:var(--color-outline)]">
                <For
                    each=move || options.get()
                    key=|option| option.label()
                    children=move |option| {
                        view! {
                            <button
                                type="button"
                                class=move || {
                                    let active = selected.get() == option;
                                    [
                                        "border-l border-[color:var(--color-outline)] px-2.5 py-1 transition-colors first:border-l-0",
                                        if active {
                                            "bg-brand-600/30 text-brand-100"
                                        } else {
                                            "bg-[color:color-mix(in_srgb,_var(--color-text)_4%,_transparent)] text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                        },
                                    ]
                                        .join(" ")
                                }
                                on:click=move |_| set_selected.set(option)
                            >
                                {match option {
                                    GroupLevel::Region => t_string!(i18n, chart_color_region).to_string(),
                                    GroupLevel::Datacenter => t_string!(i18n, chart_color_datacenter).to_string(),
                                    GroupLevel::World => t_string!(i18n, chart_color_world).to_string(),
                                }}
                            </button>
                        }
                    }
                />
                </div>
            </div>
        </Show>
    }
}

/// Crosshair + per-series dots at the hovered bucket. Lives INSIDE the
/// chart's `<svg>` so it shares the viewBox coordinate space.
#[component]
fn HoverLayer(
    model: Memo<PriceChartModel>,
    hover_index: RwSignal<Option<usize>>,
) -> impl IntoView {
    move || {
        hover_index.get().and_then(|i| {
            model.with(|m| {
                let bucket = m.hover.buckets.get(i)?;
                let dots = bucket
                    .series_values
                    .iter()
                    .enumerate()
                    .filter_map(|(series_index, value)| {
                        let (y, _) = (*value)?;
                        let color = m.series.get(series_index)?.color;
                        Some(view! {
                            <circle
                                cx=px(bucket.x)
                                cy=px(y)
                                r="4"
                                fill=color_attr(&color)
                                stroke="#16131f"
                                stroke-width="1.5"
                            />
                        })
                    })
                    .collect_view();
                Some(view! {
                    <g class="pointer-events-none">
                        <line
                            x1=px(bucket.x)
                            y1=px(m.hover.plot_top)
                            x2=px(bucket.x)
                            y2=px(m.hover.plot_bottom)
                            stroke="#9ca3af"
                            stroke-opacity="0.45"
                            stroke-width="1"
                        />
                        {dots}
                    </g>
                })
            })
        })
    }
}

/// HTML tooltip positioned over the chart container; flips to the left of
/// the crosshair past the midpoint so it never clips on the right edge.
#[component]
fn HoverTooltip(
    model: Memo<PriceChartModel>,
    hover_index: RwSignal<Option<usize>>,
    #[prop(into)] show_quantity: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    move || {
        hover_index.get().and_then(|i| {
            model.with(|m| {
                let bucket = m.hover.buckets.get(i)?.clone();
                let series = m.series.clone();
                let left_pct = (bucket.x / m.scene.width * 100.0).clamp(0.0, 100.0);
                let style = if left_pct > 55.0 {
                    format!("left:calc({left_pct:.1}% - 12px);transform:translateX(-100%)")
                } else {
                    format!("left:calc({left_pct:.1}% + 12px)")
                };
                Some(view! {
                    <div
                        class="pointer-events-none absolute top-2 z-10 min-w-36 rounded-md border border-[color:var(--color-outline)] bg-violet-950/95 px-3 py-2 text-xs shadow-lg"
                        style=style
                    >
                        <div class="mb-1 font-semibold text-[color:var(--color-text)]">
                            {bucket.label.clone()}
                        </div>
                        {series
                            .iter()
                            .enumerate()
                            .filter_map(|(series_index, info)| {
                                let (_, vwap) =
                                    bucket.series_values.get(series_index).copied().flatten()?;
                                Some(view! {
                                    <div class="flex items-center justify-between gap-3">
                                        <span class="inline-flex items-center gap-1.5">
                                            <span
                                                class="inline-block h-2 w-2 rounded-full"
                                                style:background-color=color_attr(&info.color)
                                            ></span>
                                            <span class="text-[color:var(--color-text-muted)]">
                                                {info.name.clone()}
                                            </span>
                                        </span>
                                        <span class="tabular-nums text-[color:var(--color-text)]">
                                            {short_number(vwap.round() as i32)}
                                        </span>
                                    </div>
                                })
                            })
                            .collect_view()}
                        {show_quantity
                            .get()
                            .then(|| {
                                view! {
                                    <div class="mt-1 flex items-center justify-between gap-3 border-t border-[color:var(--color-outline)]/60 pt-1">
                                        <span class="text-[color:var(--color-text-muted)]">
                                            {t!(i18n, chart_legend_quantity)}
                                        </span>
                                        <span class="tabular-nums text-[color:var(--color-text)]">
                                            {bucket.volume}
                                        </span>
                                    </div>
                                }
                            })}
                    </div>
                })
            })
        })
    }
}

// ── Main component ────────────────────────────────────────────────────────────

#[component]
pub fn PriceHistoryChart(
    #[prop(into)] sales: Signal<Vec<SaleHistory>>,
    #[prop(into)] filter_outliers: Signal<bool>,
    #[prop(into)] scope_name: Signal<String>,
    /// Selected days window from the parent (7 / 30 / 90 / 0 for All).
    #[prop(into)]
    days_range: Signal<i32>,
) -> impl IntoView {
    let local_world_data = use_context::<LocalWorldData>().unwrap();
    let helper = local_world_data.0.unwrap();
    let i18n = use_i18n();
    let (show_market_average, set_show_market_average) = signal(true);
    let (show_trend, set_show_trend) = signal(false);
    let (show_quantity, set_show_quantity) = signal(false);
    let (color_by, set_color_by) = signal(GroupLevel::World);
    // Series the user hid by clicking legend chips. Stored as a sorted Vec so
    // the model memo's PartialEq sees a stable value.
    let hidden_series = RwSignal::new(Vec::<String>::new());

    // Viewer timezone for axis/tooltip LABELS only. SSR and the first client
    // render agree on 0 (UTC); this effect shifts the labels after hydration
    // — same idea as ChartWrapper's `hydrated` gate, so tachys never sees
    // divergent markup. Bucketing/geometry are timezone-independent.
    let utc_offset = RwSignal::new(0i32);
    Effect::new(move |_| {
        utc_offset.set(chrono::Local::now().offset().local_minus_utc() / 60);
    });

    // Responsive: rebuild the scene at the measured container width so text
    // renders at natural size instead of scaling down. Unmeasured (SSR and
    // first client render) falls back to 960, and leptos-use only updates
    // the signal post-mount — hydration-safe for the same reason as above.
    let container = NodeRef::<leptos::html::Div>::new();
    let UseElementBoundingReturn {
        left: container_left,
        width: container_width,
        ..
    } = use_element_bounding(container);

    let helper_for_options = helper.clone();
    let color_by_options =
        Memo::new(move |_| available_group_levels(&helper_for_options, &scope_name.get()));
    let effective_color_by = Memo::new(move |_| {
        let selected = color_by.get();
        let options = color_by_options.get();
        if options.contains(&selected) {
            selected
        } else {
            *options.last().unwrap_or(&GroupLevel::World)
        }
    });

    let helper_for_model = helper.clone();
    let model = Memo::new(move |_| {
        let sales = sales.get();
        let measured = container_width.get() as f32;
        let width = if measured > 0.0 {
            measured.clamp(320.0, 1600.0)
        } else {
            960.0
        };
        let height = (width * 0.56).clamp(300.0, 540.0);
        build_price_history_chart(
            &helper_for_model,
            &sales,
            &PriceChartOptions {
                width,
                height,
                remove_outliers: filter_outliers.get(),
                show_market_average: show_market_average.get(),
                show_trendline: show_trend.get(),
                show_volume: show_quantity.get(),
                show_legend: false,
                title: None,
                icon_data_uri: None,
                days_range: Some(days_range.get()),
                group_level: Some(effective_color_by.get()),
                utc_offset_minutes: utc_offset.get(),
                hidden_series: hidden_series.get(),
                theme: Theme::site(),
            },
        )
    });

    let stats = Signal::derive(move || model.with(|m| m.stats.clone()));
    let hover_index = RwSignal::new(None::<usize>);

    let on_pointer_move = move |evt: web_sys::PointerEvent| {
        let width = container_width.get_untracked();
        if width <= 0.0 {
            return;
        }
        let x_css = evt.client_x() as f64 - container_left.get_untracked();
        let index = model.with_untracked(|m| {
            m.hover
                .nearest_index((x_css / width) as f32 * m.scene.width)
        });
        hover_index.set(index);
    };

    view! {
        <div class="flex flex-col gap-3">
            <StatsStrip stats=stats />
            <div class="flex flex-wrap items-center gap-2 text-xs">
                <ChartOverlayToggle
                    label=t_string!(i18n, chart_toggle_market_avg).to_string()
                    checked=show_market_average
                    set_checked=set_show_market_average
                />
                <ChartOverlayToggle
                    label=t_string!(i18n, chart_legend_trend).to_string()
                    checked=show_trend
                    set_checked=set_show_trend
                />
                <ChartOverlayToggle
                    label=t_string!(i18n, chart_legend_quantity).to_string()
                    checked=show_quantity
                    set_checked=set_show_quantity
                />
            </div>
            <ColorByControl options=color_by_options selected=effective_color_by set_selected=set_color_by />
            <div
                role="img"
                aria-label=move || {
                    let n = stats.get().map(|s| s.n).unwrap_or(0);
                    let (from, to) = model.with(|m| {
                        (
                            m.hover.buckets.first().map(|b| b.label.clone()).unwrap_or_default(),
                            m.hover.buckets.last().map(|b| b.label.clone()).unwrap_or_default(),
                        )
                    });
                    t_string!(i18n, chart_aria_label)
                        .to_string()
                        .replace("{n}", &n.to_string())
                        .replace("{from}", &from)
                        .replace("{to}", &to)
                }
                class="price-history-chart relative w-full overflow-visible"
                node_ref=container
                on:pointermove=on_pointer_move
                on:pointerleave=move |_| hover_index.set(None)
            >
                {move || {
                    let m = model.get();
                    if m.hover.buckets.is_empty() {
                        let msg = t_string!(i18n, chart_no_sales_in_window).to_string();
                        return view! {
                            <div class="flex items-center justify-center w-full h-full text-[color:var(--color-text)]/60 text-sm">
                                {msg}
                            </div>
                        }
                            .into_any();
                    }
                    view! {
                        <svg
                            class="block w-full h-auto"
                            viewBox=format!("0 0 {:.0} {:.0}", m.scene.width, m.scene.height)
                            preserveAspectRatio="xMidYMid meet"
                        >
                            {scene_view(&m.scene)}
                            <HoverLayer model=model hover_index=hover_index />
                        </svg>
                    }
                        .into_any()
                }}
                <HoverTooltip model=model hover_index=hover_index show_quantity=show_quantity />
            </div>
            {move || {
                let m = model.get();
                (!m.series.is_empty())
                    .then(|| {
                        let toggleable = m.series.len() > 1;
                        view! {
                            <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-[color:var(--color-text-muted)]">
                                {m
                                    .series
                                    .iter()
                                    .map(|info| {
                                        let name = info.name.clone();
                                        let toggle_name = info.name.clone();
                                        let hidden = info.hidden;
                                        view! {
                                            <button
                                                type="button"
                                                disabled=!toggleable
                                                class=[
                                                    "inline-flex items-center gap-1.5 transition-opacity",
                                                    if toggleable { "cursor-pointer" } else { "cursor-default" },
                                                    if hidden { "opacity-40 line-through" } else { "" },
                                                ]
                                                    .join(" ")
                                                on:click=move |_| {
                                                    if !toggleable {
                                                        return;
                                                    }
                                                    hidden_series
                                                        .update(|hidden_list| {
                                                            if let Some(pos) = hidden_list
                                                                .iter()
                                                                .position(|n| n == &toggle_name)
                                                            {
                                                                hidden_list.remove(pos);
                                                            } else {
                                                                hidden_list.push(toggle_name.clone());
                                                                hidden_list.sort();
                                                            }
                                                        });
                                                }
                                            >
                                                <span
                                                    class="h-2.5 w-2.5 rounded-full ring-1 ring-blue-100/70"
                                                    style:background-color=color_attr(&info.color)
                                                ></span>
                                                {name}
                                            </button>
                                        }
                                    })
                                    .collect_view()}
                                {show_market_average
                                    .get()
                                    .then(|| {
                                        view! {
                                            <span class="inline-flex items-center gap-1.5">
                                                <span class="h-0.5 w-5 bg-[#facc15]"></span>
                                                {t!(i18n, chart_legend_market_avg)}
                                            </span>
                                        }
                                    })}
                                {show_trend
                                    .get()
                                    .then(|| {
                                        view! {
                                            <span class="inline-flex items-center gap-1.5">
                                                <span class="h-0.5 w-5 bg-[#94a3b8]"></span>
                                                {t!(i18n, chart_legend_trend)}
                                            </span>
                                        }
                                    })}
                                {show_quantity
                                    .get()
                                    .then(|| {
                                        view! {
                                            <span class="inline-flex items-center gap-1.5">
                                                <span class="h-2.5 w-3 rounded-sm bg-[#22c55e]"></span>
                                                {t!(i18n, chart_legend_quantity)}
                                            </span>
                                        }
                                    })}
                            </div>
                        }
                    })
            }}
        </div>
    }
}
```

Implementation notes (compile-fix latitude, report anything you change):
- `web_sys::PointerEvent` — ultros-app's web-sys already lists the `PointerEvent` feature under both ssr and hydrate.
- If `LocalWorldData`'s field access differs from `local_world_data.0.unwrap()`, copy exactly what the OLD file did at its line 309–310 (you're replacing that file; read it first).
- The tooltip surface uses `bg-violet-950/95`; if a chart-adjacent surface class exists in `components/tooltip.rs`'s popup, prefer matching that instead — check it and pick the closer match, noting your choice.
- If `For`'s `key=|option| option.label()` displeases the type checker, use `key=|option| *option` (GroupLevel is Copy+Eq+Hash if you add `Hash` to its derives in core — fine to do, note it).

- [ ] **Step 3: Remove chartistry CSS**

Search `style/tailwind.css` (and `ultros/static/main.css` if present) for `chartistry` / `_chartistry_` / `price-history-chart`. Remove rules that exist solely to style chartistry internals (e.g., `._chartistry_*` selectors). KEEP any rule for `.price-history-chart` itself if it does general layout. Report what you removed.

- [ ] **Step 4: Compile both targets**

```powershell
cargo check -p ultros-app
cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown
```

Both must pass. (The first compiles the ssr default; the second is the WASM client. The wasm32 target is installed — cargo-leptos builds use it.) Fix compile errors minimally; anything structural, stop and report.

- [ ] **Step 5: Run the charts crate tests once more**

Run: `cargo test -p ultros-charts --features "image leptos"`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add ultros-frontend/ultros-app/Cargo.toml ultros-frontend/ultros-app/src/components/price_history_chart.rs Cargo.lock
# plus style/tailwind.css if modified
git commit -m "feat: render the item-page price chart from ultros_charts, drop leptos-chartistry" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

Verify chartistry is gone: `rg -i chartistry -g '*.rs' -g '*.toml'` → no hits; Cargo.lock no longer lists `leptos-chartistry` after the check builds.

### Task 5: Browser verification (controller-driven checkpoint)

This task is performed by the CONTROLLER with the chrome-devtools MCP, not a subagent. Bring up the app locally (see `docs/superpowers/plans/...` is not needed — use the documented local-run procedure / `./scripts/run_e2e.sh` with a reused `$BASE_URL` if a server is already running). Then on an item page with sales data:

- [ ] Chart renders (VWAP lines + dots + grid + axis labels), stats strip shows, legend chips correct.
- [ ] Hover: crosshair + dots + tooltip follow the pointer; tooltip flips sides past the midpoint; leaves cleanly on pointerleave.
- [ ] Toggles: market avg / trend / quantity redraw correctly; quantity adds the volume lane; Color-by switches series on a DC/region scope.
- [ ] Legend chips: clicking hides/shows a series (dimmed + line-through while hidden, axes rescale); hiding everything leaves the legend visible so it can be undone.
- [ ] Window selector (7/30/90/All) still drives the chart; outlier toggle works.
- [ ] No hydration panic in the console on first load (check console for `hydration` / `unreachable`).
- [ ] Axis labels show local time after load (offset effect fired) without any flash-of-wrong-markup warnings.
- [ ] Screenshot desktop + a ~400px-wide emulated viewport (responsive rebuild).

Fixes found here go through small follow-up commits.

### Task 6: CI gate + PR

- [ ] **Step 1:** `cargo fmt --all -- --check` (autofix with `cargo fmt --all` + commit only branch-touched files).
- [ ] **Step 2:** `cargo clippy --all-targets -- -D warnings` (Strawberry Perl PATH prefix; rerun on timeout — cache resumes). Fix findings in branch-touched files properly; commit.
- [ ] **Step 3:** Push and open the PR:

```bash
git push -u origin ultros-charts-web
gh pr create --title "feat: interactive item-page price chart on ultros_charts (removes leptos-chartistry)" --body "PR 2 of 3 for the ultros_charts rewrite (spec: docs/superpowers/specs/2026-06-09-ultros-charts-design.md).

- ultros-charts: PriceChartModel (hover buckets + stats + series metadata), explicit grouping levels, viewer-timezone label offset, and a new \`leptos\` feature rendering any Scene as SVG view nodes
- ultros-app: price_history_chart.rs rewritten from ~1050 lines of chartistry plumbing to wiring over the shared model — crosshair + tooltip + toggles preserved with the same i18n keys (no new strings), legend chips now hide/show series, responsive scene rebuild at measured width
- leptos-chartistry removed from the workspace
- Hydration safety: timezone offset and container width start at deterministic defaults (UTC / 960px) for SSR + first client render and shift via post-hydration effects — same pattern as the existing hydrated gates
- The server PNG path (Discord/item card) is untouched: build_price_history_scene delegates to the new model builder, golden behavior covered by existing tests

PR 3 (interactive sparklines) follows.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

---

## Out of scope (PR 3)

Interactive `<Sparkline>` (hover dot + micro-tooltip), gap-interpolation port into `data/`, migration of Market Movers / Continue Tracking / Trends / Analyzer, deletion of `sparkline.rs`.
