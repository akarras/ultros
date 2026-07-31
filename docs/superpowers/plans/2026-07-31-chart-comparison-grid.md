# Chart Comparison Grid Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement spec 3 of the chart revamp (`docs/superpowers/specs/2026-07-26-chart-comparison-grid-design.md`): a small-multiples grid view with one shared crosshair, a searchable world filter driving the existing `hidden_series`, and an indexed-%-change toggle for overlay view.

**Architecture:** A `UnionIndex` (all distinct bucket timestamps across visible series, each series mapped by position) becomes shared state above the cells; one `hover_index` signal drives every cell's crosshair and a single container-level tooltip. A new `charts/grid.rs` layout builds one compact scene per series (Price/Candles/Range marks, no axes, no volume lane) against a shared y-domain, capped at 24 cells. The world filter and the legend write the same `hidden_series` signal. `% change` is a `PriceChartOptions` flag that rebases each series to its first visible bucket.

**Tech Stack:** Same as spec 2 — `ultros-charts` (pure scene graph), `ultros-app` (Leptos + leptos-i18n + icondata). No server changes: grid cells re-divide the already-fetched `PriceSeries`.

**Base branch:** stacked on `claude/item-view-chart-improvements-ecd66e` (spec 2, PR #1042). PR target = that branch until #1042 merges.

**Conventions:** TDD per task; `cargo fmt --all` before every commit; new user-facing strings land in all seven locale files; `./check_ci.sh` (or fmt + `clippy --all-targets -j 8 -- -D warnings`) before push.

**Decisions pinned here (spec deviations / open questions resolved):**

- **Grid is NOT offered in Density mode.** The spec says grid is available in every mode on the premise that "spec 1's per-series payload already contains everything" — that premise is false for Density, whose payload (`PriceDensity`) is scope-wide, not per-series. A per-world density grid would need N fetches, which the spec's own "No new fetching" non-goal forbids. The view toggle's Grid button disables with a reason in Density mode. Flagged for review in the PR.
- **`% change` applies to overlay Price mode only** (disabled with a reason otherwise). The spec specifies it as overlay-only and motivates it by multi-series offset — which is Price mode; rebasing OHLC candles is deferred until someone asks.
- **Sort-by-change** computes over the buckets in the fetched window (first→last vwap per series), recomputed only when the model rebuilds — the refetch debounce already prevents the "sort order shifts while dragging" concern.
- Cell cap 24 with a "+N more" affordance that opens the world filter, per spec.

---

### Task 1: `UnionIndex` in `ultros-charts`

**Files:**
- Create: `ultros-frontend/ultros-charts/src/data/union_index.rs`
- Modify: `ultros-frontend/ultros-charts/src/data/mod.rs` (add `pub mod union_index;` — check the file name: `data/` has `buckets.rs`, `grouping.rs`, `stats.rs`, `trend.rs`; find its mod file with `ls ultros-frontend/ultros-charts/src/data`)

- [ ] **Step 1: Write the file with failing tests**

```rust
//! Shared time index for the comparison grid: the sorted set of all bucket
//! timestamps across the visible series, with each series mapped onto it by
//! position (`None` where a series has no bucket at that timestamp).
//!
//! Because every series in a `PriceSeries` response came from one query with
//! one bucket width, the union index is exact — no interpolation or
//! snapping. This generalises what `HoverModel::buckets` already does; here
//! the index is shared state owned above the grid cells, which is what makes
//! one crosshair line up across every cell.

use chrono::NaiveDateTime;
use ultros_api_types::price_series::PriceBucket;

#[derive(Clone, Debug, PartialEq)]
pub struct UnionIndex {
    /// Sorted, distinct bucket timestamps across all indexed series.
    pub timestamps: Vec<NaiveDateTime>,
    /// `positions[series][union_pos]` = index into that series' bucket vec,
    /// `None` where the series has no bucket at that timestamp.
    pub positions: Vec<Vec<Option<usize>>>,
}

impl UnionIndex {
    pub fn is_empty(&self) -> bool {
        self.timestamps.is_empty()
    }

    /// Bucket of series `s` at union position `i`, if any.
    pub fn bucket<'a>(
        &self,
        series_buckets: &'a [PriceBucket],
        s: usize,
        i: usize,
    ) -> Option<&'a PriceBucket> {
        let idx = (*self.positions.get(s)?.get(i)?)?;
        series_buckets.get(idx)
    }
}

/// Build the union index over the given series' bucket slices (callers pass
/// only VISIBLE series — hidden ones must not widen the index).
pub fn build_union_index(series: &[&[PriceBucket]]) -> UnionIndex {
    let mut timestamps: Vec<NaiveDateTime> =
        series.iter().flat_map(|b| b.iter().map(|x| x.ts)).collect();
    timestamps.sort_unstable();
    timestamps.dedup();

    let positions = series
        .iter()
        .map(|buckets| {
            // Both sides sorted ascending: single merge pass per series.
            let mut out = vec![None; timestamps.len()];
            let mut bi = 0usize;
            for (ui, ts) in timestamps.iter().enumerate() {
                if bi < buckets.len() && buckets[bi].ts == *ts {
                    out[ui] = Some(bi);
                    bi += 1;
                }
            }
            out
        })
        .collect();

    UnionIndex {
        timestamps,
        positions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::bucket;

    fn buckets_at(secs: &[i64]) -> Vec<PriceBucket> {
        secs.iter()
            .map(|s| bucket(*s, 100, 120, 90, 105, 2))
            .collect()
    }

    #[test]
    fn union_holds_every_distinct_timestamp_once_sorted() {
        let a = buckets_at(&[100, 300, 500]);
        let b = buckets_at(&[200, 300, 700]);
        let u = build_union_index(&[&a, &b]);
        let secs: Vec<i64> = u
            .timestamps
            .iter()
            .map(|t| t.and_utc().timestamp())
            .collect();
        assert_eq!(secs, vec![100, 200, 300, 500, 700]);
        // a maps to positions 0, 2, 3 with gaps at 1, 4
        assert_eq!(u.positions[0], vec![Some(0), None, Some(1), Some(2), None]);
        // b maps to positions 1, 2, 4
        assert_eq!(u.positions[1], vec![None, Some(0), Some(1), None, Some(2)]);
    }

    #[test]
    fn strict_subset_series_maps_without_shifting() {
        let full = buckets_at(&[100, 200, 300, 400]);
        let sub = buckets_at(&[200, 400]);
        let u = build_union_index(&[&full, &sub]);
        assert_eq!(u.timestamps.len(), 4);
        assert_eq!(u.positions[1], vec![None, Some(0), None, Some(1)]);
        // Round-trip through the accessor
        assert_eq!(
            u.bucket(&sub, 1, 1).map(|b| b.ts),
            Some(sub[0].ts),
            "accessor resolves union position to the right bucket"
        );
        assert!(u.bucket(&sub, 1, 0).is_none());
    }

    #[test]
    fn empty_input_yields_empty_index() {
        let u = build_union_index(&[]);
        assert!(u.is_empty());
        assert!(u.positions.is_empty());
    }
}
```

- [ ] **Step 2: Run to verify failure, register the module, run to pass**

Run: `cargo test -p ultros-charts union_index` — FAIL (module unknown) → add `pub mod union_index;` to `data`'s mod file → PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-charts/src/data
git commit -m "feat(charts): UnionIndex shared time index for the grid"
```

---

### Task 2: Grid layout in `ultros-charts`

**Files:**
- Create: `ultros-frontend/ultros-charts/src/charts/grid.rs`
- Modify: `ultros-frontend/ultros-charts/src/charts/mod.rs` (`pub mod grid;`)

The layout builds one compact scene per visible series — plot marks only (no axes, no grid lines, no volume lane, no title; the HTML cell renders the label). All cells share: logical size, x positions (`xs[union_pos]`), and (by default) the y-domain.

- [ ] **Step 1: Write failing tests** (in-file `#[cfg(test)]`, using `test_util::{two_world_series, world_helper}`)

```rust
    #[test]
    fn one_cell_per_visible_series_sorted_by_name() {
        let model = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions::default(),
        );
        let names: Vec<&str> = model.cells.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Adamantoise", "Gilgamesh"]);
        assert_eq!(model.overflow, 0);
    }

    #[test]
    fn hidden_series_are_excluded_from_cells_index_and_domain() {
        let model = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions {
                hidden_series: vec!["Gilgamesh".to_string()],
                ..Default::default()
            },
        );
        assert_eq!(model.cells.len(), 1);
        // Fixture: Adamantoise base 1200 (low 1190..high 1310), Gilgamesh
        // base 1000 — domain must reflect Adamantoise only.
        assert!(model.y_domain.0 >= 1150.0, "hidden series widened the domain: {:?}", model.y_domain);
    }

    #[test]
    fn cell_cap_collapses_the_remainder_into_overflow() {
        let mut series = two_world_series();
        // 30 copies of the first entry under synthetic ids — resolvable ids
        // don't matter for the cap; unresolvable ones are dropped, so reuse
        // the two real ids alternately… simpler: cap test via options.
        let model = build_price_grid(
            &world_helper(),
            &series,
            &GridOptions { cell_cap: 1, ..Default::default() },
        );
        assert_eq!(model.cells.len(), 1);
        assert_eq!(model.overflow, 1);
    }

    #[test]
    fn shared_y_domain_spans_all_cells_and_per_cell_scaling_does_not() {
        let shared = build_price_grid(&world_helper(), &two_world_series(), &GridOptions::default());
        // Fixture spans ~990..=1400 across both worlds.
        assert!(shared.y_domain.0 < 1000.0 && shared.y_domain.1 > 1300.0);
        let per_cell = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions { shared_y: false, ..Default::default() },
        );
        // Per-cell mode still reports the union domain in y_domain, but each
        // cell's marks are scaled to its own extent — assert the two modes
        // produce different scenes for the lower-priced cell.
        assert_ne!(shared.cells[1].scene, per_cell.cells[1].scene);
    }

    #[test]
    fn sort_by_change_orders_by_relative_window_change() {
        // Gilgamesh: 1005 -> 1095 (+9%); Adamantoise: 1205 -> 1295 (+7.5%).
        let model = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions { sort: GridSort::Change, ..Default::default() },
        );
        let names: Vec<&str> = model.cells.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Gilgamesh", "Adamantoise"], "biggest change first");
    }

    #[test]
    fn cells_draw_marks_but_no_axis_text_or_volume() {
        let model = build_price_grid(&world_helper(), &two_world_series(), &GridOptions::default());
        for cell in &model.cells {
            assert!(cell.scene.nodes.iter().any(|n| matches!(n, Node::Polyline { .. })));
            assert!(!cell.scene.nodes.iter().any(|n| matches!(n, Node::Text { .. })));
            assert!(!cell.scene.nodes.iter().any(|n| matches!(n, Node::Rect { .. })), "no volume bars in cells");
        }
    }

    #[test]
    fn candle_cells_emit_batched_paths() {
        let model = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions { mode: ChartMode::Candles, ..Default::default() },
        );
        for cell in &model.cells {
            let paths = cell.scene.nodes.iter().filter(|n| matches!(n, Node::Path { .. })).count();
            assert!((1..=3).contains(&paths), "wick + up/down bodies, batched");
        }
    }

    #[test]
    fn xs_align_with_the_union_index_and_are_shared_by_all_cells() {
        let model = build_price_grid(&world_helper(), &two_world_series(), &GridOptions::default());
        assert_eq!(model.xs.len(), model.union.timestamps.len());
        assert!(model.xs.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn hiding_every_series_yields_empty_cells_but_metadata_survives_upstream() {
        let model = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions {
                hidden_series: vec!["Gilgamesh".into(), "Adamantoise".into()],
                ..Default::default()
            },
        );
        assert!(model.cells.is_empty());
        assert!(model.union.is_empty());
    }
```

(While writing, verify the exact fixture numbers in `test_util::two_world_series` — bases 1000/1200, +10/day over 10 buckets, open p−0? The `bucket(ts, p, p+20, p-10, p+5, 2)` shape gives vwap = gil/units = close = p+5. Adjust the literal expectations in `sort_by_change_orders…` and the domain tests to the fixture's real numbers before first run.)

- [ ] **Step 2: Implement**

```rust
//! Small-multiples grid layout: one compact scene per visible series, all
//! cells sharing x positions and (by default) a y-domain, so the container's
//! single crosshair lines up in every cell. Cells draw plot marks only —
//! the HTML layer renders labels; axes would be noise at cell size, and the
//! volume lane is deliberately omitted (spec 3).

use ultros_api_types::price_series::{PriceBucket, PriceSeries, SeriesGroup};
use ultros_api_types::world_helper::{AnySelector, WorldHelper};

use crate::charts::ChartMode;
use crate::data::union_index::{UnionIndex, build_union_index};
use crate::scale::{LinearScale, TimeScale};
use crate::scene::{Color, Node, Scene, Stroke};
use crate::svg::{band_path_d, rects_path_d, vlines_path_d};
use crate::theme::Theme;

pub const GRID_CELL_CAP: usize = 24;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GridSort {
    #[default]
    Name,
    /// Largest relative change over the fetched window first.
    Change,
}

#[derive(Clone, Debug)]
pub struct GridOptions {
    /// Logical cell size; the HTML layer scales via viewBox.
    pub cell_width: f32,
    pub cell_height: f32,
    /// Price, Candles or Range. Density is scope-wide, not per-series, so
    /// the app never requests a density grid (view toggle disables it).
    pub mode: ChartMode,
    /// Shared y-domain across cells (default). `false` = per-cell scaling,
    /// the escape hatch for one outlier world flattening everything else.
    pub shared_y: bool,
    pub sort: GridSort,
    pub cell_cap: usize,
    pub hidden_series: Vec<String>,
    pub theme: Theme,
}

impl Default for GridOptions {
    fn default() -> Self {
        Self {
            cell_width: 280.0,
            cell_height: 150.0,
            mode: ChartMode::Price,
            shared_y: true,
            sort: GridSort::Name,
            cell_cap: GRID_CELL_CAP,
            hidden_series: Vec::new(),
            theme: Theme::site(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GridCell {
    pub name: String,
    pub color: Color,
    pub scene: Scene,
    /// VWAP per union position for the container tooltip (`None` = gap).
    pub values: Vec<Option<f64>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GridModel {
    pub cells: Vec<GridCell>,
    /// Visible series beyond the cap, collapsed into "+N more".
    pub overflow: usize,
    pub union: UnionIndex,
    /// Shared price domain of every drawn cell (informational when
    /// `shared_y` is off).
    pub y_domain: (f64, f64),
    /// Pixel x per union position — identical for every cell.
    pub xs: Vec<f32>,
    pub cell_width: f32,
    pub cell_height: f32,
    pub plot_top: f32,
    pub plot_bottom: f32,
}
```

`build_price_grid(world_helper, series, options) -> GridModel` steps:

1. Resolve + name-sort series exactly like `price_history.rs` (same `AnySelector` match, drop unresolvable ids), assign palette colors by sorted index **before** filtering/sorting-by-change so colors agree with the overlay legend.
2. Drop user-hidden series; sort by `options.sort` (`Change`: `(last.vwap / first.vwap - 1)` descending over each series' buckets, `None`-vwap buckets skipped); truncate to `cell_cap`, remember `overflow`.
3. `build_union_index` over the remaining cells' bucket slices.
4. Shared domain from all drawn cells' `low..high` (Candles/Range) or vwap extent (Price), 5% pad like the overlay. Per-cell mode recomputes a domain per cell but still reports the union domain on the model.
5. Time scale over `union.timestamps.first()..last() + bucket`, range `(0, cell_width)`; `xs[i] = time.scale(ts_i + bucket/2)`; `plot_top = 4.0`, `plot_bottom = cell_height - 4.0`.
6. Per cell, one scene of size `cell_width × cell_height` (transparent background) with the mode's marks — direct ports of spec 2's drawing arms operating on one series:
   - Price: vwap polyline (width 1.5) + area fill at 0.10 alpha to `plot_bottom`.
   - Candles: wick vlines path + up/down `rects_path_d`, `MIN_CANDLE_SALES` rule and 1.2px floor reused (import the constant).
   - Range: low–high band (0.08), p25–p75 band (0.20), p50 polyline (1.5), via `band_path_d`.
7. `values[i]` = vwap at union position `i` via `UnionIndex::bucket`.

- [ ] **Step 3: Run `cargo test -p ultros-charts` to pass, then commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-charts/src/charts
git commit -m "feat(charts): small-multiples grid layout with shared domain and union xs"
```

---

### Task 3: Indexed % change in the overlay layout

**Files:**
- Modify: `ultros-frontend/ultros-charts/src/charts/price_history.rs`
- Modify: `ultros-frontend/ultros-charts/src/scale.rs` (percent label helper)

- [ ] **Step 1: Failing tests**

```rust
    #[test]
    fn percent_mode_rebases_each_series_to_zero_at_its_first_bucket() {
        let model = build_price_history_chart(
            &world_helper(),
            &two_world_series(),
            &PriceChartOptions {
                index_to_percent: true,
                show_market_average: true, // must be ignored in % mode
                ..Default::default()
            },
        );
        // Both series start at 0%: first hover bucket carries ~0 for each.
        let first = model.hover.buckets.first().unwrap();
        for v in first.series_values.iter().flatten() {
            assert!(v.1.abs() < 1e-9, "first bucket rebases to 0%, got {}", v.1);
        }
        // Market-average line must not draw in % mode (meaningless).
        let dashed_lines = model
            .scene
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Line { stroke, .. } if stroke.dash.is_some()))
            .count();
        assert_eq!(dashed_lines, 0);
    }

    #[test]
    fn percent_labels_format_with_sign_and_symbol() {
        assert_eq!(format_percent(0.0), "0%");
        assert_eq!(format_percent(12.34), "+12.3%");
        assert_eq!(format_percent(-5.0), "-5%");
    }
```

- [ ] **Step 2: Implement**

- `scale.rs`: `pub fn format_percent(v: f64) -> String` — trims trailing `.0`, prefixes `+` for positives, plain `0%` for zero.
- `PriceChartOptions` gains `pub index_to_percent: bool` (default `false`).
- In the layout, when the flag is set (only honoured in `ChartMode::Price`): per series, `base` = first bucket with a vwap; every plotted value becomes `(vwap / base - 1.0) * 100.0`. The price domain, y-axis tick labels (`format_percent` instead of `short_number`), hover values, area fill baseline (0%), and stats-independent parts all use the transformed values. `show_market_average` and `show_trendline` are ignored while on (the caption/control layer explains why). Raw dots are skipped in % mode (their gil values are not comparable to a % axis).

- [ ] **Step 3: Test to pass, commit**

```bash
cargo fmt --all
git add ultros-frontend/ultros-charts/src
git commit -m "feat(charts): indexed percent-change mode for the overlay price lane"
```

---

### Task 4: i18n keys — all seven locales

**Files:** `ultros-frontend/ultros-app/locales/{en,fr,de,ja,cn,ko,tc}.json`

- [ ] **Step 1: Add the keys** (same insertion point, after `chart_density_quantity_unavailable`):

| key | en | fr | de | ja | cn | ko | tc |
|---|---|---|---|---|---|---|---|
| `chart_view_overlay` | Overlay | Superposition | Überlagerung | 重ね表示 | 叠加视图 | 오버레이 | 疊加檢視 |
| `chart_view_grid` | Grid | Grille | Raster | グリッド | 网格视图 | 그리드 | 網格檢視 |
| `chart_view_group` | Chart view | Vue du graphique | Diagrammansicht | チャート表示 | 图表视图 | 차트 보기 | 圖表檢視 |
| `chart_grid_density_unavailable` | Grid view isn't available in density mode. | La vue grille n'est pas disponible en mode densité. | Die Rasteransicht ist im Dichtemodus nicht verfügbar. | 密度モードではグリッド表示を利用できません。 | 密度模式下无法使用网格视图。 | 밀도 모드에서는 그리드 보기를 사용할 수 없습니다. | 密度模式下無法使用網格檢視。 |
| `chart_world_filter` | Worlds | Mondes | Welten | ワールド | 服务器 | 월드 | 伺服器 |
| `chart_filter_search` | Search worlds… | Rechercher des mondes… | Welten suchen… | ワールドを検索… | 搜索服务器… | 월드 검색… | 搜尋伺服器… |
| `chart_filter_all` | All | Tout | Alle | すべて | 全选 | 전체 | 全選 |
| `chart_filter_none` | None | Aucun | Keine | なし | 全不选 | 없음 | 全不選 |
| `chart_percent_change` | Index to % change | Indexer en % de variation | Auf %-Änderung indexieren | 変化率(%)で表示 | 按涨跌幅(%)显示 | 변동률(%)로 표시 | 按漲跌幅(%)顯示 |
| `chart_percent_disables_overlays` | Market average and trendline aren't available on a % axis. | La moyenne du marché et la tendance ne sont pas disponibles sur un axe en %. | Marktdurchschnitt und Trendlinie sind auf einer %-Achse nicht verfügbar. | %軸では市場平均とトレンドラインを利用できません。 | %坐标轴下无法使用市场均价和趋势线。 | % 축에서는 시장 평균과 추세선을 사용할 수 없습니다. | %座標軸下無法使用市場均價和趨勢線。 |
| `chart_percent_overlay_only` | % change applies to the overlay price view. | La variation en % s'applique à la vue superposée des prix. | Die %-Änderung gilt für die überlagerte Preisansicht. | 変化率(%)は重ね表示の価格ビューでのみ有効です。 | 涨跌幅(%)仅适用于叠加价格视图。 | 변동률(%)은 오버레이 가격 보기에만 적용됩니다. | 漲跌幅(%)僅適用於疊加價格檢視。 |
| `chart_grid_more` | +{n} more | +{n} de plus | +{n} weitere | ほか{n}件 | 另 {n} 个 | 외 {n}개 | 另 {n} 個 |
| `chart_sort_name` | Sort by name | Trier par nom | Nach Name sortieren | 名前順 | 按名称排序 | 이름순 정렬 | 按名稱排序 |
| `chart_sort_change` | Sort by change | Trier par variation | Nach Änderung sortieren | 変化率順 | 按涨跌排序 | 변동순 정렬 | 按漲跌排序 |
| `chart_scale_per_cell` | Scale each cell | Échelle par cellule | Jede Zelle skalieren | セルごとにスケール | 每格独立缩放 | 셀별 스케일 | 每格獨立縮放 |
| `chart_hint_use_grid` | Switch to grid | Passer à la grille | Zum Raster wechseln | グリッドに切り替え | 切换到网格 | 그리드로 전환 | 切換到網格 |

Use the same python insertion script pattern as spec 2's Task 10 (anchor: `chart_density_quantity_unavailable`), then `git diff --stat` to confirm ~16 lines per file.

- [ ] **Step 2: `cargo check -p ultros-app`** (keys validate at compile time) — run once Task 5 lands if avoiding a double compile; commit locales with Task 5.

---

### Task 5: Toolbar — view toggle + world filter chip

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/chart_toolbar.rs`

- [ ] **Step 1: Implement**

Add to the component's props:

```rust
    #[prop(into)] view: Signal<ChartView>,
    set_view: WriteSignal<ChartView>,
    /// Grid is disabled (with reason) in density mode.
    #[prop(into)] grid_disabled: Signal<bool>,
    /// All worlds of the current scope grouped by datacenter, for the filter
    /// popover: (datacenter name, world names).
    #[prop(into)] filter_groups: Signal<Vec<(String, Vec<String>)>>,
    /// The same signal the legend writes — the filter is legend-at-scale.
    hidden_series: RwSignal<Vec<String>>,
    #[prop(into)] percent_change: Signal<bool>,
    set_percent_change: WriteSignal<bool>,
    /// % change is overlay+Price only.
    #[prop(into)] percent_disabled: Signal<bool>,
```

with, at the top of the file:

```rust
/// Overlay = today's single chart; Grid = one small chart per series with a
/// shared crosshair (spec 3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartView {
    #[default]
    Overlay,
    Grid,
}
```

**View toggle** fills the reserved slot after the mode group: a two-item icon segmented group identical in style to the mode group — `LuChartNoAxesCombined` (Overlay), `LuLayoutGrid` (Grid), aria-labels `chart_view_overlay`/`chart_view_grid`, group aria-label `chart_view_group`. The Grid button gets `prop:disabled=grid_disabled` and `title` = `chart_grid_density_unavailable` when disabled.

**World filter chip** fills the second reserved slot, wrapped in `<Show when=…>` only when the scope has more than one world (`filter_groups` flattened len > 1): `TbFilterOutline` icon + `chart_world_filter` label + count badge `{visible}/{total}` (visible = total − hidden that exist in groups). Popover (same pattern as the group menu):

- search `<input>` bound to a local `filter_query` signal, placeholder `chart_filter_search`, filtering world names case-insensitively;
- per datacenter group: header row with the DC name plus two small buttons `chart_filter_all` / `chart_filter_none` operating on that group's worlds (remove-from / add-to `hidden_series`);
- one checkbox row per world, checked = not hidden, writing `hidden_series` exactly like a legend click (push + sort / remove — copy the closure from the legend so state stays byte-identical).

**Overlays popover** gains a fourth `OverlayRow` for `chart_percent_change` (`disabled=percent_disabled`, reason `chart_percent_overlay_only`), and the market-average row's `disabled`/reason become reactive: disabled when `percent_change` is on, reason `chart_percent_disables_overlays` (the trend row too).

- [ ] **Step 2: `cargo check -p ultros-app`** (with Task 4's locales staged) — expect call-site errors only in `price_history_chart.rs` (fixed next task). Commit Tasks 4+5 together once Task 6 compiles, or keep staged.

---

### Task 6: Grid view + shared crosshair in `PriceHistoryChart`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/price_history_chart.rs`
- Modify: `ultros-frontend/ultros-app/src/routes/item_view.rs` (no new props — view/filter/percent state lives inside the chart component; nothing about them gates a fetch)

- [ ] **Step 1: State + models**

Inside `PriceHistoryChart` (imports: `ChartView`, `GridOptions`, `GridSort`, `GridModel`, `build_price_grid`, `GRID_CELL_CAP`, `format_percent` not needed here):

```rust
    let (view, set_view) = signal(ChartView::Overlay);
    let (percent_change, set_percent_change) = signal(false);
    let (grid_per_cell_scale, set_grid_per_cell_scale) = signal(false);
    let (grid_sort, set_grid_sort) = signal(GridSort::Name);
```

- `model` memo: `index_to_percent: percent_change.get() && mode.get() == ChartMode::Price && view.get() == ChartView::Overlay`.
- New `grid_model: Memo<GridModel>` calling `build_price_grid` with `mode` (Price/Candles/Range — when mode is Density the grid button is disabled so the memo can just pass Price), `shared_y: !grid_per_cell_scale.get()`, `sort: grid_sort.get()`, `hidden_series`, `Theme::site()`, cell 280×150.
- `filter_groups` memo: from `LocalWorldData`'s helper + `scope_name` — look up the scope, collect its datacenters and world names (world scope → single group with one world; the chip auto-hides). Reuse `AnySelector`/`AnyResult` walking as in `available_group_levels`' source for the shapes.
- Toolbar call site gains the new props; `grid_disabled = Signal::derive(move || mode.get() == ChartMode::Density)`; `percent_disabled = Signal::derive(move || !(mode.get() == ChartMode::Price && view.get() == ChartView::Overlay))`.
- The spec-2 mode-cap hint: when the cap hint shows and view is Overlay, append a small action button `chart_hint_use_grid` calling `set_view.set(ChartView::Grid)` (grid rescues single-series modes — the spec's requested affordance).

- [ ] **Step 2: Grid rendering + shared crosshair**

In the chart area closure, before the density branch: when `view.get() == ChartView::Grid && mode.get() != ChartMode::Density`, render the grid instead:

```rust
    let gm = grid_model.get();
    if gm.cells.is_empty() {
        return empty_state(); // hiding everything → empty card; legend below still offers un-hiding
    }
    let hover_x = hover_index.get().and_then(|i| gm.xs.get(i).copied());
    return view! {
        <div
            class="grid gap-2"
            style="grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));"
            on:pointermove=on_grid_pointer_move
            on:pointerleave=move |_| hover_index.set(None)
        >
            {gm.cells.iter().map(|cell| { /* per cell: */
                // header row: color dot + name (HTML, not scene text)
                // <svg viewBox="0 0 280 150"> scene_view(&cell.scene)
                //   + crosshair <line> at hover_x when Some
            }).collect_view()}
            {(gm.overflow > 0).then(|| /* "+N more" button opening the world filter:
                a chip that calls a shared `open_world_filter` RwSignal<bool> the toolbar popover also reads */)}
        </div>
        // single container-level tooltip listing every cell's value at
        // hover_index via gm.cells[..].values[i], reusing HoverTooltip's
        // styling in a small inline block
    }.into_any();
```

`on_grid_pointer_move`: resolve the pointer's cell-local x — `target.closest("svg")`'s bounding rect → `x_css / rect.width() * gm.cell_width` → nearest index over `gm.xs` (binary search like `HoverModel::nearest_index`; add `pub fn nearest_x(xs: &[f32], x: f32) -> Option<usize>` to `grid.rs` with a unit test rather than reimplementing inline). Because every cell shares `xs`, resolving in any cell drives all cells — the property that makes the view work.

Practical Leptos note: iterate `gm.cells` by value (`into_iter`) or clone per cell — scenes are plain data. Grid header row (sort select + per-cell-scale checkbox) sits above the grid: a `<select>` with the two `GridSort` options (`chart_sort_name` / `chart_sort_change`) and a labelled checkbox `chart_scale_per_cell`.

"+N more" and the toolbar's filter chip share one `world_filter_open: RwSignal<bool>` — lift the popover-open signal out of `ChartToolbar` into a prop so both can open it.

- [ ] **Step 3: Caption + guarantees**

- Caption line: append view (`chart_view_grid` when grid) and `% change` marker (`chart_percent_change`) when active.
- The pre-existing guarantee test behaviourally: hiding every series in grid view shows the empty card while the legend (rendered from the overlay `model`, unchanged) still offers un-hiding — the legend block stays outside the view branch, verify it does.
- `HoverTooltip`/`HoverLayer` stay overlay-only (inside the overlay branch).

- [ ] **Step 4: Compile, run app unit tests, commit**

```bash
cargo check -p ultros-app && cargo test -p ultros-app price_history
cargo fmt --all
git add ultros-frontend/ultros-app
git commit -m "feat(app): grid view with shared crosshair, world filter, percent change"
```

---

### Task 7: Verification

- [ ] `cargo test -p ultros-charts -p ultros-api-types` — all green (union index, grid layout, % change, plus every spec-2 regression test untouched).
- [ ] `cargo fmt --all -- --check` then `cargo clippy --all-targets -j 8 -- -D warnings` → exit 0.
- [ ] Browser pass when an environment with ClickHouse is available: overlay↔grid preserves mode/grouping/filter/window; crosshair lines up across cells; filter chip badge; legend↔filter stay in sync; `% change` present in overlay Price, disabled elsewhere. (Same limitation as PR #1042 — note in PR if not run.)
- [ ] Push and open PR with base `claude/item-view-chart-improvements-ecd66e` (stacked on #1042); retarget to `main` after #1042 merges.

## Self-review notes (spec 3 coverage)

- Union index exact-timestamps model + tests ✅ (Task 1, spec's three index tests) · shared crosshair driven by one `hover_index` resolved once at the container ✅ (Task 6) · single container-level tooltip ✅ · grid layout: width-responsive columns, name-sort default + sort-by-change, shared y-domain default + per-cell escape hatch, cap 24 + "+N more" opening the filter, per-cell labels, volume lane omitted ✅ (Tasks 2, 6) · world filter drives `hidden_series` (legend parity), searchable, DC-grouped select all/none, count badge ✅ (Task 5) · `% change` overlay-only, market-average disabled with reason, caption reflects it ✅ (Tasks 3, 5, 6) · view toggle icons per spec, grid rescue hint as the cap-hint's action ✅ · no new fetching, no cross-scope compare tray ✅ (nothing added).
- **Deviations:** grid unavailable in Density mode (spec premise about the payload is wrong for density — documented at the top); `% change` scoped to Price mode.
- Spec risks tracked: node budget honoured by reusing spec-2 batching in cells; the union-index width risk (80-wide `Vec<Option<..>>`) bounded by the 24-cell cap before the index is built.
