//! Layout for the sale-history market chart: a VWAP line per series with
//! the raw sales as dimmed dots behind it, and a volume lane along the
//! bottom. One function builds the whole picture so the server PNG and the
//! web chart (PR 2) can never drift apart.

use std::borrow::Cow;
use std::collections::BTreeMap;

use chrono::TimeDelta;
use itertools::Itertools;
use ultros_api_types::SaleHistory;
use ultros_api_types::world_helper::WorldHelper;

use crate::data::buckets::{bucket_seconds, volume_buckets_from_points, vwap_buckets};
use crate::data::grouping::{GroupLevel, auto_group_level, group_sales_by_level};
use crate::data::outliers::filter_outliers;
use crate::data::stats::{median, vwap};
use crate::data::trend::least_squares;
use crate::scale::{LinearScale, TimeScale, short_number};
use crate::scene::{Color, Node, Scene, Stroke, TextAnchor};
use crate::theme::Theme;

#[derive(Clone, Debug)]
pub struct PriceChartOptions {
    pub width: f32,
    pub height: f32,
    pub remove_outliers: bool,
    pub show_market_average: bool,
    pub show_trendline: bool,
    pub show_volume: bool,
    /// Drawn in the title row, so only meaningful when `title` is set
    /// (the web chart renders its legend as HTML chips instead).
    pub show_legend: bool,
    /// Card title (item name); `None` hides the title row (web — the page
    /// already shows the item name).
    pub title: Option<String>,
    /// `data:image/png;base64,…` icon shown beside the title.
    pub icon_data_uri: Option<String>,
    /// User-selected day window (7/30/90); `None`/0 = derive from data span.
    pub days_range: Option<i32>,
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
    pub theme: Theme,
}

impl Default for PriceChartOptions {
    fn default() -> Self {
        Self {
            width: 960.0,
            height: 540.0,
            remove_outliers: false,
            show_market_average: true,
            show_trendline: false,
            show_volume: true,
            show_legend: true,
            title: None,
            icon_data_uri: None,
            days_range: None,
            group_level: None,
            utc_offset_minutes: 0,
            hidden_series: Vec::new(),
            theme: Theme::dark_card(),
        }
    }
}

/// Trim a segment to the horizontal band `[y_top, y_bottom]`, preserving
/// slope — used to keep the trendline inside the price lane.
fn clip_segment_to_band(
    (x1, y1): (f32, f32),
    (x2, y2): (f32, f32),
    y_top: f32,
    y_bottom: f32,
) -> Option<((f32, f32), (f32, f32))> {
    if y1 == y2 {
        return (y1 >= y_top && y1 <= y_bottom).then_some(((x1, y1), (x2, y2)));
    }
    let t_for = |y: f32| (y - y1) / (y2 - y1);
    let (ta, tb) = (t_for(y_top), t_for(y_bottom));
    let (t_min, t_max) = if ta < tb { (ta, tb) } else { (tb, ta) };
    let t0 = t_min.max(0.0);
    let t1 = t_max.min(1.0);
    if t0 >= t1 {
        return None;
    }
    let point_at = |t: f32| (x1 + t * (x2 - x1), y1 + t * (y2 - y1));
    Some((point_at(t0), point_at(t1)))
}

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

pub fn build_price_history_chart(
    world_helper: &WorldHelper,
    sales: &[SaleHistory],
    options: &PriceChartOptions,
) -> PriceChartModel {
    let theme = &options.theme;
    let mut scene = Scene {
        width: options.width,
        height: options.height,
        background: theme.background,
        font_family: theme.font_family.clone(),
        nodes: Vec::new(),
    };

    let sales = if options.remove_outliers {
        filter_outliers(sales)
    } else {
        Cow::Borrowed(sales)
    };
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
    let (min_price, max_price) = all_points()
        .map(|p| p.price)
        .minmax()
        .into_option()
        .expect("non-empty by the timestamp check above");

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

    // ── Geometry ────────────────────────────────────────────────────────
    let title_height = if options.title.is_some() { 56.0 } else { 12.0 };
    let margin_left = 68.0;
    let margin_right = 16.0;
    let margin_bottom = 32.0;
    let plot_left = margin_left;
    let plot_right = options.width - margin_right;
    let plot_top = title_height;
    let plot_bottom = options.height - margin_bottom;
    let plot_height = plot_bottom - plot_top;
    let (volume_top, price_bottom) = if options.show_volume {
        let volume_height = plot_height * 0.22;
        (
            plot_bottom - volume_height,
            plot_bottom - volume_height - 10.0,
        )
    } else {
        (plot_bottom, plot_bottom)
    };

    let time = TimeScale::new(first_ts, last_ts, (plot_left, plot_right));
    // Don't anchor the price axis at zero: gil prices cluster far above it
    // and the signal is the variation. 5% headroom on both sides.
    let price_pad = ((max_price - min_price) as f64 * 0.05).max(1.0);
    let price = LinearScale::new(
        (
            (min_price as f64 - price_pad).max(0.0),
            max_price as f64 + price_pad,
        ),
        (price_bottom, plot_top),
    );

    // ── Grid + axis labels ──────────────────────────────────────────────
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
        let x = time.scale(tick.ts);
        scene.nodes.push(Node::Text {
            x,
            y: plot_bottom + 20.0,
            content: tick.label,
            size: 13.0,
            color: theme.text_muted,
            anchor: TextAnchor::Middle,
            bold: false,
        });
    }

    // ── Volume lane ─────────────────────────────────────────────────────
    let span_days = (last_ts - first_ts).num_days();
    let bucket_secs = bucket_seconds(options.days_range, span_days);
    let volumes = volume_buckets_from_points(all_points(), bucket_secs);
    if options.show_volume {
        if let Some(max_volume) = volumes.iter().map(|v| v.quantity).max() {
            let volume = LinearScale::new((0.0, max_volume as f64), (plot_bottom, volume_top));
            let bucket_px =
                time.scale(first_ts + TimeDelta::seconds(bucket_secs)) - time.scale(first_ts);
            let bar_width = (bucket_px * 0.8).max(1.0);
            for bucket in &volumes {
                let center = bucket.ts + TimeDelta::seconds(bucket_secs / 2);
                let x = time.scale(center);
                let left = (x - bar_width / 2.0).max(plot_left);
                let right = (x + bar_width / 2.0).min(plot_right);
                if right <= left {
                    continue;
                }
                let top = volume.scale(bucket.quantity as f64);
                scene.nodes.push(Node::Rect {
                    x: left,
                    y: top,
                    width: right - left,
                    height: (plot_bottom - top).max(1.0),
                    rx: 1.0,
                    fill: theme.volume.with_alpha(0.7),
                });
            }
        }
    }

    // ── Raw sale dots (under the lines) ─────────────────────────────────
    let series_color = |index: usize| theme.palette[index % theme.palette.len()];
    for (index, group) in series.iter().enumerate() {
        if series_info[index].hidden {
            continue;
        }
        let color = series_color(index);
        for point in &group.points {
            scene.nodes.push(Node::Circle {
                cx: time.scale(point.ts),
                cy: price.scale(point.price as f64),
                r: 2.0,
                fill: color.with_alpha(0.35),
            });
        }
    }

    // ── VWAP lines (the primary visual) ─────────────────────────────────
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

    // ── Overlays ────────────────────────────────────────────────────────
    if options.show_market_average {
        let pairs: Vec<(i32, i32)> = all_points().map(|p| (p.price, p.quantity)).collect();
        if let Some(market_average) = vwap(&pairs) {
            let y = price.scale(market_average as f64);
            scene.nodes.push(Node::Line {
                x1: plot_left,
                y1: y,
                x2: plot_right,
                y2: y,
                stroke: Stroke {
                    color: theme.market_average.with_alpha(0.9),
                    width: 1.5,
                    dash: Some((2.0, 4.0)),
                },
            });
        }
    }
    if options.show_trendline {
        let points: Vec<(f64, f64)> = all_points()
            .map(|p| (p.ts.and_utc().timestamp() as f64, p.price as f64))
            .collect();
        if let Some((slope, intercept)) = least_squares(&points) {
            let x1 = first_ts.and_utc().timestamp() as f64;
            let x2 = last_ts.and_utc().timestamp() as f64;
            let start = (time.scale(first_ts), price.scale(intercept + slope * x1));
            let end = (time.scale(last_ts), price.scale(intercept + slope * x2));
            if let Some(((x1, y1), (x2, y2))) =
                clip_segment_to_band(start, end, plot_top, price_bottom)
            {
                scene.nodes.push(Node::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    stroke: Stroke {
                        color: theme.trend.with_alpha(0.8),
                        width: 1.5,
                        dash: Some((6.0, 4.0)),
                    },
                });
            }
        }
    }

    // ── Title row: icon + title left, legend chips right ───────────────
    if let Some(title) = &options.title {
        let mut x = 16.0;
        if let Some(icon) = &options.icon_data_uri {
            scene.nodes.push(Node::Image {
                x,
                y: 8.0,
                width: 40.0,
                height: 40.0,
                href: icon.clone(),
            });
            x += 48.0;
        }
        scene.nodes.push(Node::Text {
            x,
            y: 36.0,
            content: title.clone(),
            size: 24.0,
            color: theme.text,
            anchor: TextAnchor::Start,
            bold: true,
        });
    }
    if options.show_legend && options.title.is_some() && series.len() > 1 {
        // Right-aligned row of "● Name" chips. 7px per char approximates
        // Jaldi at 13px — close enough for a legend.
        let mut x = plot_right;
        for (index, group) in series.iter().enumerate().rev() {
            x -= group.name.len() as f32 * 7.0;
            scene.nodes.push(Node::Text {
                x,
                y: 32.0,
                content: group.name.clone(),
                size: 13.0,
                color: theme.text,
                anchor: TextAnchor::Start,
                bold: false,
            });
            x -= 12.0;
            scene.nodes.push(Node::Circle {
                cx: x + 4.0,
                cy: 28.0,
                r: 4.0,
                fill: series_color(index),
            });
            x -= 14.0;
        }
    }

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
}

pub fn build_price_history_scene(
    world_helper: &WorldHelper,
    sales: &[SaleHistory],
    options: &PriceChartOptions,
) -> Scene {
    build_price_history_chart(world_helper, sales, options).scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Node;
    use crate::test_util::{sale, ts, world_helper};
    use ultros_api_types::SaleHistory;

    fn count(scene: &crate::scene::Scene, predicate: impl Fn(&Node) -> bool) -> usize {
        scene.nodes.iter().filter(|n| predicate(n)).count()
    }

    fn two_world_sales() -> Vec<SaleHistory> {
        // 20 sales over ~10 days alternating between two worlds of one DC
        (0..20)
            .map(|i| {
                sale(
                    1_000 + i * 10,
                    2,
                    1 + (i % 2),
                    ts(1_700_000_000 + i as i64 * 43_200),
                )
            })
            .collect()
    }

    #[test]
    fn renders_lines_dots_volume_and_labels() {
        let scene = build_price_history_scene(
            &world_helper(),
            &two_world_sales(),
            &PriceChartOptions {
                title: Some("Test Item".to_string()),
                ..Default::default()
            },
        );
        let polylines = count(&scene, |n| matches!(n, Node::Polyline { .. }));
        assert_eq!(polylines, 2, "one VWAP line per world series");
        let circles = count(&scene, |n| matches!(n, Node::Circle { .. }));
        assert!(circles >= 20, "raw sale dots (plus legend chips)");
        let bars = count(&scene, |n| matches!(n, Node::Rect { .. }));
        assert!(bars >= 1, "volume lane bars");
        let texts: Vec<_> = scene
            .nodes
            .iter()
            .filter_map(|n| match n {
                Node::Text { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&"Test Item"));
        assert!(texts.contains(&"Gilgamesh"), "legend entries present");
    }

    #[test]
    fn single_series_gets_area_fill() {
        let sales: Vec<_> = (0..20)
            .map(|i| sale(1_000 + i * 10, 2, 1, ts(1_700_000_000 + i as i64 * 43_200)))
            .collect();
        let scene =
            build_price_history_scene(&world_helper(), &sales, &PriceChartOptions::default());
        assert_eq!(count(&scene, |n| matches!(n, Node::Area { .. })), 1);
        assert_eq!(count(&scene, |n| matches!(n, Node::Polyline { .. })), 1);
    }

    #[test]
    fn empty_sales_renders_no_data_card() {
        let scene = build_price_history_scene(&world_helper(), &[], &PriceChartOptions::default());
        let has_no_data_text = scene
            .nodes
            .iter()
            .any(|n| matches!(n, Node::Text { content, .. } if content == "No recent sales"));
        assert!(has_no_data_text);
    }

    #[test]
    fn clip_keeps_inside_segments_and_trims_crossings() {
        // Fully inside: unchanged
        assert_eq!(
            clip_segment_to_band((0.0, 5.0), (10.0, 6.0), 0.0, 10.0),
            Some(((0.0, 5.0), (10.0, 6.0)))
        );
        // Crosses the bottom: trimmed at y=10, slope preserved
        let ((ax, ay), (bx, by)) =
            clip_segment_to_band((0.0, 0.0), (10.0, 20.0), 0.0, 10.0).unwrap();
        assert_eq!((ax, ay), (0.0, 0.0));
        assert_eq!((bx, by), (5.0, 10.0));
        // Entirely outside: dropped
        assert_eq!(
            clip_segment_to_band((0.0, 20.0), (10.0, 30.0), 0.0, 10.0),
            None
        );
    }

    #[test]
    fn volume_bars_stay_inside_plot_bounds() {
        let scene = build_price_history_scene(
            &world_helper(),
            &two_world_sales(),
            &PriceChartOptions::default(),
        );
        for node in &scene.nodes {
            if let Node::Rect { x, width, .. } = node {
                assert!(*x >= 68.0 - 0.01, "bar starts left of plot: {x}");
                assert!(x + width <= 960.0 - 16.0 + 0.01, "bar ends right of plot");
            }
        }
    }

    #[test]
    fn hiding_volume_emits_no_bars() {
        let scene = build_price_history_scene(
            &world_helper(),
            &two_world_sales(),
            &PriceChartOptions {
                show_volume: false,
                ..Default::default()
            },
        );
        assert_eq!(count(&scene, |n| matches!(n, Node::Rect { .. })), 0);
        assert_eq!(count(&scene, |n| matches!(n, Node::Polyline { .. })), 2);
    }

    #[test]
    fn trendline_stays_inside_the_price_lane() {
        let scene = build_price_history_scene(
            &world_helper(),
            &two_world_sales(),
            &PriceChartOptions {
                show_trendline: true,
                show_market_average: false,
                ..Default::default()
            },
        );
        // With market average off and no title, the only dashed Line nodes
        // are gridless overlays: exactly one trendline.
        let trend_lines: Vec<_> = scene
            .nodes
            .iter()
            .filter_map(|n| match n {
                Node::Line { y1, y2, stroke, .. } if stroke.dash.is_some() => Some((*y1, *y2)),
                _ => None,
            })
            .collect();
        assert_eq!(trend_lines.len(), 1);
        let (y1, y2) = trend_lines[0];
        // No title → plot_top = 12; volume lane top boundary = price_bottom.
        // Just assert the broad invariant: inside the drawing area, above the
        // bottom margin.
        for y in [y1, y2] {
            assert!(
                (12.0..=540.0 - 32.0).contains(&y),
                "trendline endpoint y={y} escaped"
            );
        }
    }

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
}
