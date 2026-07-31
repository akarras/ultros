//! Small-multiples grid layout: one compact scene per visible series, all
//! cells sharing x positions and (by default) a y-domain, so the container's
//! single crosshair lines up in every cell. Cells draw plot marks only —
//! the HTML layer renders labels; axes would be noise at cell size, and the
//! volume lane is deliberately omitted (spec 3).

use chrono::TimeDelta;
use ultros_api_types::price_series::{PriceBucket, PriceSeries, SeriesGroup};
use ultros_api_types::world_helper::{AnySelector, WorldHelper};

use crate::charts::ChartMode;
use crate::charts::price_history::MIN_CANDLE_SALES;
use crate::data::union_index::{UnionIndex, build_union_index};
use crate::scale::{LinearScale, TimeScale};
use crate::scene::{Color, Node, Scene, Stroke};
use crate::svg::{band_path_d, rects_path_d, vlines_path_d};
use crate::theme::Theme;

/// Beyond this many cells nothing is legible and the node count is
/// unbounded; the remainder collapses into a "+N more" affordance that
/// opens the world filter.
pub const GRID_CELL_CAP: usize = 24;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GridSort {
    /// Stable across refetches.
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
    /// the app never requests a density grid (its view toggle disables it).
    pub mode: ChartMode,
    /// Shared y-domain across cells (default) — cell height stays meaningful
    /// across the grid. `false` = per-cell scaling, the escape hatch for one
    /// outlier world flattening everything else.
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
    /// Same palette assignment as the overlay legend (by name-sorted index
    /// over ALL resolved series), so a series keeps its color across views.
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
    /// Price domain shared by every drawn cell (informational when
    /// `shared_y` is off).
    pub y_domain: (f64, f64),
    /// Pixel x per union position — identical for every cell, which is what
    /// makes the shared crosshair line up across the grid.
    pub xs: Vec<f32>,
    pub cell_width: f32,
    pub cell_height: f32,
    pub plot_top: f32,
    pub plot_bottom: f32,
}

/// Index of the closest x in a sorted slice — `HoverModel::nearest_index`
/// for a bare position list; the container resolves pointer x through this.
pub fn nearest_x(xs: &[f32], x: f32) -> Option<usize> {
    if xs.is_empty() {
        return None;
    }
    let i = xs.partition_point(|v| *v < x);
    if i == 0 {
        return Some(0);
    }
    if i >= xs.len() {
        return Some(xs.len() - 1);
    }
    if (x - xs[i - 1]) <= (xs[i] - x) {
        Some(i - 1)
    } else {
        Some(i)
    }
}

struct ResolvedCell {
    name: String,
    color: Color,
    buckets: Vec<PriceBucket>,
}

/// Relative change over the series' fetched window, for `GridSort::Change`.
/// `None` when the series has no two priceable buckets.
fn window_change(buckets: &[PriceBucket]) -> Option<f64> {
    let mut vwaps = buckets.iter().filter_map(|b| b.vwap());
    let first = vwaps.next()?;
    let last = vwaps.next_back().unwrap_or(first);
    (first > 0.0).then(|| last / first - 1.0)
}

pub fn build_price_grid(
    world_helper: &WorldHelper,
    series: &PriceSeries,
    options: &GridOptions,
) -> GridModel {
    let theme = &options.theme;

    // Resolve + name-sort exactly like the overlay layout, and assign
    // palette colors by that sorted index BEFORE hiding/sorting/capping so
    // colors agree with the overlay legend.
    let mut resolved: Vec<(String, Vec<PriceBucket>)> = series
        .series
        .iter()
        .filter_map(|entry| {
            let selector = match series.group {
                SeriesGroup::Region => AnySelector::Region(entry.id),
                SeriesGroup::Datacenter => AnySelector::Datacenter(entry.id),
                SeriesGroup::World => AnySelector::World(entry.id),
            };
            let name = world_helper
                .lookup_selector(selector)?
                .get_name()
                .to_string();
            Some((name, entry.buckets.clone()))
        })
        .collect();
    resolved.sort_by(|a, b| a.0.cmp(&b.0));

    let mut visible: Vec<ResolvedCell> = resolved
        .into_iter()
        .enumerate()
        .filter(|(_, (name, _))| !options.hidden_series.iter().any(|h| h == name))
        .map(|(index, (name, buckets))| ResolvedCell {
            name,
            color: theme.palette[index % theme.palette.len()],
            buckets,
        })
        .collect();

    if options.sort == GridSort::Change {
        visible.sort_by(|a, b| {
            let ca = window_change(&a.buckets).unwrap_or(f64::NEG_INFINITY);
            let cb = window_change(&b.buckets).unwrap_or(f64::NEG_INFINITY);
            cb.partial_cmp(&ca)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.cmp(&b.name))
        });
    }

    let overflow = visible.len().saturating_sub(options.cell_cap.max(1));
    visible.truncate(options.cell_cap.max(1));

    let bucket_slices: Vec<&[PriceBucket]> = visible.iter().map(|c| c.buckets.as_slice()).collect();
    let union = build_union_index(&bucket_slices);

    let plot_top = 4.0;
    let plot_bottom = options.cell_height - 4.0;

    if union.is_empty() {
        return GridModel {
            cells: Vec::new(),
            overflow,
            union,
            y_domain: (0.0, 0.0),
            xs: Vec::new(),
            cell_width: options.cell_width,
            cell_height: options.cell_height,
            plot_top,
            plot_bottom,
        };
    }

    let bucket_secs = series.bucket_seconds.max(1);
    let half_bucket = TimeDelta::seconds(bucket_secs / 2);
    let first_ts = *union.timestamps.first().expect("non-empty");
    let last_ts = *union.timestamps.last().expect("non-empty");
    let time = TimeScale::new(
        first_ts,
        last_ts + TimeDelta::seconds(bucket_secs),
        (0.0, options.cell_width),
    );
    let xs: Vec<f32> = union
        .timestamps
        .iter()
        .map(|ts| time.scale(*ts + half_bucket))
        .collect();

    // Domain over low..high covers every mode's marks (a vwap line always
    // sits inside its buckets' low/high band). 5% pad like the overlay.
    let padded_extent = |cells: &[&ResolvedCell]| -> (f64, f64) {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for cell in cells {
            for b in &cell.buckets {
                lo = lo.min(b.low as f64);
                hi = hi.max(b.high as f64);
            }
        }
        if !lo.is_finite() || !hi.is_finite() {
            return (0.0, 1.0);
        }
        let pad = ((hi - lo) * 0.05).max(1.0);
        ((lo - pad).max(0.0), hi + pad)
    };
    let all_refs: Vec<&ResolvedCell> = visible.iter().collect();
    let y_domain = padded_extent(&all_refs);

    let bucket_px = time.scale(first_ts + TimeDelta::seconds(bucket_secs)) - time.scale(first_ts);
    let body_w = (bucket_px * 0.6).clamp(1.0, 10.0);

    let cells: Vec<GridCell> = visible
        .iter()
        .enumerate()
        .map(|(s, cell)| {
            let domain = if options.shared_y {
                y_domain
            } else {
                padded_extent(&[cell])
            };
            let price = LinearScale::new(domain, (plot_bottom, plot_top));
            let mut scene = Scene {
                width: options.cell_width,
                height: options.cell_height,
                background: None,
                font_family: theme.font_family.clone(),
                nodes: Vec::new(),
            };
            let color = cell.color;
            let curve = |f: fn(&PriceBucket) -> i32| -> Vec<(f32, f32)> {
                cell.buckets
                    .iter()
                    .map(|b| (time.scale(b.ts + half_bucket), price.scale(f(b) as f64)))
                    .collect()
            };
            match options.mode {
                ChartMode::Price | ChartMode::Density => {
                    let line: Vec<(f32, f32)> = cell
                        .buckets
                        .iter()
                        .filter_map(|b| {
                            b.vwap()
                                .map(|v| (time.scale(b.ts + half_bucket), price.scale(v)))
                        })
                        .collect();
                    if line.len() > 1 {
                        scene.nodes.push(Node::Area {
                            points: line.clone(),
                            baseline_y: plot_bottom,
                            fill: color.with_alpha(0.10),
                        });
                        scene.nodes.push(Node::Polyline {
                            points: line,
                            stroke: Stroke {
                                color,
                                width: 1.5,
                                dash: None,
                            },
                        });
                    }
                }
                ChartMode::Candles => {
                    let mut up: Vec<(f32, f32, f32, f32)> = Vec::new();
                    let mut down: Vec<(f32, f32, f32, f32)> = Vec::new();
                    let mut wicks: Vec<(f32, f32, f32)> = Vec::new();
                    for b in &cell.buckets {
                        let x = time.scale(b.ts + half_bucket);
                        wicks.push((x, price.scale(b.high as f64), price.scale(b.low as f64)));
                        if b.sales < MIN_CANDLE_SALES {
                            continue;
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
                        scene.nodes.push(Node::Path {
                            d,
                            fill: Some(theme.candle_up),
                            stroke: None,
                        });
                    }
                    if let Some(d) = rects_path_d(&down) {
                        scene.nodes.push(Node::Path {
                            d,
                            fill: Some(theme.candle_down),
                            stroke: None,
                        });
                    }
                }
                ChartMode::Range => {
                    if let Some(d) = band_path_d(&curve(|b| b.high), &curve(|b| b.low)) {
                        scene.nodes.push(Node::Path {
                            d,
                            fill: Some(color.with_alpha(0.08)),
                            stroke: None,
                        });
                    }
                    if let Some(d) = band_path_d(&curve(|b| b.p75), &curve(|b| b.p25)) {
                        scene.nodes.push(Node::Path {
                            d,
                            fill: Some(color.with_alpha(0.20)),
                            stroke: None,
                        });
                    }
                    let p50 = curve(|b| b.p50);
                    if p50.len() > 1 {
                        scene.nodes.push(Node::Polyline {
                            points: p50,
                            stroke: Stroke {
                                color,
                                width: 1.5,
                                dash: None,
                            },
                        });
                    }
                }
            }
            let values: Vec<Option<f64>> = (0..union.timestamps.len())
                .map(|i| union.bucket(&cell.buckets, s, i).and_then(|b| b.vwap()))
                .collect();
            GridCell {
                name: cell.name.clone(),
                color,
                scene,
                values,
            }
        })
        .collect();

    GridModel {
        cells,
        overflow,
        union,
        y_domain,
        xs,
        cell_width: options.cell_width,
        cell_height: options.cell_height,
        plot_top,
        plot_bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{two_world_series, world_helper};

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
        assert_eq!(model.cells[0].name, "Adamantoise");
        // Fixture: Adamantoise low min = 1190; Gilgamesh low min = 990. The
        // hidden series must not widen the domain.
        assert!(
            model.y_domain.0 >= 1150.0,
            "hidden series widened the domain: {:?}",
            model.y_domain
        );
    }

    #[test]
    fn cell_cap_collapses_the_remainder_into_overflow() {
        let model = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions {
                cell_cap: 1,
                ..Default::default()
            },
        );
        assert_eq!(model.cells.len(), 1);
        assert_eq!(model.overflow, 1);
    }

    #[test]
    fn shared_y_domain_spans_all_cells_and_per_cell_scaling_does_not() {
        let shared = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions::default(),
        );
        // Fixture spans lows 990 .. highs 1310 across both worlds.
        assert!(shared.y_domain.0 < 1000.0 && shared.y_domain.1 > 1300.0);
        let per_cell = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions {
                shared_y: false,
                ..Default::default()
            },
        );
        // cells[1] = Gilgamesh (lower-priced): full-height in its own frame,
        // squashed to the lower band in the shared frame.
        assert_ne!(shared.cells[1].scene, per_cell.cells[1].scene);
    }

    #[test]
    fn sort_by_change_orders_by_relative_window_change() {
        // Gilgamesh vwap 1005 -> 1095 (+9.0%); Adamantoise 1205 -> 1295 (+7.5%).
        let model = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions {
                sort: GridSort::Change,
                ..Default::default()
            },
        );
        let names: Vec<&str> = model.cells.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Gilgamesh", "Adamantoise"],
            "biggest change first"
        );
        // Color still matches the overlay legend's name-sorted assignment:
        // Adamantoise = palette[0], Gilgamesh = palette[1] regardless of sort.
        assert_eq!(
            model.cells[0].color,
            GridOptions::default().theme.palette[1]
        );
    }

    #[test]
    fn cells_draw_marks_but_no_axis_text_or_volume() {
        let model = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions::default(),
        );
        for cell in &model.cells {
            assert!(
                cell.scene
                    .nodes
                    .iter()
                    .any(|n| matches!(n, Node::Polyline { .. }))
            );
            assert!(
                !cell
                    .scene
                    .nodes
                    .iter()
                    .any(|n| matches!(n, Node::Text { .. }))
            );
            assert!(
                !cell
                    .scene
                    .nodes
                    .iter()
                    .any(|n| matches!(n, Node::Rect { .. })),
                "no volume bars in cells"
            );
        }
    }

    #[test]
    fn candle_cells_emit_batched_paths() {
        let model = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions {
                mode: ChartMode::Candles,
                ..Default::default()
            },
        );
        for cell in &model.cells {
            let paths = cell
                .scene
                .nodes
                .iter()
                .filter(|n| matches!(n, Node::Path { .. }))
                .count();
            assert!((1..=3).contains(&paths), "wick + up/down bodies, batched");
        }
    }

    #[test]
    fn range_cells_emit_two_bands_and_a_median() {
        let model = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions {
                mode: ChartMode::Range,
                ..Default::default()
            },
        );
        for cell in &model.cells {
            let bands = cell
                .scene
                .nodes
                .iter()
                .filter(|n| matches!(n, Node::Path { fill: Some(_), .. }))
                .count();
            assert_eq!(bands, 2);
            let medians = cell
                .scene
                .nodes
                .iter()
                .filter(|n| matches!(n, Node::Polyline { .. }))
                .count();
            assert_eq!(medians, 1);
        }
    }

    #[test]
    fn xs_align_with_the_union_index_and_are_shared_by_all_cells() {
        let model = build_price_grid(
            &world_helper(),
            &two_world_series(),
            &GridOptions::default(),
        );
        assert_eq!(model.xs.len(), model.union.timestamps.len());
        assert!(model.xs.windows(2).all(|w| w[0] < w[1]));
        // Every cell exposes a value slot per union position.
        for cell in &model.cells {
            assert_eq!(cell.values.len(), model.union.timestamps.len());
        }
    }

    #[test]
    fn hiding_every_series_yields_an_empty_grid() {
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

    #[test]
    fn nearest_x_snaps_to_the_closest_position() {
        let xs = [10.0_f32, 20.0, 30.0];
        assert_eq!(nearest_x(&xs, -5.0), Some(0));
        assert_eq!(nearest_x(&xs, 14.0), Some(0));
        assert_eq!(nearest_x(&xs, 16.0), Some(1));
        assert_eq!(nearest_x(&xs, 99.0), Some(2));
        assert_eq!(nearest_x(&[], 1.0), None);
    }
}
