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
    /// Patch milestones (spec 4). Density is the exception to bands: a
    /// tinted background beneath a sequential color ramp would misread as
    /// data, so milestones degrade to boundary lines only here.
    pub milestones: Vec<crate::charts::MilestoneSpec>,
    pub theme: Theme,
}

impl Default for DensityChartOptions {
    fn default() -> Self {
        Self {
            width: 960.0,
            height: 540.0,
            utc_offset_minutes: 0,
            milestones: Vec::new(),
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
            hover: HoverModel {
                plot_top: 0.0,
                plot_bottom: 0.0,
                buckets: Vec::new(),
            },
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
    let price = LinearScale::new(
        (density.price_lo as f64, price_top),
        (plot_bottom, plot_top),
    );

    // ── Grid + axis labels (mirrors price_history) ──────────────────────
    for tick in price.ticks(5) {
        let y = price.scale(tick);
        scene.nodes.push(Node::Line {
            x1: plot_left,
            y1: y,
            x2: plot_right,
            y2: y,
            stroke: Stroke {
                color: theme.grid,
                width: 1.0,
                dash: None,
            },
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

    // ── Patch milestones: boundary lines only (never band rects) ────────
    let window_end = last_ts + TimeDelta::seconds(bucket_secs);
    for spec in &options.milestones {
        if spec.start > first_ts && spec.start < window_end {
            let x = time.scale(spec.start);
            scene.nodes.push(Node::Line {
                x1: x,
                y1: plot_top,
                x2: x,
                y2: plot_bottom,
                stroke: Stroke {
                    color: theme.text_muted.with_alpha(0.35),
                    width: 1.0,
                    dash: None,
                },
            });
        }
    }

    // ── Cells, one batched path per ramp step ───────────────────────────
    let max_n = density.max_count();
    let cell_w =
        (time.scale(first_ts + TimeDelta::seconds(bucket_secs)) - time.scale(first_ts)).max(1.0);
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
            let color = theme.density_ramp[step.min(theme.density_ramp.len().saturating_sub(1))];
            scene.nodes.push(Node::Path {
                d,
                fill: Some(color),
                stroke: None,
            });
        }
    }

    // ── Hover: one bucket per populated timestamp ───────────────────────
    let label_format = if bucket_secs < 86_400 {
        "%m-%d %H:%M"
    } else {
        "%Y-%m-%d"
    };
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
        hover: HoverModel {
            plot_top,
            plot_bottom,
            buckets,
        },
        total_sales,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Node;
    use ultros_api_types::price_density::DensityCell;

    fn ts(secs: i64) -> chrono::NaiveDateTime {
        chrono::DateTime::from_timestamp(secs, 0)
            .unwrap()
            .naive_utc()
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
            .map(|i| DensityCell {
                ts: ts((i % 10) * 86_400),
                bin: (i % 8) as u16,
                n: 1 + i as u32,
            })
            .collect();
        let expected_cells = cells.len();
        let model = build_price_density_chart(&fixture(cells), &DensityChartOptions::default());
        let cell_paths: Vec<&String> = model
            .scene
            .nodes
            .iter()
            .filter_map(|n| match n {
                Node::Path {
                    d, fill: Some(_), ..
                } => Some(d),
                _ => None,
            })
            .collect();
        assert!(
            cell_paths.len() <= 8,
            "one node per opacity step max, got {}",
            cell_paths.len()
        );
        let subpaths: usize = cell_paths.iter().map(|d| d.matches('M').count()).sum();
        assert_eq!(
            subpaths, expected_cells,
            "every populated cell draws exactly once"
        );
    }

    #[test]
    fn milestones_degrade_to_boundary_lines_never_band_rects() {
        let cells = vec![
            DensityCell {
                ts: ts(0),
                bin: 0,
                n: 2,
            },
            DensityCell {
                ts: ts(9 * 86_400),
                bin: 1,
                n: 4,
            },
        ];
        let model = build_price_density_chart(
            &fixture(cells),
            &DensityChartOptions {
                milestones: vec![crate::charts::MilestoneSpec {
                    start: ts(4 * 86_400),
                    version: 700,
                    ex_version: 5,
                }],
                ..Default::default()
            },
        );
        assert!(
            !model
                .scene
                .nodes
                .iter()
                .any(|n| matches!(n, Node::Rect { .. })),
            "density must never draw band rects beneath the ramp"
        );
        let vertical_lines = model
            .scene
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Line { x1, x2, .. } if x1 == x2))
            .count();
        assert_eq!(vertical_lines, 1, "one boundary line per in-window patch");
    }

    #[test]
    fn empty_grid_renders_the_no_data_card() {
        let model =
            build_price_density_chart(&fixture(Vec::new()), &DensityChartOptions::default());
        assert!(model.hover.buckets.is_empty());
        assert_eq!(model.total_sales, 0);
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
            DensityCell {
                ts: ts(0),
                bin: 0,
                n: 2,
            },
            DensityCell {
                ts: ts(0),
                bin: 3,
                n: 1,
            },
            DensityCell {
                ts: ts(86_400),
                bin: 1,
                n: 4,
            },
        ];
        let model = build_price_density_chart(&fixture(cells), &DensityChartOptions::default());
        assert_eq!(model.hover.buckets.len(), 2, "two distinct timestamps");
        assert!(model.hover.buckets.windows(2).all(|w| w[0].x <= w[1].x));
        assert_eq!(model.total_sales, 7);
    }
}
