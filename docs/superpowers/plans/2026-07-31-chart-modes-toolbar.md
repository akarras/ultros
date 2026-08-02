# Chart Modes & Icon Toolbar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement spec 2 of the chart revamp (`docs/superpowers/specs/2026-07-26-chart-modes-toolbar-design.md`): three new chart render modes (Candles, Range, Density), a density endpoint backed by ClickHouse, a dense icon toolbar replacing the three stacked chip rows, and a caption line replacing the stats strip.

**Architecture:** The `ultros-charts` scene-graph layout (`build_price_history_chart`) grows a `ChartMode` branch for its price lane; Candles and Range render entirely from columns the merged spec-1 `PriceSeries` payload already carries. Density gets its own wire type (`PriceDensity`), ClickHouse query, `/api/v1/price_density` endpoint (sharing the spec-1 cache and `Cache-Control` plumbing), and its own small layout. The frontend collapses its chip rows into one `ChartToolbar` (icon-only mode group, group-by menu, overlays popover) and spells the resolved state out in a caption line under the chart.

**Tech Stack:** Rust workspace — `ultros-charts` (scene graph, no framework deps), `ultros-api-types` (wire types), `ultros-clickhouse` (queries), `ultros` (axum web), `ultros-app` (Leptos 0.7 + leptos-i18n + icondata).

**Conventions that bind every task:**
- TDD: write the failing test, see it fail, implement, see it pass, commit.
- Run tests with `cargo test -p <crate>` (charts/api-types are fast; don't build the whole workspace per step).
- Before every commit: `cargo fmt --all`. Before the final push: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"` (never read a piped `$?`).
- Every user-facing string added in `ultros-app` lands in **all seven** locale files (`en`, `fr`, `de`, `ja`, `cn`, `ko`, `tc`) — the build fails otherwise. Task 10 carries the full translations.
- Spec open questions resolved here: mode **resets to Price** per visit (no localStorage — shared links and fresh visits agree); Range **supports two series** as specified.

---

### Task 1: `ChartMode` enum and options plumbing

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/charts/mod.rs`
- Modify: `ultros-frontend/ultros-charts/src/charts/price_history.rs` (options struct + Default)

- [ ] **Step 1: Write the failing test**

In `ultros-frontend/ultros-charts/src/charts/mod.rs`, append:

```rust
#[cfg(test)]
mod tests {
    use super::ChartMode;

    #[test]
    fn default_mode_is_price() {
        assert_eq!(ChartMode::default(), ChartMode::Price);
    }

    #[test]
    fn series_caps_follow_the_spec_matrix() {
        assert_eq!(ChartMode::Price.series_cap(), None);
        assert_eq!(ChartMode::Candles.series_cap(), Some(1));
        assert_eq!(ChartMode::Range.series_cap(), Some(2));
        assert_eq!(ChartMode::Density.series_cap(), Some(1));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ultros-charts charts::tests`
Expected: FAIL — `ChartMode` not found.

- [ ] **Step 3: Write the implementation**

`ultros-frontend/ultros-charts/src/charts/mod.rs` becomes:

```rust
pub mod price_history;
pub mod sparkline;

/// Which rendering the price chart uses for its price lane (spec 2 of the
/// chart revamp). `Density` is listed for the toolbar's benefit but is drawn
/// by its own layout (`price_density`, Task 8), not `price_history` — the
/// price-history layout falls back to `Price` rendering if handed `Density`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartMode {
    #[default]
    Price,
    Candles,
    Range,
    Density,
}

impl ChartMode {
    /// How many series the mode can draw at once; `None` = unlimited.
    /// Series beyond the cap are suppressed from drawing (but stay in the
    /// legend metadata) and the frontend surfaces a hint.
    pub fn series_cap(self) -> Option<usize> {
        match self {
            Self::Price => None,
            Self::Range => Some(2),
            Self::Candles | Self::Density => Some(1),
        }
    }

    /// Stable identifier for keys/debugging; user-facing names come from
    /// the app's i18n layer.
    pub fn label(self) -> &'static str {
        match self {
            Self::Price => "Price",
            Self::Candles => "Candles",
            Self::Range => "Range",
            Self::Density => "Density",
        }
    }
}
```

In `price_history.rs`, add to `PriceChartOptions` (after `hidden_series`):

```rust
    /// Price-lane rendering mode. `Density` falls back to `Price` here —
    /// density has its own layout and payload.
    pub mode: crate::charts::ChartMode,
```

and to its `Default` impl:

```rust
            mode: crate::charts::ChartMode::Price,
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-charts`
Expected: PASS (all pre-existing tests too — nothing reads `mode` yet).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-charts/src/charts/mod.rs ultros-frontend/ultros-charts/src/charts/price_history.rs
git commit -m "feat(charts): add ChartMode enum and options plumbing"
```

---

### Task 2: Theme — candle pair and density ramp

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/theme.rs`

- [ ] **Step 1: Write the failing tests**

Append to `theme.rs` tests:

```rust
    /// WCAG relative luminance — good enough to prove a greyscale render
    /// keeps the up/down distinction (the spec's colorblind-safety bar).
    fn luminance(c: Color) -> f64 {
        fn lin(u: u8) -> f64 {
            let x = u as f64 / 255.0;
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
    }

    #[test]
    fn candle_pair_survives_greyscale() {
        let theme = Theme::dark_card();
        let delta = (luminance(theme.candle_up) - luminance(theme.candle_down)).abs();
        assert!(
            delta >= 0.30,
            "candle up/down must differ in luminance by >= 0.30, got {delta:.3}"
        );
    }

    #[test]
    fn density_ramp_holds_lightness_order() {
        let theme = Theme::dark_card();
        assert_eq!(theme.density_ramp.len(), 8, "quantised to 8 opacity steps");
        for pair in theme.density_ramp.windows(2) {
            assert!(
                luminance(pair[0]) < luminance(pair[1]),
                "ramp must be strictly increasing in luminance"
            );
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros-charts theme`
Expected: FAIL — no `candle_up` field.

- [ ] **Step 3: Implement**

Add fields to `Theme` (after `trend`):

```rust
    /// Candle direction pair. Deliberately NOT red/green: the pair separates
    /// on lightness as well as hue so direction survives greyscale and the
    /// common forms of color blindness (see `candle_pair_survives_greyscale`).
    pub candle_up: Color,
    pub candle_down: Color,
    /// Sequential ramp for the density mode, darkest (fewest sales) to
    /// lightest, quantised to 8 steps so cells batch into <= 8 Path nodes.
    pub density_ramp: Vec<Color>,
```

In `Theme::base`, add:

```rust
            candle_up: Color::hex("#5eead4"),
            candle_down: Color::hex("#c2410c"),
            density_ramp: [
                "#1e1b4b", "#312e81", "#3730a3", "#4338ca", "#4f46e5", "#6366f1", "#818cf8",
                "#c7d2fe",
            ]
            .iter()
            .map(|c| Color::hex(c))
            .collect(),
```

(`#5eead4` teal ≈ luminance 0.66 vs `#c2410c` burnt orange ≈ 0.15 — passes the 0.30 bar with margin; the indigo ramp ascends in lightness.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-charts theme`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-charts/src/theme.rs
git commit -m "feat(charts): candle direction colors and density ramp in Theme"
```

---

### Task 3: Candles rendering

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/svg.rs` (path helpers)
- Modify: `ultros-frontend/ultros-charts/src/charts/price_history.rs`

- [ ] **Step 1: Write the failing svg-helper tests**

In `svg.rs` tests (next to the `dots_path_d` tests):

```rust
    #[test]
    fn rects_path_batches_and_rejects_empty() {
        assert_eq!(rects_path_d(&[]), None);
        let d = rects_path_d(&[(1.0, 2.0, 3.0, 4.0), (5.0, 6.0, 7.0, 8.0)]).unwrap();
        assert_eq!(d, "M1.0 2.0h3.0v4.0h-3.0ZM5.0 6.0h7.0v8.0h-7.0Z");
    }

    #[test]
    fn vlines_path_batches_and_rejects_empty() {
        assert_eq!(vlines_path_d(&[]), None);
        let d = vlines_path_d(&[(1.0, 2.0, 9.0), (4.0, 5.0, 6.0)]).unwrap();
        assert_eq!(d, "M1.0 2.0V9.0M4.0 5.0V6.0");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros-charts svg`
Expected: FAIL — functions not defined.

- [ ] **Step 3: Implement the helpers**

In `svg.rs`, below `dots_path_d`:

```rust
/// Path data drawing one axis-aligned rect per `(x, y, w, h)` tuple, as one
/// subpath each — candle bodies and density cells batch through this so 2,000
/// marks share one node per fill color.
pub(crate) fn rects_path_d(rects: &[(f32, f32, f32, f32)]) -> Option<String> {
    if rects.is_empty() {
        return None;
    }
    let mut d = String::with_capacity(rects.len() * 28);
    for (x, y, w, h) in rects {
        let _ = write!(d, "M{x:.1} {y:.1}h{w:.1}v{h:.1}h-{w:.1}Z");
    }
    Some(d)
}

/// Path data drawing one vertical stroke per `(x, y1, y2)` tuple — candle
/// wicks batch through this into a single stroked node.
pub(crate) fn vlines_path_d(lines: &[(f32, f32, f32)]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let mut d = String::with_capacity(lines.len() * 20);
    for (x, y1, y2) in lines {
        let _ = write!(d, "M{x:.1} {y1:.1}V{y2:.1}");
    }
    Some(d)
}
```

Run `cargo test -p ultros-charts svg` — PASS. Commit checkpoint is at the end of the task.

- [ ] **Step 4: Write the failing layout tests**

In `price_history.rs` tests:

```rust
    use crate::charts::ChartMode;

    /// A one-world series with `n` daily buckets; `sales_per_bucket` drives
    /// the sparse-candle rule.
    fn one_world_series(n: usize, sales_per_bucket: u32) -> PriceSeries {
        let buckets = (0..n)
            .map(|i| {
                let mut b = bucket(1_700_006_400 + i as i64 * 86_400, 100, 120, 90, 105, 2);
                b.sales = sales_per_bucket;
                b
            })
            .collect();
        PriceSeries {
            bucket_seconds: 86_400,
            group: SeriesGroup::World,
            from: crate::test_util::ts(1_700_006_400),
            to: crate::test_util::ts(1_700_006_400 + n as i64 * 86_400),
            series: vec![PriceSeriesEntry { id: 1, buckets }],
            raw: None,
        }
    }

    fn candle_options() -> PriceChartOptions {
        PriceChartOptions {
            mode: ChartMode::Candles,
            show_market_average: false,
            ..Default::default()
        }
    }

    #[test]
    fn candles_batch_into_single_digit_node_count() {
        let scene =
            build_price_history_scene(&world_helper(), &one_world_series(2_000, 5), &candle_options());
        let paths = count(&scene, |n| matches!(n, Node::Path { .. }));
        assert!(paths <= 3, "2,000 candles must batch into <= 3 Path nodes, got {paths}");
        assert!(paths >= 2, "expected wick + body paths");
        assert_eq!(count(&scene, |n| matches!(n, Node::Polyline { .. })), 0, "no VWAP line in candle mode");
    }

    #[test]
    fn sparse_buckets_render_wick_only_ticks() {
        // sales < 3 per bucket: range known, direction unknown -> no bodies.
        let scene =
            build_price_history_scene(&world_helper(), &one_world_series(10, 2), &candle_options());
        let fills = count(&scene, |n| matches!(n, Node::Path { fill: Some(_), .. }));
        assert_eq!(fills, 0, "sparse buckets must not grow candle bodies");
        let strokes = count(&scene, |n| matches!(n, Node::Path { stroke: Some(_), fill: None, .. }));
        assert_eq!(strokes, 1, "one batched wick path");
    }

    #[test]
    fn flat_prices_keep_a_visible_body_floor() {
        // open == close == high == low: bodies get the 1.2px floor rather
        // than disappearing.
        let mut series = one_world_series(5, 5);
        for b in &mut series.series[0].buckets {
            b.open = 100;
            b.close = 100;
            b.high = 100;
            b.low = 100;
        }
        let scene = build_price_history_scene(&world_helper(), &series, &candle_options());
        let body_d = scene
            .nodes
            .iter()
            .find_map(|n| match n {
                Node::Path { d, fill: Some(_), .. } => Some(d.clone()),
                _ => None,
            })
            .expect("flat candles must still emit a body path");
        assert!(body_d.contains("v1.2"), "zero-height bodies floor at 1.2px: {body_d}");
    }

    #[test]
    fn candles_draw_only_the_first_visible_series() {
        let model = build_price_history_chart(&world_helper(), &two_world_series(), &candle_options());
        // Metadata keeps both series (legend + the frontend's hint need them)…
        assert_eq!(model.series.len(), 2);
        // …but only one series' candles draw: bodies exist for exactly one
        // series (fixture buckets have sales=3, close >= open -> one up-path),
        // and the y-domain reflects the drawn series only.
        let fills = count(&model.scene, |n| matches!(n, Node::Path { fill: Some(_), .. }));
        assert_eq!(fills, 1, "one body path for the single drawn series");
    }
```

Note: `bucket(...)` in `test_util` builds sales=3 buckets (check `crate::test_util::bucket`'s signature before writing — the existing tests call `bucket(1_700_006_400, 100, 120, 90, 105, 2)`; keep that call shape and set `sales` explicitly where the test cares).

- [ ] **Step 5: Run to verify failure**

Run: `cargo test -p ultros-charts price_history`
Expected: FAIL — candle tests see VWAP polylines (mode is ignored so far).

- [ ] **Step 6: Implement candle rendering**

In `price_history.rs`:

1. Near the top, add imports:

```rust
use crate::charts::ChartMode;
use crate::svg::{band_path_d, dots_path_d, rects_path_d, vlines_path_d};
```

(`band_path_d` arrives in Task 4 — if implementing tasks strictly in order, import it there instead.)

2. Add the sparse-candle constant near `PriceChartOptions`:

```rust
/// Buckets with fewer sale rows than this render as a wick-only tick in
/// candle mode: "range known, direction unknown". Two sales do not make an
/// open/close trend.
pub const MIN_CANDLE_SALES: u32 = 3;
```

3. **Mode-aware visibility.** Right after `series_info` is built, replace the plain `is_hidden`-driven `visible_count` with a draw-visibility vector that also applies the mode's series cap:

```rust
    // User-hidden plus mode-suppressed: modes with a series cap draw only
    // the first `cap` visible series (spec: "shows only the first and
    // surfaces a hint"). Suppressed series stay in `series_info` so the
    // legend and the frontend hint can still name them.
    let mut draw_hidden: Vec<bool> = series_info.iter().map(|s| s.hidden).collect();
    if let Some(cap) = options.mode.series_cap() {
        let mut seen = 0usize;
        for slot in draw_hidden.iter_mut() {
            if !*slot {
                seen += 1;
                if seen > cap {
                    *slot = true;
                }
            }
        }
    }
    let visible_count = draw_hidden.iter().filter(|h| !**h).count();
```

Then change `all_visible_buckets` (and the raw-dots / VWAP loops' `series_info[index].hidden` checks) to consult `draw_hidden[index]` instead of `is_hidden(&s.name)` / `series_info[index].hidden`, so axes, stats, volume, and hover all reflect what is actually drawn. (`is_hidden` stays — it still builds `series_info`.)

```rust
    let all_visible_buckets = || {
        resolved
            .iter()
            .enumerate()
            .filter(|(i, _)| !draw_hidden[*i])
            .flat_map(|(_, s)| s.buckets.iter())
    };
```

4. **Gate raw dots to Price mode.** Wrap the existing raw-dots block:

```rust
    if options.mode == ChartMode::Price
        && let Some(raw) = &series.raw
    {
        // …existing body, with `series_info[index].hidden` -> `draw_hidden[index]`…
    }
```

5. **Branch the price lane.** Replace the "VWAP lines (the primary visual)" block's drawing part. The hover map construction must survive every mode, so first split it out — build `hover_map` in its own loop before the mode match:

```rust
    // Hover values are VWAP in every mode — the tooltip's one number stays
    // consistent as the user flips modes.
    let mut hover_map: BTreeMap<NaiveDateTime, Vec<Option<(f32, f64)>>> = BTreeMap::new();
    for (index, s) in resolved.iter().enumerate() {
        if draw_hidden[index] {
            continue;
        }
        for bucket in &s.buckets {
            let Some(vwap) = bucket.vwap() else { continue };
            hover_map
                .entry(bucket.ts)
                .or_insert_with(|| vec![None; resolved.len()])[index] =
                Some((price.scale(vwap), vwap));
        }
    }

    let half_bucket = TimeDelta::seconds(bucket_secs / 2);
    match options.mode {
        ChartMode::Price | ChartMode::Density => {
            // Existing VWAP polyline + single-series area fill, minus the
            // hover_map bookkeeping (moved above).
            for (index, s) in resolved.iter().enumerate() {
                if draw_hidden[index] {
                    continue;
                }
                let color = series_color(index);
                let line: Vec<(f32, f32)> = s
                    .buckets
                    .iter()
                    .filter_map(|b| {
                        b.vwap()
                            .map(|v| (time.scale(b.ts + half_bucket), price.scale(v)))
                    })
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
                        stroke: Stroke { color, width: 2.0, dash: None },
                    });
                }
            }
        }
        ChartMode::Candles => {
            if let Some((_, s)) = resolved
                .iter()
                .enumerate()
                .find(|(i, _)| !draw_hidden[*i])
            {
                let bucket_px =
                    time.scale(first_ts + TimeDelta::seconds(bucket_secs)) - time.scale(first_ts);
                let body_w = (bucket_px * 0.6).clamp(1.5, 18.0);
                let mut up: Vec<(f32, f32, f32, f32)> = Vec::new();
                let mut down: Vec<(f32, f32, f32, f32)> = Vec::new();
                let mut wicks: Vec<(f32, f32, f32)> = Vec::new();
                for b in &s.buckets {
                    let x = time.scale(b.ts + half_bucket);
                    wicks.push((x, price.scale(b.high as f64), price.scale(b.low as f64)));
                    if b.sales < MIN_CANDLE_SALES {
                        continue; // wick-only tick: range known, direction unknown
                    }
                    let y_open = price.scale(b.open as f64);
                    let y_close = price.scale(b.close as f64);
                    let rect = (
                        x - body_w / 2.0,
                        y_open.min(y_close),
                        body_w,
                        (y_open - y_close).abs().max(1.2),
                    );
                    if b.close >= b.open {
                        up.push(rect);
                    } else {
                        down.push(rect);
                    }
                }
                if let Some(d) = vlines_path_d(&wicks) {
                    scene.nodes.push(Node::Path {
                        d,
                        fill: None,
                        stroke: Some(Stroke {
                            color: theme.text_muted.with_alpha(0.8),
                            width: 1.0,
                            dash: None,
                        }),
                    });
                }
                if let Some(d) = rects_path_d(&up) {
                    scene.nodes.push(Node::Path { d, fill: Some(theme.candle_up), stroke: None });
                }
                if let Some(d) = rects_path_d(&down) {
                    scene.nodes.push(Node::Path { d, fill: Some(theme.candle_down), stroke: None });
                }
            }
        }
        ChartMode::Range => {
            // Task 4.
        }
    }
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p ultros-charts`
Expected: PASS — including every pre-existing test (the Price path must be behaviour-identical; `raw_sales_present_draw_one_path_per_visible_series_and_no_circles` and the hidden-series tests are the regression net for the `draw_hidden` refactor).

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-charts/src/svg.rs ultros-frontend/ultros-charts/src/charts/price_history.rs
git commit -m "feat(charts): candlestick mode with batched bodies and sparse wick-only ticks"
```

---

### Task 4: Range rendering

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/svg.rs`
- Modify: `ultros-frontend/ultros-charts/src/charts/price_history.rs`

- [ ] **Step 1: Write the failing tests**

`svg.rs`:

```rust
    #[test]
    fn band_path_closes_upper_and_lower_curves() {
        assert_eq!(band_path_d(&[], &[]), None);
        assert_eq!(band_path_d(&[(0.0, 1.0)], &[(0.0, 2.0)]), None, "a band needs 2+ points");
        let d = band_path_d(
            &[(0.0, 1.0), (10.0, 2.0)],
            &[(0.0, 5.0), (10.0, 6.0)],
        )
        .unwrap();
        assert_eq!(d, "M0.0 1.0L10.0 2.0L10.0 6.0L0.0 5.0Z");
    }
```

`price_history.rs`:

```rust
    fn range_options() -> PriceChartOptions {
        PriceChartOptions {
            mode: ChartMode::Range,
            show_market_average: false,
            ..Default::default()
        }
    }

    #[test]
    fn range_mode_emits_two_bands_and_a_median_per_series_median_last() {
        let model = build_price_history_chart(&world_helper(), &two_world_series(), &range_options());
        let bands = model
            .scene
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Path { fill: Some(_), .. }))
            .count();
        assert_eq!(bands, 4, "low-high + p25-p75 band per series, two series");
        let polylines: Vec<usize> = model
            .scene
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| matches!(n, Node::Polyline { .. }).then_some(i))
            .collect();
        assert_eq!(polylines.len(), 2, "one p50 median line per series");
        let last_band = model
            .scene
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| matches!(n, Node::Path { fill: Some(_), .. }).then_some(i))
            .max()
            .unwrap();
        assert!(
            polylines.iter().all(|i| *i > last_band),
            "medians draw after every band"
        );
    }

    #[test]
    fn range_mode_caps_at_two_visible_series() {
        let mut series = two_world_series();
        // Clone the first entry under a third resolvable id (world 3 must
        // exist in test_util's world_helper; if it doesn't, extend the
        // helper's fixture rather than using an unresolvable id).
        let mut third = series.series[0].clone();
        third.id = 3;
        series.series.push(third);
        let model = build_price_history_chart(&world_helper(), &series, &range_options());
        assert_eq!(model.series.len(), 3, "metadata keeps all three");
        let medians = model
            .scene
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Polyline { .. }))
            .count();
        assert_eq!(medians, 2, "third series is mode-suppressed");
    }
```

Check `test_util.rs` first: if `world_helper()` only defines worlds 1 and 2, add a third world to the fixture (same datacenter) as part of this task.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros-charts range`
Expected: FAIL — `band_path_d` missing; Range arm empty.

- [ ] **Step 3: Implement**

`svg.rs`:

```rust
/// Closed polygon filling the area between two curves of equal length:
/// forward along `upper`, back along `lower`. The ribbon primitive for
/// range mode — `Node::Area` can only fill to a flat baseline.
pub(crate) fn band_path_d(upper: &[(f32, f32)], lower: &[(f32, f32)]) -> Option<String> {
    if upper.len() < 2 || upper.len() != lower.len() {
        return None;
    }
    let mut d = String::with_capacity((upper.len() + lower.len()) * 12);
    for (i, (x, y)) in upper.iter().enumerate() {
        let cmd = if i == 0 { 'M' } else { 'L' };
        let _ = write!(d, "{cmd}{x:.1} {y:.1}");
    }
    for (x, y) in lower.iter().rev() {
        let _ = write!(d, "L{x:.1} {y:.1}");
    }
    d.push('Z');
    Some(d)
}
```

`price_history.rs` — fill the `ChartMode::Range` arm:

```rust
        ChartMode::Range => {
            let visible: Vec<usize> = (0..resolved.len()).filter(|i| !draw_hidden[*i]).collect();
            let curve = |s: &ResolvedSeries, f: fn(&PriceBucket) -> i32| -> Vec<(f32, f32)> {
                s.buckets
                    .iter()
                    .map(|b| (time.scale(b.ts + half_bucket), price.scale(f(b) as f64)))
                    .collect()
            };
            // Bands for every drawn series first, medians after, so the p50
            // lines always read on top of both ribbons.
            for &i in &visible {
                let s = &resolved[i];
                let color = series_color(i);
                if let Some(d) = band_path_d(&curve(s, |b| b.high), &curve(s, |b| b.low)) {
                    scene.nodes.push(Node::Path { d, fill: Some(color.with_alpha(0.08)), stroke: None });
                }
                if let Some(d) = band_path_d(&curve(s, |b| b.p75), &curve(s, |b| b.p25)) {
                    scene.nodes.push(Node::Path { d, fill: Some(color.with_alpha(0.20)), stroke: None });
                }
            }
            for &i in &visible {
                let s = &resolved[i];
                let p50 = curve(s, |b| b.p50);
                if p50.len() > 1 {
                    scene.nodes.push(Node::Polyline {
                        points: p50,
                        stroke: Stroke { color: series_color(i), width: 2.0, dash: None },
                    });
                }
            }
        }
```

(`curve` needs `ResolvedSeries` and `PriceBucket` in scope — both already are.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-charts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-charts/src/svg.rs ultros-frontend/ultros-charts/src/charts/price_history.rs ultros-frontend/ultros-charts/src/test_util.rs
git commit -m "feat(charts): range mode with quantile ribbons and median line"
```

---

### Task 5: `PriceDensity` wire types

**Files:**
- Create: `ultros-api-types/src/price_density.rs`
- Modify: `ultros-api-types/src/lib.rs` (add `pub mod price_density;` next to `pub mod price_series;` and re-export nothing extra — consumers path-qualify like they do for `price_series`)

- [ ] **Step 1: Write the file with its tests**

```rust
//! Time × price sale-count grid — the density chart's data source.
//!
//! Unlike `price_series`, cells are *sparse*: only populated
//! `(bucket, bin)` pairs are shipped. Payload is bounded by
//! `buckets × price_bins` regardless of sale volume.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// One populated cell of the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DensityCell {
    /// Bucket start, naive UTC, aligned like `PriceBucket::ts`.
    pub ts: NaiveDateTime,
    /// Price-bin index in `0..price_bins`.
    pub bin: u16,
    /// Sale-row count in the cell.
    pub n: u32,
}

/// Response payload for `/api/v1/price_density/{world}/{itemid}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceDensity {
    /// Bucket width the server actually chose (same ladder as
    /// `PriceSeries::bucket_seconds`).
    pub bucket_seconds: i64,
    /// Requested window (density has no per-bucket domain to narrow to).
    pub from: NaiveDateTime,
    pub to: NaiveDateTime,
    /// Price of bin 0's lower edge.
    pub price_lo: i32,
    /// Uniform bin height in gil; always >= 1.
    pub bin_width: f64,
    pub price_bins: u16,
    pub cells: Vec<DensityCell>,
}

impl PriceDensity {
    /// `[lower, upper)` gil bounds of a bin, for axis labels and tooltips.
    pub fn bin_bounds(&self, bin: u16) -> (f64, f64) {
        let lower = self.price_lo as f64 + bin as f64 * self.bin_width;
        (lower, lower + self.bin_width)
    }

    /// Largest cell count — the top of the opacity ramp.
    pub fn max_count(&self) -> u32 {
        self.cells.iter().map(|c| c.n).max().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn density(cells: Vec<DensityCell>) -> PriceDensity {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc();
        PriceDensity {
            bucket_seconds: 86_400,
            from: epoch,
            to: epoch,
            price_lo: 100,
            bin_width: 25.0,
            price_bins: 4,
            cells,
        }
    }

    #[test]
    fn bin_bounds_step_by_bin_width_from_lo() {
        let d = density(Vec::new());
        assert_eq!(d.bin_bounds(0), (100.0, 125.0));
        assert_eq!(d.bin_bounds(3), (175.0, 200.0));
    }

    #[test]
    fn max_count_and_empty() {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc();
        let d = density(vec![
            DensityCell { ts: epoch, bin: 0, n: 3 },
            DensityCell { ts: epoch, bin: 1, n: 9 },
        ]);
        assert_eq!(d.max_count(), 9);
        assert!(!d.is_empty());
        assert!(density(Vec::new()).is_empty());
        assert_eq!(density(Vec::new()).max_count(), 0);
    }

    #[test]
    fn round_trips_through_json() {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc();
        let d = density(vec![DensityCell { ts: epoch, bin: 2, n: 7 }]);
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<PriceDensity>(&json).unwrap(), d);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p ultros-api-types price_density`
Expected: PASS (first run compiles the new module; if `lib.rs` wasn't updated the module tests never ran — verify the test names appear in output).

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add ultros-api-types/src/price_density.rs ultros-api-types/src/lib.rs
git commit -m "feat(api-types): PriceDensity wire types"
```

---

### Task 6: ClickHouse density query

**Files:**
- Modify: `ultros-clickhouse/src/queries.rs` (below `raw_sales`)
- Create: `ultros-clickhouse/tests/price_density_smoke.rs`

- [ ] **Step 1: Write the integration test** (mirrors `price_series_smoke.rs` — same docker one-liner, `integration_enabled()` gate, `ALTER TABLE … DELETE` seeding, distinct fixture id)

```rust
//! Integration tests for the price_density aggregate.
//!
//! Run with a throwaway ClickHouse:
//!   docker run --rm -d -p 8123:8123 -e CLICKHOUSE_DB=ultros \
//!     -e CLICKHOUSE_USER=ultros -e CLICKHOUSE_PASSWORD= \
//!     --name ch-test clickhouse/clickhouse-server
//!   ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test price_density_smoke

use ultros_api_types::price_series::HqFilter;
use ultros_clickhouse::{ClickHouseClient, queries, rows::SaleRow};

fn integration_enabled() -> bool {
    std::env::var("ULTROS_CH_INTEGRATION").is_ok()
}

fn load_env() {
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();
}

/// Distinct from every id in price_series_smoke.rs (they run concurrently).
const FIXTURE_ITEM_DENSITY: i32 = 999_000_006;

fn ts(secs: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(secs, 0).unwrap()
}

const T0: i64 = 1_700_006_400; // day-aligned

async fn seed(ch: &ClickHouseClient, item: i32) {
    ch.client()
        .query("ALTER TABLE sales DELETE WHERE item_id = ? SETTINGS mutations_sync = 1")
        .bind(item)
        .execute()
        .await
        .expect("clear fixtures");
    // Prices 100..=400 across two day buckets: with lo=100, bin_width=100,
    // bins=4 the expected non-empty cells are unambiguous.
    let rows = [
        // (pg_id, offset_secs, price)
        (1, 0, 100u32),        // day 0, bin 0
        (2, 60, 150),          // day 0, bin 0
        (3, 120, 250),         // day 0, bin 1
        (4, 86_400, 400),      // day 1, bin 3 (clamped by least())
        (5, 86_460, 399),      // day 1, bin 2
    ];
    let mut insert = ch.client().insert::<SaleRow>("sales").await.expect("insert");
    for (pg_id, offset, price) in rows {
        insert
            .write(&SaleRow {
                pg_id,
                sold_date: ts(T0 + offset),
                item_id: item,
                hq: 0,
                world_id: 1,
                price_per_item: price,
                quantity: 1,
                buying_character_id: 0,
                buyer_name: String::new(),
            })
            .await
            .expect("write");
    }
    insert.end().await.expect("end insert");
}

#[tokio::test]
async fn density_bins_and_counts_match_the_fixture() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env().await.expect("client");
    seed(&ch, FIXTURE_ITEM_DENSITY).await;

    let (lo, hi) = queries::price_min_max(
        &ch,
        FIXTURE_ITEM_DENSITY,
        &[1],
        HqFilter::Any,
        ts(T0),
        ts(T0 + 3 * 86_400),
    )
    .await
    .expect("min_max query")
    .expect("fixture has rows");
    assert_eq!((lo, hi), (100, 400));

    let rows = queries::price_density(
        &ch,
        FIXTURE_ITEM_DENSITY,
        &[1],
        HqFilter::Any,
        ts(T0),
        ts(T0 + 3 * 86_400),
        86_400,
        100,   // lo
        100.0, // bin_width -> bins are [100,200) [200,300) [300,400) [400,..]
        4,
    )
    .await
    .expect("density query");

    // (bucket_offset_days, bin, n)
    let got: Vec<(i64, u16, u64)> = rows
        .iter()
        .map(|r| ((r.bucket.timestamp() - T0) / 86_400, r.price_bin, r.n))
        .collect();
    assert_eq!(got, vec![(0, 0, 2), (0, 1, 1), (1, 2, 1), (1, 3, 1)]);
}

#[tokio::test]
async fn empty_window_returns_no_min_max() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env().await.expect("client");
    let none = queries::price_min_max(
        &ch,
        FIXTURE_ITEM_DENSITY,
        &[1],
        HqFilter::Any,
        ts(0),
        ts(60), // 1970: nothing there
    )
    .await
    .expect("query");
    assert!(none.is_none(), "count()=0 must map to None, not Some((0, 0))");
}
```

(Verify `ClickHouseClient::from_env` is the constructor the sibling smoke tests use — copy whatever `price_series_smoke.rs` actually calls.)

- [ ] **Step 2: Run to verify compile failure**

Run: `cargo test -p ultros-clickhouse --test price_density_smoke`
Expected: FAIL to compile — `price_min_max` / `price_density` not defined. (Without `ULTROS_CH_INTEGRATION=1` the tests skip at runtime, but the compile check is the point here.)

- [ ] **Step 3: Implement the queries**

In `queries.rs` below `raw_sales`, following its conventions (no `FINAL`, no join, numeric-only interpolation, shared `window_predicate`):

```rust
/// One populated `(bucket, price_bin)` cell. Column order matches the SELECT.
#[derive(Debug, Clone, Row, Deserialize)]
pub struct PriceDensityRow {
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub bucket: chrono::DateTime<chrono::Utc>,
    pub price_bin: u16,
    pub n: u64,
}

#[derive(Debug, Clone, Row, Deserialize)]
struct MinMaxRow {
    count: u64,
    lo: u32,
    hi: u32,
}

/// Price extent over the window — the density endpoint derives its bin
/// layout from this before running [`price_density`]. `None` when the
/// window holds no sales (ClickHouse `min`/`max` over zero rows return 0,
/// which must not be mistaken for a real price of 0).
pub async fn price_min_max(
    ch: &ClickHouseClient,
    item_id: i32,
    world_ids: &[i32],
    hq: HqFilter,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> Result<Option<(u32, u32)>, ClickHouseError> {
    if world_ids.is_empty() {
        return Ok(None);
    }
    let worlds = world_ids
        .iter()
        .map(|w| w.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let predicate = window_predicate(item_id, &worlds, from, to, hq_predicate(hq));
    let sql = format!(
        r#"
        SELECT
            toUInt64(count())               AS count,
            toUInt32(min(price_per_item))   AS lo,
            toUInt32(max(price_per_item))   AS hi
        FROM sales
        WHERE {predicate}
        "#
    );
    let row = ch.client().query(&sql).fetch_one::<MinMaxRow>().await?;
    Ok((row.count > 0).then_some((row.lo, row.hi)))
}

/// Sale counts on a time × price grid: same predicate shape as
/// [`price_series`]/[`raw_sales`], grouped by bucket and price bin. Bins are
/// `floor((price - lo) / bin_width)` clamped into `0..bins` — the clamp
/// covers the top edge (`price == hi` lands exactly on `bins` without it)
/// and guards against a stale `lo` from a caller racing new sales.
#[allow(clippy::too_many_arguments)]
pub async fn price_density(
    ch: &ClickHouseClient,
    item_id: i32,
    world_ids: &[i32],
    hq: HqFilter,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    bucket_seconds: i64,
    lo: u32,
    bin_width: f64,
    bins: u16,
) -> Result<Vec<PriceDensityRow>, ClickHouseError> {
    if world_ids.is_empty() || bins == 0 || bin_width <= 0.0 {
        return Ok(Vec::new());
    }
    let worlds = world_ids
        .iter()
        .map(|w| w.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let predicate = window_predicate(item_id, &worlds, from, to, hq_predicate(hq));
    let max_bin = bins - 1;
    let sql = format!(
        r#"
        SELECT
            toStartOfInterval(sold_date, INTERVAL {bucket_seconds} SECOND) AS bucket,
            toUInt16(least(greatest(floor((toFloat64(price_per_item) - {lo}) / {bin_width}), 0), {max_bin})) AS price_bin,
            toUInt64(count())                                              AS n
        FROM sales
        WHERE {predicate}
        GROUP BY bucket, price_bin
        ORDER BY bucket, price_bin
        "#
    );
    Ok(ch.client().query(&sql).fetch_all::<PriceDensityRow>().await?)
}
```

- [ ] **Step 4: Compile-check, then run against docker if available**

Run: `cargo test -p ultros-clickhouse --test price_density_smoke`
Expected: compiles; tests print `skipped:` without the env var. If docker is available, run the boxed one-liner from the test header and re-run with `ULTROS_CH_INTEGRATION=1` — expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-clickhouse/src/queries.rs ultros-clickhouse/tests/price_density_smoke.rs
git commit -m "feat(clickhouse): price_density grid query and price_min_max"
```

---

### Task 7: `/api/v1/price_density` endpoint

**Files:**
- Modify: `ultros/src/web/price_series_cache.rs` (CacheKey gains `bins`)
- Modify: `ultros/src/web.rs` (query struct, helper, handler, route)

- [ ] **Step 1: Extend the cache key**

Add to `CacheKey`:

```rust
    /// Price-bin count — 0 for `price_series` entries, non-zero for
    /// `price_density`, so the two endpoints can share one cache without
    /// key collisions.
    pub bins: u16,
```

Fix the existing `price_series` handler's `CacheKey { … }` literal (add `bins: 0`) and the cache's own tests' `key()` helper. Run `cargo test -p ultros price_series_cache` — PASS.

- [ ] **Step 2: Write the failing unit test for the bin-layout helper**

In `web.rs`'s `mod price_series_tests` (or a new sibling `mod price_density_tests`):

```rust
    #[test]
    fn density_bin_width_covers_the_inclusive_range() {
        // [100, 400] over 4 bins -> width 75.25 (301 distinct prices).
        assert_eq!(super::density_bin_width(100, 400, 4), 301.0 / 4.0);
        // Degenerate flat price: floor at 1.0 so floor((p-lo)/w) stays 0.
        assert_eq!(super::density_bin_width(100, 100, 32), 1.0);
    }
```

Run: `cargo test -p ultros density_bin_width` — FAIL (undefined).

- [ ] **Step 3: Implement helper, query struct, and handler**

In `web.rs`, next to `resolve_bucket_seconds`:

```rust
/// Uniform bin height covering `[lo, hi]` inclusive in `bins` steps, floored
/// at 1 gil so degenerate windows (every sale at one price) still bin sanely.
fn density_bin_width(lo: u32, hi: u32, bins: u16) -> f64 {
    (((hi - lo) as f64 + 1.0) / bins as f64).max(1.0)
}
```

Next to `PriceSeriesQuery` (find it with `grep -n "struct PriceSeriesQuery" ultros/src/web.rs` and mirror its serde attributes exactly):

```rust
#[derive(serde::Deserialize)]
struct PriceDensityQuery {
    from: Option<i64>,
    to: Option<i64>,
    hq: Option<String>,
    bucket: Option<i64>,
    price_bins: Option<u16>,
}
```

Handler, below the `price_series` handler (same naming convention — fully qualified calls into `ultros_clickhouse::queries`):

```rust
/// `GET /api/v1/price_density/{world}/{itemid}` — sale counts on a
/// time × price grid for the chart's density mode. Same window/HQ semantics,
/// bucket ladder, cache, and `Cache-Control` plumbing as [`price_series`];
/// the payload is bounded by `buckets × price_bins` regardless of volume.
async fn price_density(
    State(world_cache): State<Arc<WorldCache>>,
    State(ch): State<ClickHouseClient>,
    State(cache): State<crate::web::price_series_cache::PriceSeriesCache>,
    Path((world, item_id)): Path<(String, i32)>,
    axum::extract::Query(query): axum::extract::Query<PriceDensityQuery>,
) -> Result<axum::response::Response, WebError> {
    let hq = match query.hq.as_deref() {
        Some("hq") => HqFilter::Hq,
        Some("nq") => HqFilter::Nq,
        _ => HqFilter::Any,
    };
    let bins = query.price_bins.unwrap_or(32).clamp(8, 96);

    let now = chrono::Utc::now();
    let to = query
        .to
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .unwrap_or(now);
    let from = query
        .from
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .unwrap_or_else(|| now - chrono::Duration::days(365 * 12));
    if from >= to {
        return Err(WebError::BadRequest);
    }

    let span_secs = (to - from).num_seconds().max(1);
    let mut bucket_seconds = resolve_bucket_seconds(query.bucket, span_secs);
    // Unlike price_series there is no post-query widening loop: the bucket
    // count is exactly span / width, known up front, so widen arithmetically
    // until the grid's time axis fits under MAX_BUCKETS.
    while span_secs / bucket_seconds > MAX_BUCKETS as i64 {
        match widen_bucket(bucket_seconds) {
            Some(wider) => bucket_seconds = wider,
            None => break,
        }
    }

    // Snap an open-ended `to` to the bucket boundary — same cache-sharing
    // rationale as price_series.
    let to = if query.to.is_none() {
        let secs = to.timestamp() - to.timestamp().rem_euclid(bucket_seconds);
        chrono::DateTime::from_timestamp(secs, 0).unwrap_or(to)
    } else {
        to
    };

    let cache_key = crate::web::price_series_cache::CacheKey {
        item_id,
        scope: world.clone(),
        from: from.timestamp(),
        to: to.timestamp(),
        bucket: bucket_seconds,
        group: "density",
        hq: hq.as_str(),
        bins,
    };
    let ttl = if query.to.is_some() {
        std::time::Duration::from_secs(3_600)
    } else {
        std::time::Duration::from_secs((bucket_seconds as u64).clamp(60, 3_600))
    };
    if let Some(hit) = cache.get(&cache_key) {
        return Ok(cached_json(hit, ttl));
    }

    let selected_value = world_cache.lookup_value_by_name(&world)?;
    let worlds = world_cache
        .get_all_worlds_in(&selected_value)
        .ok_or_else(|| Error::msg("Unable to get worlds"))?;

    let extent = ultros_clickhouse::queries::price_min_max(&ch, item_id, &worlds, hq, from, to)
        .await
        .map_err(|e| {
            tracing::warn!(error = ?e, item_id, "price_density min_max CH query failed");
            anyhow::anyhow!("ClickHouse price_min_max query failed: {e}")
        })?;

    let payload = match extent {
        None => ultros_api_types::price_density::PriceDensity {
            bucket_seconds,
            from: from.naive_utc(),
            to: to.naive_utc(),
            price_lo: 0,
            bin_width: 1.0,
            price_bins: bins,
            cells: Vec::new(),
        },
        Some((lo, hi)) => {
            let bin_width = density_bin_width(lo, hi, bins);
            let rows = ultros_clickhouse::queries::price_density(
                &ch, item_id, &worlds, hq, from, to, bucket_seconds, lo, bin_width, bins,
            )
            .await
            .map_err(|e| {
                tracing::warn!(error = ?e, item_id, "price_density CH query failed");
                anyhow::anyhow!("ClickHouse price_density query failed: {e}")
            })?;
            ultros_api_types::price_density::PriceDensity {
                bucket_seconds,
                from: from.naive_utc(),
                to: to.naive_utc(),
                price_lo: lo as i32,
                bin_width,
                price_bins: bins,
                cells: rows
                    .into_iter()
                    .map(|r| ultros_api_types::price_density::DensityCell {
                        ts: r.bucket.naive_utc(),
                        bin: r.price_bin,
                        n: u32::try_from(r.n).unwrap_or(u32::MAX),
                    })
                    .collect(),
            }
        }
    };

    let body = serde_json::to_string(&payload).map_err(anyhow::Error::from)?;
    cache.insert(cache_key, body.clone(), ttl);
    Ok(cached_json(body, ttl))
}
```

Route, next to the `price_series` route registration (`web.rs:2026`):

```rust
        .route("/api/v1/price_density/{world}/{itemid}", get(price_density))
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ultros density_bin_width && cargo test -p ultros price_series`
Expected: PASS (including the cache tests updated for `bins`).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros/src/web.rs ultros/src/web/price_series_cache.rs
git commit -m "feat(api): /api/v1/price_density endpoint sharing the price_series cache"
```

---

### Task 8: Density chart layout

**Files:**
- Create: `ultros-frontend/ultros-charts/src/charts/price_density.rs`
- Modify: `ultros-frontend/ultros-charts/src/charts/mod.rs` (`pub mod price_density;`)
- Modify: `ultros-frontend/ultros-charts/Cargo.toml` only if `ultros-api-types` isn't already a dependency (it is — `price_history.rs` imports it).

- [ ] **Step 1: Write the failing tests** (bottom of the new file; write the file skeleton with `todo!()`-free signatures first if preferred, but the test list is)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Node;
    use ultros_api_types::price_density::{DensityCell, PriceDensity};

    fn ts(secs: i64) -> chrono::NaiveDateTime {
        chrono::DateTime::from_timestamp(secs, 0).unwrap().naive_utc()
    }

    fn fixture(cells: Vec<DensityCell>) -> PriceDensity {
        PriceDensity {
            bucket_seconds: 86_400,
            from: ts(0),
            to: ts(10 * 86_400),
            price_lo: 100,
            bin_width: 50.0,
            price_bins: 8,
            cells,
        }
    }

    #[test]
    fn opacity_step_quantises_into_1_to_8() {
        assert_eq!(opacity_step(1, 1_000), 1);
        assert_eq!(opacity_step(1_000, 1_000), 8);
        assert_eq!(opacity_step(500, 1_000), 4);
        assert_eq!(opacity_step(5, 5), 8);
        assert_eq!(opacity_step(1, 1), 8);
    }

    #[test]
    fn cells_batch_into_at_most_one_node_per_step() {
        // 40 cells with counts spanning the full ramp.
        let cells: Vec<DensityCell> = (0..40)
            .map(|i| DensityCell { ts: ts((i % 10) * 86_400), bin: (i % 8) as u16, n: 1 + i as u32 })
            .collect();
        let expected_cells = cells.len();
        let model = build_price_density_chart(&fixture(cells), &DensityChartOptions::default());
        let cell_paths: Vec<&String> = model
            .scene
            .nodes
            .iter()
            .filter_map(|n| match n {
                Node::Path { d, fill: Some(_), .. } => Some(d),
                _ => None,
            })
            .collect();
        assert!(cell_paths.len() <= 8, "one node per opacity step max, got {}", cell_paths.len());
        let subpaths: usize = cell_paths.iter().map(|d| d.matches('M').count()).sum();
        assert_eq!(subpaths, expected_cells, "every populated cell draws exactly once");
    }

    #[test]
    fn empty_grid_renders_the_no_data_card() {
        let model = build_price_density_chart(&fixture(Vec::new()), &DensityChartOptions::default());
        assert!(model.hover.buckets.is_empty());
        assert!(
            model
                .scene
                .nodes
                .iter()
                .any(|n| matches!(n, Node::Text { content, .. } if content == "No recent sales"))
        );
    }

    #[test]
    fn hover_buckets_cover_each_populated_time_bucket_once() {
        let cells = vec![
            DensityCell { ts: ts(0), bin: 0, n: 2 },
            DensityCell { ts: ts(0), bin: 3, n: 1 },
            DensityCell { ts: ts(86_400), bin: 1, n: 4 },
        ];
        let model = build_price_density_chart(&fixture(cells), &DensityChartOptions::default());
        assert_eq!(model.hover.buckets.len(), 2, "two distinct timestamps");
        assert!(model.hover.buckets.windows(2).all(|w| w[0].x <= w[1].x));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ultros-charts price_density`
Expected: FAIL — module/functions missing.

- [ ] **Step 3: Implement the layout**

```rust
//! Layout for the density mode: a time × price grid, each cell shaded by
//! sale count. Data comes from `/api/v1/price_density` (sparse populated
//! cells only). Cells are quantised onto the theme's 8-step ramp and
//! batched into one `Node::Path` per step — at full history this is the
//! cheapest mode to render, not the most expensive.

use std::collections::BTreeMap;

use chrono::TimeDelta;
use ultros_api_types::price_density::PriceDensity;

use crate::charts::price_history::{HoverBucket, HoverModel};
use crate::scale::{LinearScale, TimeScale, short_number};
use crate::scene::{Node, Scene, Stroke, TextAnchor};
use crate::svg::rects_path_d;
use crate::theme::Theme;

#[derive(Clone, Debug)]
pub struct DensityChartOptions {
    pub width: f32,
    pub height: f32,
    /// Label shift for viewer-local times; geometry stays UTC (same contract
    /// as `PriceChartOptions::utc_offset_minutes`).
    pub utc_offset_minutes: i32,
    pub theme: Theme,
}

impl Default for DensityChartOptions {
    fn default() -> Self {
        Self {
            width: 960.0,
            height: 540.0,
            utc_offset_minutes: 0,
            theme: Theme::dark_card(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DensityChartModel {
    pub scene: Scene,
    pub hover: HoverModel,
    /// Total sale rows in the grid, for the caption line.
    pub total_sales: u64,
}

/// Ramp index for a cell: `1..=8`, linear in `n / max_n`. `max_n` cells land
/// on 8; the smallest non-zero counts land on 1.
pub(crate) fn opacity_step(n: u32, max_n: u32) -> usize {
    if max_n == 0 {
        return 1;
    }
    (((n as u64 * 8).div_ceil(max_n as u64)) as usize).clamp(1, 8)
}

pub fn build_price_density_chart(
    density: &PriceDensity,
    options: &DensityChartOptions,
) -> DensityChartModel {
    let theme = &options.theme;
    let mut scene = Scene {
        width: options.width,
        height: options.height,
        background: theme.background,
        font_family: theme.font_family.clone(),
        nodes: Vec::new(),
    };

    if density.is_empty() {
        scene.nodes.push(Node::Text {
            x: options.width / 2.0,
            y: options.height / 2.0,
            content: "No recent sales".to_string(),
            size: 22.0,
            color: theme.text_muted,
            anchor: TextAnchor::Middle,
            bold: false,
        });
        return DensityChartModel {
            scene,
            hover: HoverModel { plot_top: 0.0, plot_bottom: 0.0, buckets: Vec::new() },
            total_sales: 0,
        };
    }

    // ── Geometry: same frame as price_history without title/volume lanes ──
    let plot_top = 12.0;
    let plot_left = 68.0;
    let plot_right = options.width - 16.0;
    let plot_bottom = options.height - 32.0;

    let bucket_secs = density.bucket_seconds.max(1);
    let first_ts = density.cells.iter().map(|c| c.ts).min().expect("non-empty");
    let last_ts = density.cells.iter().map(|c| c.ts).max().expect("non-empty");
    let time = TimeScale::new(
        first_ts,
        last_ts + TimeDelta::seconds(bucket_secs),
        (plot_left, plot_right),
    );
    let price_top = density.price_lo as f64 + density.bin_width * density.price_bins as f64;
    let price = LinearScale::new((density.price_lo as f64, price_top), (plot_bottom, plot_top));

    // ── Grid + axis labels (mirrors price_history) ──
    for tick in price.ticks(5) {
        let y = price.scale(tick);
        scene.nodes.push(Node::Line {
            x1: plot_left,
            y1: y,
            x2: plot_right,
            y2: y,
            stroke: Stroke { color: theme.grid, width: 1.0, dash: None },
        });
        scene.nodes.push(Node::Text {
            x: plot_left - 8.0,
            y: y + 4.0,
            content: short_number(tick.round() as i32),
            size: 13.0,
            color: theme.text_muted,
            anchor: TextAnchor::End,
            bold: false,
        });
    }
    let x_tick_target = ((options.width / 150.0) as usize).clamp(3, 8);
    for tick in time.ticks(x_tick_target, options.utc_offset_minutes) {
        scene.nodes.push(Node::Text {
            x: time.scale(tick.ts),
            y: plot_bottom + 20.0,
            content: tick.label,
            size: 13.0,
            color: theme.text_muted,
            anchor: TextAnchor::Middle,
            bold: false,
        });
    }

    // ── Cells, one batched path per ramp step ──
    let max_n = density.max_count();
    let cell_w = (time.scale(first_ts + TimeDelta::seconds(bucket_secs)) - time.scale(first_ts))
        .max(1.0);
    let cell_h = ((plot_bottom - plot_top) / density.price_bins.max(1) as f32).max(1.0);
    let mut by_step: Vec<Vec<(f32, f32, f32, f32)>> = vec![Vec::new(); 8];
    for cell in &density.cells {
        let x = time.scale(cell.ts);
        let (_, upper) = density.bin_bounds(cell.bin);
        let y = price.scale(upper);
        by_step[opacity_step(cell.n, max_n) - 1].push((x, y, cell_w, cell_h));
    }
    for (step, rects) in by_step.iter().enumerate() {
        if let Some(d) = rects_path_d(rects) {
            let color = theme.density_ramp[step.min(theme.density_ramp.len() - 1)];
            scene.nodes.push(Node::Path { d, fill: Some(color), stroke: None });
        }
    }

    // ── Hover: one bucket per populated timestamp ──
    let label_format = if bucket_secs < 86_400 { "%m-%d %H:%M" } else { "%Y-%m-%d" };
    let mut per_ts: BTreeMap<chrono::NaiveDateTime, u64> = BTreeMap::new();
    for cell in &density.cells {
        *per_ts.entry(cell.ts).or_insert(0) += cell.n as u64;
    }
    let total_sales: u64 = per_ts.values().sum();
    let buckets: Vec<HoverBucket> = per_ts
        .into_iter()
        .map(|(ts, n)| {
            let center = ts + TimeDelta::seconds(bucket_secs / 2);
            let display = center + TimeDelta::minutes(options.utc_offset_minutes as i64);
            HoverBucket {
                x: time.scale(center),
                label: display.format(label_format).to_string(),
                series_values: Vec::new(),
                volume: n as i64,
            }
        })
        .collect();

    DensityChartModel {
        scene,
        hover: HoverModel { plot_top, plot_bottom, buckets },
        total_sales,
    }
}
```

`rects_path_d` and the `HoverBucket`/`HoverModel` types must be reachable: make `HoverBucket`, `HoverModel` stay `pub` in `price_history` (they are), and `rects_path_d` is `pub(crate)` (Task 3). Check `TimeScale::new`/`ticks` signatures against `scale.rs` while implementing — mirror exactly how `price_history.rs` calls them.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros-charts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-charts/src/charts/price_density.rs ultros-frontend/ultros-charts/src/charts/mod.rs
git commit -m "feat(charts): density grid layout with 8-step batched cells"
```

---

### Task 9: Frontend density fetch

**Files:**
- Modify: `ultros-frontend/ultros-app/src/api.rs` (below `get_price_series`)

- [ ] **Step 1: Implement** (thin wrapper — the pattern has no unit tests in this file; compile is the check)

```rust
/// Time × price sale-count grid for the chart's density mode. Fetched only
/// while density mode is active — see the gated LocalResource in item_view.
pub(crate) async fn get_price_density(
    item_id: i32,
    world: &str,
    hq: HqFilter,
    range: Option<(i64, i64)>,
    price_bins: u16,
) -> AppResult<PriceDensity> {
    if item_id == 0 {
        return Err(AppError::NoItem);
    }
    let mut url = format!(
        "/api/v1/price_density/{world}/{item_id}?hq={}&price_bins={price_bins}",
        hq.as_str()
    );
    if let Some((from, to)) = range {
        url.push_str(&format!("&from={from}&to={to}"));
    }
    fetch_api(&url).await
}
```

Add `use ultros_api_types::price_density::PriceDensity;` to the file's imports (next to the existing `price_series` import block).

- [ ] **Step 2: Compile**

Run: `cargo check -p ultros-app`
Expected: clean (one `dead_code` warning is acceptable until Task 13 wires it; if clippy `-D warnings` would trip, add the call site in the same commit as Task 13 instead — in that case just leave this staged and fold it into Task 13's commit).

- [ ] **Step 3: Commit** (or fold into Task 13 per the note above)

```bash
cargo fmt --all
git add ultros-frontend/ultros-app/src/api.rs
git commit -m "feat(app): get_price_density API wrapper"
```

---

### Task 10: i18n keys — all seven locales

**Files:**
- Modify: `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json`

- [ ] **Step 1: Add the keys.** Every file gets all ten keys (flat JSON, alongside the existing `chart_*` keys). Interpolation uses the codebase's `.replace("{x}", …)` convention, not leptos-i18n variables — keep the literal `{name}`/`{group}` braces.

`en.json`:
```json
"chart_mode_price": "Price line",
"chart_mode_candles": "Candlesticks",
"chart_mode_range": "Price range",
"chart_mode_density": "Sale density",
"chart_toolbar_mode_group": "Chart mode",
"chart_toolbar_overlays": "Overlays",
"chart_hint_single_series": "This mode shows one series at a time — showing {name} only.",
"chart_hint_range_limit": "Range mode shows the first two series only.",
"chart_caption_grouped_by": "grouped by {group}",
"chart_density_quantity_unavailable": "The quantity lane isn't available in density mode."
```

`fr.json`:
```json
"chart_mode_price": "Courbe de prix",
"chart_mode_candles": "Chandeliers",
"chart_mode_range": "Plage de prix",
"chart_mode_density": "Densité des ventes",
"chart_toolbar_mode_group": "Mode du graphique",
"chart_toolbar_overlays": "Superpositions",
"chart_hint_single_series": "Ce mode n'affiche qu'une série à la fois — seule {name} est affichée.",
"chart_hint_range_limit": "Le mode plage n'affiche que les deux premières séries.",
"chart_caption_grouped_by": "groupé par {group}",
"chart_density_quantity_unavailable": "La bande de quantité n'est pas disponible en mode densité."
```

`de.json`:
```json
"chart_mode_price": "Preislinie",
"chart_mode_candles": "Kerzenchart",
"chart_mode_range": "Preisspanne",
"chart_mode_density": "Verkaufsdichte",
"chart_toolbar_mode_group": "Diagrammmodus",
"chart_toolbar_overlays": "Überlagerungen",
"chart_hint_single_series": "Dieser Modus zeigt nur eine Serie gleichzeitig — nur {name} wird angezeigt.",
"chart_hint_range_limit": "Der Spannenmodus zeigt nur die ersten beiden Serien.",
"chart_caption_grouped_by": "gruppiert nach {group}",
"chart_density_quantity_unavailable": "Die Mengenspur ist im Dichtemodus nicht verfügbar."
```

`ja.json`:
```json
"chart_mode_price": "価格ライン",
"chart_mode_candles": "ローソク足",
"chart_mode_range": "価格レンジ",
"chart_mode_density": "売買密度",
"chart_toolbar_mode_group": "チャートモード",
"chart_toolbar_overlays": "オーバーレイ",
"chart_hint_single_series": "このモードでは一度に1系列のみ表示できます — 現在は{name}のみ表示中です。",
"chart_hint_range_limit": "レンジモードでは最初の2系列のみ表示されます。",
"chart_caption_grouped_by": "{group}別",
"chart_density_quantity_unavailable": "密度モードでは数量レーンを利用できません。"
```

`cn.json`:
```json
"chart_mode_price": "价格曲线",
"chart_mode_candles": "K线图",
"chart_mode_range": "价格区间",
"chart_mode_density": "成交密度",
"chart_toolbar_mode_group": "图表模式",
"chart_toolbar_overlays": "叠加层",
"chart_hint_single_series": "此模式一次只能显示一个系列 — 当前仅显示{name}。",
"chart_hint_range_limit": "区间模式仅显示前两个系列。",
"chart_caption_grouped_by": "按{group}分组",
"chart_density_quantity_unavailable": "密度模式下无法使用数量栏。"
```

`ko.json`:
```json
"chart_mode_price": "가격 라인",
"chart_mode_candles": "캔들 차트",
"chart_mode_range": "가격 범위",
"chart_mode_density": "판매 밀도",
"chart_toolbar_mode_group": "차트 모드",
"chart_toolbar_overlays": "오버레이",
"chart_hint_single_series": "이 모드는 한 번에 하나의 시리즈만 표시합니다 — 현재 {name}만 표시 중입니다.",
"chart_hint_range_limit": "범위 모드는 처음 두 시리즈만 표시합니다.",
"chart_caption_grouped_by": "{group}별",
"chart_density_quantity_unavailable": "밀도 모드에서는 수량 레인을 사용할 수 없습니다."
```

`tc.json`:
```json
"chart_mode_price": "價格曲線",
"chart_mode_candles": "K線圖",
"chart_mode_range": "價格區間",
"chart_mode_density": "成交密度",
"chart_toolbar_mode_group": "圖表模式",
"chart_toolbar_overlays": "疊加層",
"chart_hint_single_series": "此模式一次只能顯示一個系列 — 目前僅顯示{name}。",
"chart_hint_range_limit": "區間模式僅顯示前兩個系列。",
"chart_caption_grouped_by": "按{group}分組",
"chart_density_quantity_unavailable": "密度模式下無法使用數量欄。"
```

- [ ] **Step 2: Verify the build accepts them**

Run: `cargo check -p ultros-app`
Expected: compiles; no missing-key warnings for `chart_*`.

- [ ] **Step 3: Commit**

```bash
git add ultros-frontend/ultros-app/locales/
git commit -m "feat(i18n): chart mode and toolbar strings in all locales"
```

---

### Task 11: `ChartToolbar` component

**Files:**
- Create: `ultros-frontend/ultros-app/src/components/chart_toolbar.rs`
- Modify: `ultros-frontend/ultros-app/src/components.rs` (or `components/mod.rs` — wherever siblings register; check with `grep -rn "price_history_chart" ultros-frontend/ultros-app/src/components*`)

The toolbar collapses the current three stacked rows into one horizontally-scrollable flex row, left to right: **mode** (icon-only segmented group, 4 items) · *(slot reserved for spec 3's view toggle)* · **group by** (dropdown chip showing current value as text, icons beside labels in the menu) · *(slot reserved for spec 3's world filter)* · **overlays** (chip with count badge, popover with the three toggles).

Icons (all resolve in the vendored `icondata` 0.7 — verified against the registry): `TbChartLineOutline`, `TbChartCandleOutline`, `TbChartAreaLineOutline`, `TbChartGridDotsOutline`, `TbAdjustmentsHorizontalOutline`, `TbStack2Outline` (Region), `TbCirclesOutline` (Datacenter), `TbPointFilled` (World).

- [ ] **Step 1: Write the component**

```rust
use icondata as i;
use leptos::prelude::*;
use ultros_charts::charts::ChartMode;
use ultros_charts::data::grouping::GroupLevel;

use crate::components::icon::Icon;
use crate::i18n::{t_string, use_i18n};

fn mode_icon(mode: ChartMode) -> icondata_core::Icon {
    match mode {
        ChartMode::Price => i::TbChartLineOutline,
        ChartMode::Candles => i::TbChartCandleOutline,
        ChartMode::Range => i::TbChartAreaLineOutline,
        ChartMode::Density => i::TbChartGridDotsOutline,
    }
}

fn group_icon(level: GroupLevel) -> icondata_core::Icon {
    match level {
        GroupLevel::Region => i::TbStack2Outline,
        GroupLevel::Datacenter => i::TbCirclesOutline,
        GroupLevel::World => i::TbPointFilled,
    }
}

const CHIP: &str = "inline-flex items-center gap-1.5 rounded-md border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--color-text)_4%,_transparent)] px-2.5 py-1 text-xs text-[color:var(--color-text-muted)] transition-colors hover:text-[color:var(--color-text)]";

#[component]
pub fn ChartToolbar(
    #[prop(into)] mode: Signal<ChartMode>,
    set_mode: WriteSignal<ChartMode>,
    #[prop(into)] group_options: Signal<Vec<GroupLevel>>,
    #[prop(into)] group: Signal<GroupLevel>,
    set_group: WriteSignal<GroupLevel>,
    #[prop(into)] show_market_average: Signal<bool>,
    set_show_market_average: WriteSignal<bool>,
    #[prop(into)] show_trend: Signal<bool>,
    set_show_trend: WriteSignal<bool>,
    #[prop(into)] show_quantity: Signal<bool>,
    set_show_quantity: WriteSignal<bool>,
    /// Density mode has no quantity lane; the toggle stays visible but
    /// disabled with a reason (spec: disabled, never hidden).
    #[prop(into)] quantity_disabled: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let (group_open, set_group_open) = signal(false);
    let (overlays_open, set_overlays_open) = signal(false);

    let mode_name = move |m: ChartMode| match m {
        ChartMode::Price => t_string!(i18n, chart_mode_price).to_string(),
        ChartMode::Candles => t_string!(i18n, chart_mode_candles).to_string(),
        ChartMode::Range => t_string!(i18n, chart_mode_range).to_string(),
        ChartMode::Density => t_string!(i18n, chart_mode_density).to_string(),
    };
    let group_name = move |g: GroupLevel| match g {
        GroupLevel::Region => t_string!(i18n, chart_color_region).to_string(),
        GroupLevel::Datacenter => t_string!(i18n, chart_color_datacenter).to_string(),
        GroupLevel::World => t_string!(i18n, chart_color_world).to_string(),
    };
    let overlay_count = Signal::derive(move || {
        [show_market_average.get(), show_trend.get(), show_quantity.get()]
            .iter()
            .filter(|on| **on)
            .count()
    });

    view! {
        <div class="flex items-center gap-2 overflow-x-auto text-xs">
            // ── Mode: icon-only segmented group ──
            <div
                role="group"
                aria-label=move || t_string!(i18n, chart_toolbar_mode_group).to_string()
                class="inline-flex shrink-0 overflow-hidden rounded-md border border-[color:var(--color-outline)]"
            >
                {[ChartMode::Price, ChartMode::Candles, ChartMode::Range, ChartMode::Density]
                    .into_iter()
                    .map(|m| {
                        view! {
                            <button
                                type="button"
                                aria-label=move || mode_name(m)
                                aria-pressed=move || (mode.get() == m).to_string()
                                class=move || {
                                    let active = mode.get() == m;
                                    [
                                        "border-l border-[color:var(--color-outline)] px-2.5 py-1.5 transition-colors first:border-l-0",
                                        if active {
                                            "bg-brand-600/30 text-brand-100"
                                        } else {
                                            "bg-[color:color-mix(in_srgb,_var(--color-text)_4%,_transparent)] text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
                                        },
                                    ]
                                    .join(" ")
                                }
                                on:click=move |_| set_mode.set(m)
                            >
                                <Icon height="1.1em" width="1.1em" icon=mode_icon(m) />
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
            // (slot: spec 3 view toggle)
            // ── Group by: dropdown chip ──
            <Show when=move || group_options.with(|o| o.len() > 1)>
                <div class="relative shrink-0">
                    <button
                        type="button"
                        class=CHIP
                        aria-haspopup="menu"
                        aria-expanded=move || group_open.get().to_string()
                        on:click=move |_| set_group_open.update(|open| *open = !*open)
                    >
                        <Icon height="1.0em" width="1.0em" icon=group_icon(group.get()) />
                        {move || group_name(group.get())}
                    </button>
                    <Show when=move || group_open.get()>
                        <div
                            role="menu"
                            class="absolute left-0 top-full z-20 mt-1 min-w-36 rounded-md border border-[color:var(--color-outline)] bg-violet-950/95 py-1 shadow-lg"
                        >
                            {move || {
                                group_options
                                    .get()
                                    .into_iter()
                                    .map(|level| {
                                        view! {
                                            <button
                                                type="button"
                                                role="menuitem"
                                                class=move || {
                                                    let active = group.get() == level;
                                                    [
                                                        "flex w-full items-center gap-2 px-3 py-1.5 text-left transition-colors hover:bg-brand-600/20",
                                                        if active { "text-brand-100" } else { "text-[color:var(--color-text-muted)]" },
                                                    ]
                                                    .join(" ")
                                                }
                                                on:click=move |_| {
                                                    set_group.set(level);
                                                    set_group_open.set(false);
                                                }
                                            >
                                                <Icon height="1.0em" width="1.0em" icon=group_icon(level) />
                                                {group_name(level)}
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </Show>
                </div>
            </Show>
            // (slot: spec 3 world filter)
            // ── Overlays: chip + count badge + popover ──
            <div class="relative shrink-0">
                <button
                    type="button"
                    class=CHIP
                    aria-haspopup="menu"
                    aria-expanded=move || overlays_open.get().to_string()
                    aria-label=move || t_string!(i18n, chart_toolbar_overlays).to_string()
                    on:click=move |_| set_overlays_open.update(|open| *open = !*open)
                >
                    <Icon height="1.0em" width="1.0em" icon=i::TbAdjustmentsHorizontalOutline />
                    {move || t_string!(i18n, chart_toolbar_overlays).to_string()}
                    <span class="inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-brand-600/40 px-1 text-[10px] tabular-nums text-brand-100">
                        {move || overlay_count.get()}
                    </span>
                </button>
                <Show when=move || overlays_open.get()>
                    <div class="absolute left-0 top-full z-20 mt-1 min-w-52 rounded-md border border-[color:var(--color-outline)] bg-violet-950/95 px-3 py-2 shadow-lg">
                        <OverlayRow
                            label=Signal::derive(move || t_string!(i18n, chart_toggle_market_avg).to_string())
                            checked=show_market_average
                            set_checked=set_show_market_average
                            disabled=Signal::derive(|| false)
                            disabled_reason=Signal::derive(String::new)
                        />
                        <OverlayRow
                            label=Signal::derive(move || t_string!(i18n, chart_legend_trend).to_string())
                            checked=show_trend
                            set_checked=set_show_trend
                            disabled=Signal::derive(|| false)
                            disabled_reason=Signal::derive(String::new)
                        />
                        <OverlayRow
                            label=Signal::derive(move || t_string!(i18n, chart_legend_quantity).to_string())
                            checked=show_quantity
                            set_checked=set_show_quantity
                            disabled=quantity_disabled
                            disabled_reason=Signal::derive(move || {
                                t_string!(i18n, chart_density_quantity_unavailable).to_string()
                            })
                        />
                    </div>
                </Show>
            </div>
        </div>
    }
}

/// One labelled checkbox row in the overlays popover. Disabled rows keep
/// their space and carry the reason as a title tooltip + aria-description —
/// a control that vanishes reads as a bug.
#[component]
fn OverlayRow(
    #[prop(into)] label: Signal<String>,
    #[prop(into)] checked: Signal<bool>,
    set_checked: WriteSignal<bool>,
    #[prop(into)] disabled: Signal<bool>,
    #[prop(into)] disabled_reason: Signal<String>,
) -> impl IntoView {
    view! {
        <label
            class=move || {
                [
                    "flex cursor-pointer select-none items-center justify-between gap-3 py-1",
                    if disabled.get() { "cursor-not-allowed opacity-45" } else { "" },
                ]
                .join(" ")
            }
            title=move || disabled.get().then(|| disabled_reason.get()).unwrap_or_default()
        >
            <span class="text-[color:var(--color-text)]">{label}</span>
            <input
                type="checkbox"
                class="accent-[color:var(--color-brand,#8b5cf6)]"
                prop:checked=checked
                prop:disabled=disabled
                on:change=move |event| {
                    set_checked.set(event_target_checked(&event));
                }
            />
        </label>
    }
}
```

Register the module beside `price_history_chart` in the components module list. Check `crate::components::icon::Icon`'s prop types (`icon=` takes `icondata_core::Icon`? — read `components/icon.rs` and match; `apps_menu.rs` passes `i::MdiJellyfish` bare, so the constant type is whatever `Icon` declares — mirror it in `mode_icon`/`group_icon` return types).

- [ ] **Step 2: Compile**

Run: `cargo check -p ultros-app`
Expected: clean apart from the not-yet-used component (wired next task; leptos `#[component]` items don't trip dead_code, but if anything does, fold this commit into Task 12's).

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-app/src/components/chart_toolbar.rs ultros-frontend/ultros-app/src/components.rs
git commit -m "feat(app): ChartToolbar with icon mode group, group-by menu, overlays popover"
```

---

### Task 12: Caption line + toolbar swap in `PriceHistoryChart`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs`

- [ ] **Step 1: Add the mode/density props and swap the control rows**

`PriceHistoryChart`'s signature grows:

```rust
#[component]
pub fn PriceHistoryChart(
    #[prop(into)] series: Signal<Option<PriceSeries>>,
    #[prop(into)] density: Signal<Option<PriceDensity>>,
    #[prop(into)] scope_name: Signal<String>,
    #[prop(into)] mode: Signal<ChartMode>,
    set_mode: WriteSignal<ChartMode>,
    #[prop(into)] group: Signal<GroupLevel>,
    set_group: WriteSignal<GroupLevel>,
    #[prop(into)] on_range_change: Callback<Option<(i64, i64)>>,
) -> impl IntoView {
```

with imports:

```rust
use ultros_api_types::price_density::PriceDensity;
use ultros_charts::charts::ChartMode;
use ultros_charts::charts::price_density::{
    DensityChartModel, DensityChartOptions, build_price_density_chart,
};
use crate::components::chart_toolbar::ChartToolbar;
```

In the `view!`, delete the three-`ChartOverlayToggle` row and the `<ColorByControl …/>` line; in their place:

```rust
            <ChartToolbar
                mode=mode
                set_mode=set_mode
                group_options=color_by_options
                group=group
                set_group=set_group
                show_market_average=show_market_average
                set_show_market_average=set_show_market_average
                show_trend=show_trend
                set_show_trend=set_show_trend
                show_quantity=show_quantity
                set_show_quantity=set_show_quantity
                quantity_disabled=Signal::derive(move || mode.get() == ChartMode::Density)
            />
```

Delete the now-unused `ChartOverlayToggle` and `ColorByControl` components from this file (clippy `-D warnings` will flag them as dead code otherwise).

Pass the mode into the model options: in the `model` memo add `mode: mode.get(),` and gate the volume lane off in density (`show_volume: show_quantity.get() && mode.get() != ChartMode::Density,`).

- [ ] **Step 2: Replace `StatsStrip` with the caption line**

Delete the `StatsStrip` component and its `<StatsStrip stats=stats />` usage at the top of the layout. After the chart `<div role="img">…</div>` block (before the legend), insert:

```rust
            // Caption line: the resolved state spelled out once — what makes
            // an icon-only toolbar viable (works on touch, read by screen
            // readers, no icon carries meaning alone).
            {move || {
                let s = stats.get();
                let mode_label = match mode.get() {
                    ChartMode::Price => t_string!(i18n, chart_mode_price).to_string(),
                    ChartMode::Candles => t_string!(i18n, chart_mode_candles).to_string(),
                    ChartMode::Range => t_string!(i18n, chart_mode_range).to_string(),
                    ChartMode::Density => t_string!(i18n, chart_mode_density).to_string(),
                };
                let grouped = color_by_options
                    .with(|o| o.len() > 1)
                    .then(|| {
                        let group_label = match group.get() {
                            GroupLevel::Region => t_string!(i18n, chart_color_region).to_string(),
                            GroupLevel::Datacenter => {
                                t_string!(i18n, chart_color_datacenter).to_string()
                            }
                            GroupLevel::World => t_string!(i18n, chart_color_world).to_string(),
                        };
                        t_string!(i18n, chart_caption_grouped_by)
                            .to_string()
                            .replace("{group}", &group_label)
                    });
                view! {
                    <div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs tabular-nums text-[color:var(--color-text)]/70">
                        <span>{mode_label}</span>
                        {grouped.map(|g| view! { <span>"· " {g}</span> })}
                        {s.as_ref()
                            .map(|s| {
                                let n_label = t_string!(i18n, chart_stat_n_sales)
                                    .to_string()
                                    .replace("{n}", &s.n.to_string());
                                view! { <span>"· " {n_label}</span> }
                            })}
                        {s.as_ref()
                            .and_then(|s| s.market_average)
                            .map(|v| {
                                view! {
                                    <span>
                                        "· " {t_string!(i18n, chart_stat_market_avg).to_string()} " "
                                        {short_number(v)}
                                    </span>
                                }
                            })}
                        {s.as_ref()
                            .and_then(|s| s.median)
                            .map(|v| {
                                view! {
                                    <span>
                                        "· " {t_string!(i18n, chart_stat_median).to_string()} " "
                                        {short_number(v)}
                                    </span>
                                }
                            })}
                    </div>
                }
            }}
```

(`chart_stat_min`/`chart_stat_max` drop out of the default view per the spec's caption example; the keys stay in the locale files — they're still referenced by other surfaces. Verify with `grep -rn "chart_stat_min" ultros-frontend/ultros-app/src` — if this file was the only consumer, leave the keys anyway; removing keys from seven locales for two strings isn't worth the churn.)

- [ ] **Step 3: Compile**

Run: `cargo check -p ultros-app`
Expected: errors only in `item_view.rs` (missing new props at the call site) — expected until Task 13. If so, proceed; commit lands with Task 13.

---

### Task 13: Mode wiring — item_view, density view, hints

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs`
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs`

- [ ] **Step 1: item_view — own the mode signal, gate the density fetch**

Next to `let (group, set_group) = signal(GroupLevel::World);` (item_view.rs ~line 1256):

```rust
    let (mode, set_mode) = signal(ChartMode::Price);
```

with `use ultros_charts::charts::ChartMode;` and `use crate::api::get_price_density;` added to the imports.

Below `series_resource`:

```rust
    // Fetched only while density mode is active — the mode is the gate, so
    // flipping to Density triggers the fetch and every other mode costs
    // nothing. Same LocalResource/hydration rationale as series_resource.
    let density_resource = LocalResource::new(move || {
        let active = mode.get() == ChartMode::Density;
        let id = item_id.get();
        let world_name = world.get();
        let hq_filter = hq.get();
        let range = debounced_range.get();
        async move {
            if !active {
                return None;
            }
            get_price_density(id, &world_name, hq_filter, range, 32).await.ok()
        }
    });
    let density = Signal::derive(move || density_resource.get().flatten());
```

Update the `<PriceHistoryChart …/>` call site (~line 1393):

```rust
                                <PriceHistoryChart
                                    series=series
                                    density=density
                                    scope_name=…  // unchanged existing props
                                    mode=mode
                                    set_mode=set_mode
                                    group=group
                                    set_group=set_group
                                    on_range_change=Callback::new(move |r| set_selected_range.set(r))
                                />
```

Mode deliberately does **not** reset on item/world change and mode switches never touch `selected_range` or `group` — that is the spec's "switching mode preserves the time window and grouping".

- [ ] **Step 2: price_history_chart — density model and swap-in view**

Inside `PriceHistoryChart`, after the existing `model` memo:

```rust
    let density_model = Memo::new(move |_| {
        let width = chart_width.get();
        let height = (width * 0.56).clamp(300.0, 540.0);
        density.get().map(|d| {
            build_price_density_chart(
                &d,
                &DensityChartOptions {
                    width,
                    height,
                    utc_offset_minutes: utc_offset.get(),
                    theme: Theme::site(),
                },
            )
        })
    });
```

In the chart-rendering closure (`{move || { let m = model.get(); … }}`), branch on mode first:

```rust
                {move || {
                    if mode.get() == ChartMode::Density {
                        let Some(dm) = density_model.get() else {
                            // Fetch in flight (or errored): keep the frame
                            // with the standard empty text.
                            let msg = t_string!(i18n, chart_no_sales_in_window).to_string();
                            return view! {
                                <div class="flex items-center justify-center w-full h-full text-[color:var(--color-text)]/60 text-sm">
                                    {msg}
                                </div>
                            }
                            .into_any();
                        };
                        if dm.hover.buckets.is_empty() {
                            let msg = t_string!(i18n, chart_no_sales_in_window).to_string();
                            return view! {
                                <div class="flex items-center justify-center w-full h-full text-[color:var(--color-text)]/60 text-sm">
                                    {msg}
                                </div>
                            }
                            .into_any();
                        }
                        return view! {
                            <svg
                                class="block w-full h-auto"
                                viewBox=format!("0 0 {:.0} {:.0}", dm.scene.width, dm.scene.height)
                                preserveAspectRatio="xMidYMid meet"
                            >
                                {scene_view(&dm.scene)}
                                {move || {
                                    hover_index
                                        .get()
                                        .and_then(|i| {
                                            density_model
                                                .with(|m| {
                                                    let m = m.as_ref()?;
                                                    let b = m.hover.buckets.get(i)?;
                                                    Some(view! {
                                                        <line
                                                            x1=px(b.x)
                                                            y1=px(m.hover.plot_top)
                                                            x2=px(b.x)
                                                            y2=px(m.hover.plot_bottom)
                                                            stroke="#9ca3af"
                                                            stroke-opacity="0.45"
                                                            stroke-width="1"
                                                        />
                                                    })
                                                })
                                        })
                                }}
                            </svg>
                        }
                        .into_any();
                    }
                    let m = model.get();
                    // …existing empty-check + svg render, unchanged…
                }}
```

and make `on_pointer_move` resolve against whichever model is live:

```rust
    let on_pointer_move = move |evt: web_sys::PointerEvent| {
        use web_sys::wasm_bindgen::JsCast;
        let Some(target) = evt
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        let rect = target.get_bounding_client_rect();
        if rect.width() <= 0.0 {
            return;
        }
        let x_css = evt.client_x() - rect.left();
        let index = if mode.get_untracked() == ChartMode::Density {
            density_model.with_untracked(|m| {
                m.as_ref().and_then(|m| {
                    m.hover
                        .nearest_index((x_css / rect.width()) as f32 * m.scene.width)
                })
            })
        } else {
            model.with_untracked(|m| {
                m.hover
                    .nearest_index((x_css / rect.width()) as f32 * m.scene.width)
            })
        };
        hover_index.set(index);
    };
```

Clear hover on density-model rebuilds too (extend the existing effect: `density_model.track();`). The HTML `HoverTooltip` only renders values for series-bearing buckets; density buckets have empty `series_values`, so it shows the timestamp header (and quantity when the toggle is on — which density disables), which is the intended minimal tooltip.

- [ ] **Step 3: The mode-cap hint**

Under the `<ChartToolbar …/>`:

```rust
            {move || {
                let cap = mode.get().series_cap()?;
                model.with(|m| {
                    let visible: Vec<&ultros_charts::charts::price_history::SeriesInfo> =
                        m.series.iter().filter(|s| !s.hidden).collect();
                    (visible.len() > cap).then(|| {
                        let text = if cap == 1 {
                            let name = visible
                                .first()
                                .map(|s| s.name.clone())
                                .unwrap_or_default();
                            t_string!(i18n, chart_hint_single_series)
                                .to_string()
                                .replace("{name}", &name)
                        } else {
                            t_string!(i18n, chart_hint_range_limit).to_string()
                        };
                        view! {
                            <div class="text-xs text-amber-200/85">{text}</div>
                        }
                    })
                })
            }}
```

(If a bare `?` inside the closure fights the return type, restructure as `mode.get().series_cap().and_then(|cap| …)`.)

- [ ] **Step 4: Full frontend compile + tests**

Run: `cargo test -p ultros-app price_history && cargo check -p ultros-app`
Expected: PASS / clean. The pre-existing unit tests in `price_history_chart.rs` (`normalize_time_range`, etc.) must be untouched.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-app/src/components/price_history_chart.rs ultros-frontend/ultros-app/src/routes/item_view.rs ultros-frontend/ultros-app/src/api.rs
git commit -m "feat(app): chart mode toolbar, caption line, density view, mode-cap hints"
```

---

### Task 14: Verification

- [ ] **Step 1: Unit tests across touched crates**

Run: `cargo test -p ultros-charts -p ultros-api-types && cargo test -p ultros price_series && cargo test -p ultros density`
Expected: all PASS.

- [ ] **Step 2: CI check** (exact form — never a piped `$?`)

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`. Formatting failures: `cargo fmt --all`. Clippy exit 137 = OOM, re-run with `-j 2`, not a lint failure.

- [ ] **Step 3: Visual smoke in the browser preview**

Start the dev server (`.claude/launch.json` / `preview_start`), open an item page with datacenter scope (e.g. `/item/Aether/34680`), and verify: mode buttons switch renders; candles show wick-only ticks on sparse items; range shows ribbons; density fetches and shades; the group-by chip is absent on a world page; caption reflects mode/group/stats; quantity toggle disables in density with the reason on hover.

- [ ] **Step 4: Optional e2e**

Run: `./scripts/run_e2e.sh` if an app instance is practical in this environment; otherwise note in the PR that the Puppeteer smoke wasn't run.

- [ ] **Step 5: Push and PR against `main`** (repo convention: PRs target `main`; no `master`).

---

## Self-review notes (spec 2 coverage)

- Four modes ✅ (Tasks 1, 3, 4, 8) · sparse-candle rule + 1.2px floor ✅ (Task 3) · Range ribbons, ≤2 series ✅ (Task 4) · density endpoint with own aggregate, param `price_bins` default 32, bucket ladder + caching ✅ (Tasks 5–7) · batching budget (≤3 candle nodes, ≤8 density nodes) ✅ (tested) · Theme ramp + colorblind-safe candle pair with greyscale test ✅ (Task 2) · toolbar with reserved spec-3 slots, group-by auto-collapse preserved ✅ (Task 11) · caption line replaces StatsStrip ✅ (Task 12) · disabled-with-reason (quantity in density) ✅ · hints for forced-single-series ✅ (Task 13) · aria-labels on all icon-only buttons, no tooltip keys beyond the disabled-reason ✅ · i18n ×7 with real translations ✅ (Task 10) · mode switch preserves window/grouping ✅ (Task 13 wiring, by construction).
- Deviation from spec text: ribbons use a closed-polygon `Node::Path` (`band_path_d`) rather than `Node::Area` — `Area` can only fill to a flat baseline, so it cannot express a band between two curves; a batched Path is the same rendering cost and keeps the node budget.
- Deviation: the spec's "three area layers" test becomes "two band paths + median last" to match the band primitive above.
- Deferred to spec 3 (per the spec's own non-goals): grid view, world filter, shared crosshair across cells, % change toggle. Slots are reserved in the toolbar markup.
