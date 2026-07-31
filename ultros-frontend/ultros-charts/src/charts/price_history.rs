//! Layout for the sale-history market chart: a VWAP line per series with
//! the raw sales as dimmed dots behind it, and a volume lane along the
//! bottom. One function builds the whole picture so the server PNG and the
//! web chart (PR 2) can never drift apart.
//!
//! All bucketing and world-hierarchy grouping happens server-side now (see
//! `ultros_api_types::price_series`); this layout only resolves series ids
//! to display names, lays out geometry, and draws.

use std::collections::BTreeMap;

use chrono::{NaiveDateTime, TimeDelta};
use itertools::Itertools;
use ultros_api_types::price_series::{PriceBucket, PriceSeries, SeriesGroup};
use ultros_api_types::world_helper::{AnySelector, WorldHelper};

use crate::data::grouping::GroupLevel;
use crate::data::stats::median;
use crate::data::trend::least_squares;
use crate::scale::{LinearScale, TimeScale, short_number};
use crate::scene::{Color, Node, Scene, Stroke, TextAnchor};
use crate::svg::dots_path_d;
use crate::theme::Theme;

#[derive(Clone, Debug)]
pub struct PriceChartOptions {
    pub width: f32,
    pub height: f32,
    /// Ignored: outlier filtering now happens server-side (or not at all —
    /// the caller gates the toggle before requesting data).
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
    /// Ignored: grouping is now decided server-side and carried on the
    /// `PriceSeries` payload (`series.group`), which is authoritative. Kept
    /// on the struct because the frontend still sets it (Task 13 wires the
    /// request, not this layout).
    pub group_level: Option<GroupLevel>,
    /// Shift applied to axis/tooltip LABELS so the browser can show
    /// viewer-local times. Bucket boundaries and geometry stay UTC-aligned;
    /// keep 0 for SSR and PNG so server and first client render agree.
    pub utc_offset_minutes: i32,
    /// Series names the user hid via the legend. They stay in the model's
    /// `series` metadata (flagged `hidden`) but draw nothing, feed no hover
    /// values, and don't influence the axes.
    pub hidden_series: Vec<String>,
    /// Price-lane rendering mode. `Density` falls back to `Price` here —
    /// density has its own layout and payload.
    pub mode: crate::charts::ChartMode,
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
            mode: crate::charts::ChartMode::Price,
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
    /// The level the server actually grouped at (`series.group`, mapped).
    pub group_level: GroupLevel,
}

/// One `PriceSeriesEntry` resolved to a display name, dropping ids the
/// world helper doesn't recognize.
struct ResolvedSeries {
    id: i32,
    name: String,
    buckets: Vec<PriceBucket>,
}

/// Map a raw sale's world id up to the id space of `group` (world id
/// unchanged; datacenter/region id of the world it belongs to) — the same
/// hierarchy walk the server used to build `series.series`, needed here only
/// to bucket `series.raw` sales back onto their series for the dot layer.
fn series_id_for_world(
    world_helper: &WorldHelper,
    group: SeriesGroup,
    world_id: i32,
) -> Option<i32> {
    match group {
        SeriesGroup::World => Some(world_id),
        SeriesGroup::Datacenter => world_helper
            .lookup_selector(AnySelector::World(world_id))
            .and_then(|r| r.as_world())
            .map(|w| w.datacenter_id),
        SeriesGroup::Region => {
            let world = world_helper
                .lookup_selector(AnySelector::World(world_id))
                .and_then(|r| r.as_world())?;
            let datacenter = world_helper
                .lookup_selector(AnySelector::Datacenter(world.datacenter_id))
                .and_then(|r| r.as_datacenter())?;
            Some(datacenter.region_id)
        }
    }
}

pub fn build_price_history_chart(
    world_helper: &WorldHelper,
    series: &PriceSeries,
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

    let group_level = GroupLevel::from(series.group);
    let bucket_secs = series.bucket_seconds;

    let mut resolved: Vec<ResolvedSeries> = series
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
            Some(ResolvedSeries {
                id: entry.id,
                name,
                buckets: entry.buckets.clone(),
            })
        })
        .collect();
    resolved.sort_by(|a, b| a.name.cmp(&b.name));

    let is_hidden = |name: &str| options.hidden_series.iter().any(|h| h == name);
    let series_info: Vec<SeriesInfo> = resolved
        .iter()
        .enumerate()
        .map(|(index, s)| SeriesInfo {
            name: s.name.clone(),
            color: theme.palette[index % theme.palette.len()],
            hidden: is_hidden(&s.name),
        })
        .collect();
    let series_color = |index: usize| theme.palette[index % theme.palette.len()];
    let visible_count = resolved.iter().filter(|s| !is_hidden(&s.name)).count();

    let all_visible_buckets = || {
        resolved
            .iter()
            .filter(|s| !is_hidden(&s.name))
            .flat_map(|s| s.buckets.iter())
    };

    let Some((first_ts, last_ts)) = all_visible_buckets().map(|b| b.ts).minmax().into_option()
    else {
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
            group_level,
        };
    };
    let (min_price, max_price) = all_visible_buckets()
        .flat_map(|b| [b.low, b.high])
        .minmax()
        .into_option()
        .expect("non-empty by the timestamp check above");

    let stats = {
        let n: usize = all_visible_buckets().map(|b| b.sales as usize).sum();
        let total_gil: i64 = all_visible_buckets().map(|b| b.gil).sum();
        let total_units: i64 = all_visible_buckets().map(|b| b.units).sum();
        let market_average = (total_units > 0).then(|| (total_gil / total_units) as i32);
        let p50s: Vec<i32> = all_visible_buckets().map(|b| b.p50).collect();
        Some(ChartStats {
            n,
            market_average,
            median: median(&p50s),
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
    let mut volume_by_bucket: BTreeMap<NaiveDateTime, i64> = BTreeMap::new();
    for bucket in all_visible_buckets() {
        *volume_by_bucket.entry(bucket.ts).or_insert(0) += bucket.units;
    }
    if options.show_volume
        && let Some(max_volume) = volume_by_bucket.values().copied().max()
    {
        let volume = LinearScale::new((0.0, max_volume as f64), (plot_bottom, volume_top));
        let bucket_px =
            time.scale(first_ts + TimeDelta::seconds(bucket_secs)) - time.scale(first_ts);
        let bar_width = (bucket_px * 0.8).max(1.0);
        for (&start, &quantity) in &volume_by_bucket {
            let center = start + TimeDelta::seconds(bucket_secs / 2);
            let x = time.scale(center);
            let left = (x - bar_width / 2.0).max(plot_left);
            let right = (x + bar_width / 2.0).min(plot_right);
            if right <= left {
                continue;
            }
            let top = volume.scale(quantity as f64);
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

    // ── Raw sale dots (under the lines) ─────────────────────────────────
    // Only drawn when the payload carries individual sales — dense windows
    // omit `raw` and show only the VWAP lines.
    if let Some(raw) = &series.raw {
        for (index, s) in resolved.iter().enumerate() {
            if series_info[index].hidden {
                continue;
            }
            let color = series_color(index);
            let points: Vec<(f32, f32)> = raw
                .iter()
                .filter(|sale| {
                    series_id_for_world(world_helper, series.group, sale.world_id) == Some(s.id)
                })
                .map(|sale| {
                    (
                        time.scale(sale.sold_date),
                        price.scale(sale.price_per_item as f64),
                    )
                })
                .collect();
            if let Some(d) = dots_path_d(&points, 2.0) {
                scene.nodes.push(Node::Path {
                    d,
                    fill: Some(color.with_alpha(0.35)),
                    stroke: None,
                });
            }
        }
    }

    // ── VWAP lines (the primary visual) ─────────────────────────────────
    let mut hover_map: BTreeMap<NaiveDateTime, Vec<Option<(f32, f64)>>> = BTreeMap::new();
    for (index, s) in resolved.iter().enumerate() {
        if series_info[index].hidden {
            continue;
        }
        let color = series_color(index);
        let mut line: Vec<(f32, f32)> = Vec::new();
        for bucket in &s.buckets {
            let Some(vwap) = bucket.vwap() else {
                continue;
            };
            hover_map
                .entry(bucket.ts)
                .or_insert_with(|| vec![None; resolved.len()])[index] =
                Some((price.scale(vwap), vwap));
            let center = bucket.ts + TimeDelta::seconds(bucket_secs / 2);
            line.push((time.scale(center), price.scale(vwap)));
        }
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
    if options.show_market_average
        && let Some(market_average) = stats.as_ref().and_then(|s| s.market_average)
    {
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
    if options.show_trendline {
        let points: Vec<(f64, f64)> = all_visible_buckets()
            .filter_map(|b| {
                b.vwap().map(|vwap| {
                    let center_ts = b.ts.and_utc().timestamp() as f64 + bucket_secs as f64 / 2.0;
                    (center_ts, vwap)
                })
            })
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
    if options.show_legend && options.title.is_some() && resolved.len() > 1 {
        // Right-aligned row of "● Name" chips. 7px per char approximates
        // Jaldi at 13px — close enough for a legend.
        let mut x = plot_right;
        for (index, s) in resolved.iter().enumerate().rev() {
            x -= s.name.len() as f32 * 7.0;
            scene.nodes.push(Node::Text {
                x,
                y: 32.0,
                content: s.name.clone(),
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

    let label_format = if bucket_secs < 86_400 {
        "%m-%d %H:%M"
    } else {
        "%Y-%m-%d"
    };
    let hover_buckets: Vec<HoverBucket> = hover_map
        .into_iter()
        .map(|(start, series_values)| {
            let center = start + TimeDelta::seconds(bucket_secs / 2);
            let display = center + TimeDelta::minutes(options.utc_offset_minutes as i64);
            HoverBucket {
                x: time.scale(center),
                label: display.format(label_format).to_string(),
                series_values,
                volume: volume_by_bucket.get(&start).copied().unwrap_or(0),
            }
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
        group_level,
    }
}

pub fn build_price_history_scene(
    world_helper: &WorldHelper,
    series: &PriceSeries,
    options: &PriceChartOptions,
) -> Scene {
    build_price_history_chart(world_helper, series, options).scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Node;
    use crate::test_util::{bucket, two_world_series, world_helper};
    use ultros_api_types::CompactSale;
    use ultros_api_types::price_series::PriceSeriesEntry;

    fn count(scene: &crate::scene::Scene, predicate: impl Fn(&Node) -> bool) -> usize {
        scene.nodes.iter().filter(|n| predicate(n)).count()
    }

    #[test]
    fn renders_lines_volume_and_labels() {
        let scene = build_price_history_scene(
            &world_helper(),
            &two_world_series(),
            &PriceChartOptions {
                title: Some("Test Item".to_string()),
                ..Default::default()
            },
        );
        let polylines = count(&scene, |n| matches!(n, Node::Polyline { .. }));
        assert_eq!(polylines, 2, "one VWAP line per world series");
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
    fn raw_sales_present_draw_one_path_per_visible_series_and_no_circles() {
        let mut series = two_world_series();
        let raw: Vec<CompactSale> = (0..500)
            .map(|i| CompactSale {
                quantity: 1,
                price_per_item: 1_000 + i % 50,
                hq: false,
                sold_date: crate::test_util::ts(1_700_006_400 + (i as i64 * 1_000) % 864_000),
                world_id: 1 + (i % 2),
            })
            .collect();
        series.raw = Some(raw);
        let scene =
            build_price_history_scene(&world_helper(), &series, &PriceChartOptions::default());
        let paths = count(&scene, |n| matches!(n, Node::Path { .. }));
        assert_eq!(paths, 2, "one dot-path per visible series");
        let circles = count(&scene, |n| matches!(n, Node::Circle { .. }));
        assert_eq!(circles, 0, "raw sales must not emit per-sale Circle nodes");
    }

    #[test]
    fn raw_absent_draws_no_dot_paths() {
        let series = two_world_series();
        assert!(series.raw.is_none());
        let scene =
            build_price_history_scene(&world_helper(), &series, &PriceChartOptions::default());
        assert_eq!(count(&scene, |n| matches!(n, Node::Path { .. })), 0);
    }

    #[test]
    fn single_series_gets_area_fill() {
        let mut series = two_world_series();
        series.series.truncate(1);
        let scene =
            build_price_history_scene(&world_helper(), &series, &PriceChartOptions::default());
        assert_eq!(count(&scene, |n| matches!(n, Node::Area { .. })), 1);
        assert_eq!(count(&scene, |n| matches!(n, Node::Polyline { .. })), 1);
    }

    #[test]
    fn empty_series_renders_no_data_card() {
        let empty = PriceSeries {
            bucket_seconds: 86_400,
            group: SeriesGroup::World,
            from: crate::test_util::ts(0),
            to: crate::test_util::ts(0),
            series: Vec::new(),
            raw: None,
        };
        let scene =
            build_price_history_scene(&world_helper(), &empty, &PriceChartOptions::default());
        let has_no_data_text = scene
            .nodes
            .iter()
            .any(|n| matches!(n, Node::Text { content, .. } if content == "No recent sales"));
        assert!(has_no_data_text);
    }

    #[test]
    fn unresolvable_series_ids_are_dropped_not_shown_as_numbers() {
        let mut series = two_world_series();
        series.series.push(PriceSeriesEntry {
            id: 999,
            buckets: vec![bucket(1_700_006_400, 100, 120, 90, 105, 2)],
        });
        let model =
            build_price_history_chart(&world_helper(), &series, &PriceChartOptions::default());
        assert_eq!(
            model.series.len(),
            2,
            "unknown world id 999 must be dropped"
        );
        assert!(model.series.iter().all(|s| s.name != "999"));
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
            &two_world_series(),
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
            &two_world_series(),
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
            &two_world_series(),
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
            &two_world_series(),
            &PriceChartOptions::default(),
        );
        assert_eq!(model.series.len(), 2);
        assert!(!model.hover.buckets.is_empty());
        for bucket in &model.hover.buckets {
            assert_eq!(bucket.series_values.len(), 2);
            assert!(!bucket.label.is_empty());
        }
        // sorted by x
        assert!(model.hover.buckets.windows(2).all(|w| w[0].x <= w[1].x));
        let stats = model.stats.expect("stats for non-empty series");
        assert_eq!(stats.n, 60, "10 buckets x 2 series x 3 sales/bucket");
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
        let scene = build_price_history_scene(
            &world_helper(),
            &two_world_series(),
            &PriceChartOptions::default(),
        );
        let model = build_price_history_chart(
            &world_helper(),
            &two_world_series(),
            &PriceChartOptions::default(),
        );
        assert_eq!(scene, model.scene);
    }

    #[test]
    fn hidden_series_are_excluded_from_drawing_but_kept_in_metadata() {
        let model = build_price_history_chart(
            &world_helper(),
            &two_world_series(),
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
            &two_world_series(),
            &PriceChartOptions {
                hidden_series: vec!["Gilgamesh".to_string(), "Adamantoise".to_string()],
                ..Default::default()
            },
        );
        assert!(model.hover.buckets.is_empty());
        assert_eq!(model.series.len(), 2, "legend must still offer un-hiding");
    }

    #[test]
    fn hover_volume_corresponds_to_bucket_sales() {
        let model = build_price_history_chart(
            &world_helper(),
            &two_world_series(),
            &PriceChartOptions::default(),
        );
        // Every bucket in the fixture carries 2 units per series, 2 series,
        // 10 buckets each -> 40 total units across the hover buckets.
        let total: i64 = model.hover.buckets.iter().map(|b| b.volume).sum();
        assert_eq!(total, 40);
        for bucket in &model.hover.buckets {
            assert!(
                bucket.series_values.iter().any(|v| v.is_some()),
                "no orphan hover buckets"
            );
            assert!(
                bucket.volume > 0,
                "aligned volume for every populated bucket"
            );
        }
    }

    #[test]
    fn buckets_with_zero_units_are_skipped_as_vwap_gaps() {
        let mut series = two_world_series();
        // Zero out units on one bucket of the first series: vwap() is None,
        // so it must be skipped as a gap, not rendered as a zero.
        series.series[0].buckets[0].units = 0;
        series.series[0].buckets[0].gil = 0;
        let model =
            build_price_history_chart(&world_helper(), &series, &PriceChartOptions::default());
        // 9 remaining buckets from series[0] (Gilgamesh) plus 10 from
        // series[1] (Adamantoise) union to 10 hover buckets (the gap bucket
        // is still populated by the other series).
        assert_eq!(model.hover.buckets.len(), 10);
        let gap_bucket = model
            .hover
            .buckets
            .iter()
            .min_by(|a, b| a.x.partial_cmp(&b.x).unwrap())
            .unwrap();
        // Gilgamesh sorts after Adamantoise (index 1); its slot is None at
        // the gap timestamp.
        assert!(gap_bucket.series_values[1].is_none());
        assert!(gap_bucket.series_values[0].is_some());
    }

    #[test]
    fn empty_series_yield_empty_model_with_no_data_scene() {
        let empty = PriceSeries {
            bucket_seconds: 86_400,
            group: SeriesGroup::World,
            from: crate::test_util::ts(0),
            to: crate::test_util::ts(0),
            series: Vec::new(),
            raw: None,
        };
        let model =
            build_price_history_chart(&world_helper(), &empty, &PriceChartOptions::default());
        assert!(model.hover.buckets.is_empty());
        assert!(model.stats.is_none());
        assert!(
            model
                .scene
                .nodes
                .iter()
                .any(|n| matches!(n, Node::Text { content, .. } if content == "No recent sales"))
        );
    }

    #[test]
    fn group_level_reflects_the_payloads_group_not_options() {
        let mut series = two_world_series();
        series.group = SeriesGroup::Datacenter;
        series.series = vec![PriceSeriesEntry {
            id: 1,
            buckets: vec![bucket(1_700_006_400, 100, 120, 90, 105, 2)],
        }];
        let model = build_price_history_chart(
            &world_helper(),
            &series,
            &PriceChartOptions {
                // Deliberately mismatched with `series.group` — must be ignored.
                group_level: Some(GroupLevel::World),
                ..Default::default()
            },
        );
        assert_eq!(model.group_level, GroupLevel::Datacenter);
    }
}
