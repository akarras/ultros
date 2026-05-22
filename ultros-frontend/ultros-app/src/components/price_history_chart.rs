use std::collections::BTreeMap;

use leptos::prelude::*;
use leptos_chartistry::*;
use ultros_api_types::SaleHistory;
use ultros_api_types::world_helper::AnySelector;

use crate::global_state::LocalWorldData;
use crate::i18n::{t, t_string, use_i18n};

type SeriesPoints = Vec<(chrono::DateTime<chrono::Local>, i32, i32)>;

const CATEGORY_PALETTE: [&str; 12] = [
    "#60a5fa", "#f97316", "#34d399", "#a78bfa", "#fb7185", "#facc15", "#22d3ee", "#c084fc",
    "#4ade80", "#f472b6", "#94a3b8", "#fdba74",
];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ColorBy {
    Region,
    Datacenter,
    World,
}

impl ColorBy {
    fn label(self) -> &'static str {
        match self {
            Self::Region => "Region",
            Self::Datacenter => "Datacenter",
            Self::World => "World",
        }
    }
}

/// Roll sales up by a chosen world hierarchy level for colouring the scatter points.
fn group_sales_by_level(
    helper: &ultros_api_types::world_helper::WorldHelper,
    sales: &[SaleHistory],
    color_by: ColorBy,
) -> Vec<(String, SeriesPoints)> {
    use itertools::Itertools;

    let mut groups = BTreeMap::<AnySelector, (String, SeriesPoints)>::new();
    for sale in sales {
        let world = match helper
            .lookup_selector(AnySelector::World(sale.world_id))
            .and_then(|result| result.as_world())
        {
            Some(world) => world,
            None => continue,
        };
        let selector = match color_by {
            ColorBy::World => AnySelector::World(world.id),
            ColorBy::Datacenter => AnySelector::Datacenter(world.datacenter_id),
            ColorBy::Region => {
                let datacenter = match helper
                    .lookup_selector(AnySelector::Datacenter(world.datacenter_id))
                    .and_then(|result| result.as_datacenter())
                {
                    Some(datacenter) => datacenter,
                    None => continue,
                };
                AnySelector::Region(datacenter.region_id)
            }
        };
        let Some(result) = helper.lookup_selector(selector) else {
            continue;
        };
        // `sold_date` is a naive UTC instant. `and_local_timezone(Local)`
        // re-interprets it as local time, silently shifting by the viewer's
        // UTC offset (8h on a Shanghai client, 0h on a UTC server). The
        // downstream `.with_timezone(&Utc)` then bakes the wrong value into
        // `SaleRow.ts`, producing SSR/CSR row sets that disagree and tripping
        // tachys hydration at `hydration.rs:227`.
        let sold_date = sale.sold_date.and_utc().with_timezone(&chrono::Local);
        groups
            .entry(selector)
            .or_insert_with(|| (result.get_name().to_string(), Vec::new()))
            .1
            .push((sold_date, sale.price_per_item, sale.quantity));
    }

    groups
        .into_values()
        .sorted_by_cached_key(|(name, _)| name.clone())
        .collect()
}

/// Volume-weighted average price. Returns None if the input is empty or total qty is 0.
fn vwap(prices_and_qty: &[(i32, i32)]) -> Option<i32> {
    let (num, den) = prices_and_qty
        .iter()
        .fold((0i64, 0i64), |(n, d), (price, qty)| {
            (n + (*price as i64) * (*qty as i64), d + (*qty as i64))
        });
    if den == 0 {
        return None;
    }
    Some((num / den) as i32)
}

/// Median price. For even counts, returns the integer mean of the two middle values.
fn median(prices: &[i32]) -> Option<i32> {
    if prices.is_empty() {
        return None;
    }
    let mut sorted: Vec<i32> = prices.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    if n % 2 == 1 {
        Some(sorted[n / 2])
    } else {
        Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2)
    }
}

/// IQR-based outlier band, matching the existing logic in `ultros-charts`.
/// Returns (min, max) where min = Q1 - 2.5*IQR, max = Q3 + 2.5*IQR.
/// Returns None for samples smaller than 10.
fn iqr_band(prices: &[i32]) -> Option<(i32, i32)> {
    if prices.len() < 10 {
        return None;
    }
    let mut sorted: Vec<i32> = prices.to_vec();
    sorted.sort_unstable();
    let q1_idx = sorted.len() / 4;
    let q3_idx = sorted.len() - q1_idx;
    let q1 = *sorted.get(q1_idx)?;
    let q3 = *sorted.get(q3_idx)?;
    let widened = ((q3 - q1) as f32 * 2.5) as i32;
    Some((q1 - widened, q3 + widened))
}

/// Format an integer price using K/mil shortening, same rules as the plotters chart.
fn short_number(value: i32) -> String {
    match value {
        1_000_000.. => format!("{:.2}mil", value as f32 / 1_000_000.0),
        1_000..=999_999 => format!("{:.2}K", value as f32 / 1_000.0),
        _ => value.to_string(),
    }
}

/// Computed statistics for the stats strip.
#[derive(Clone, Debug, PartialEq)]
struct ChartStats {
    n: usize,
    market_average_val: Option<i32>,
    median_val: Option<i32>,
    min_val: i32,
    max_val: i32,
}

/// One row in the flat chartistry data Vec.
/// Each row represents a single sale event. Series that don't have a point
/// for this row carry f64::NAN, which chartistry skips when `skip_missing()`
/// is set on the tooltip.
#[derive(Clone, Debug, PartialEq)]
struct SaleRow {
    /// Sale timestamp, used as the X axis. chartistry's built-in `DateTime<Utc>`
    /// `Tick` impl automatically formats labels as dates.
    ts: chrono::DateTime<chrono::Utc>,
    /// Price per item (f64 for chartistry).
    price: f64,
    /// The colour/category series this sale belongs to.
    group_index: usize,
    /// Quantity-weighted average price overlay (constant across all rows, or NAN).
    market_average_y: f64,
    /// Least-squares trendline value at this row's timestamp (or NAN).
    trend_y: f64,
}

/// Aggregated quantity for a time bucket. The quantity sub-chart renders these
/// as one bar per bucket so bars stay visible even when sales cluster in time.
#[derive(Clone, Debug, PartialEq)]
struct QuantityBucket {
    ts: chrono::DateTime<chrono::Utc>,
    quantity: f64,
}

/// Pick a bucket size (in seconds) for the quantity histogram based on the
/// active days window. `days_range == 0` means "all" — fall back to the data span.
fn quantity_bucket_seconds(days_range: i32, data_span_days: i64) -> i64 {
    const HOUR: i64 = 3_600;
    const DAY: i64 = 86_400;
    const WEEK: i64 = DAY * 7;
    let effective_days = if days_range > 0 {
        days_range as i64
    } else {
        data_span_days.max(1)
    };
    match effective_days {
        ..=2 => HOUR,
        3..=10 => 6 * HOUR,
        11..=45 => DAY,
        46..=120 => DAY,
        121..=400 => WEEK,
        _ => DAY * 30,
    }
}

/// Bucket sales by `bucket_secs`, summing quantities into each bucket. Buckets
/// are aligned to absolute timestamps (UTC) so they line up with calendar boundaries
/// for day/week/month sizes.
fn bucket_quantities(sales: &[SaleHistory], bucket_secs: i64) -> Vec<QuantityBucket> {
    if bucket_secs <= 0 || sales.is_empty() {
        return Vec::new();
    }
    let mut sums: BTreeMap<i64, i64> = BTreeMap::new();
    for s in sales {
        let ts = s.sold_date.and_utc().timestamp();
        let bucket = (ts.div_euclid(bucket_secs)) * bucket_secs;
        *sums.entry(bucket).or_default() += s.quantity as i64;
    }
    sums.into_iter()
        .filter_map(|(ts, q)| {
            chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0).map(|dt| QuantityBucket {
                ts: dt,
                quantity: q as f64,
            })
        })
        .collect()
}

/// Choose tick `Period`s for the X axis based on the selected window. Chartistry
/// will pick the densest period that still fits the available width, so we pass
/// a small set bracketing the expected scale rather than a single period.
fn x_axis_periods(days_range: i32, data_span_days: i64) -> Vec<Period> {
    let effective_days = if days_range > 0 {
        days_range as i64
    } else {
        data_span_days.max(1)
    };
    match effective_days {
        ..=2 => vec![Period::Hour, Period::Day],
        3..=10 => vec![Period::Day, Period::Hour],
        11..=45 => vec![Period::Day, Period::Month],
        46..=120 => vec![Period::Day, Period::Month],
        121..=400 => vec![Period::Month, Period::Day],
        _ => vec![Period::Month, Period::Year],
    }
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
                                .market_average_val
                                .map(|v| {
                                    view! {
                                        <span>
                                            {market_average_label.clone()} " " {short_number(v)}
                                        </span>
                                    }
                                })}
                            {s
                                .median_val
                                .map(|v| {
                                    view! {
                                        <span>
                                            {median_label} " " {short_number(v)}
                                        </span>
                                    }
                                })}
                            <span>{min_label} " " {short_number(s.min_val)}</span>
                            <span>{max_label} " " {short_number(s.max_val)}</span>
                        </div>
                    }
                        .into_any()
                })
        }}
    }
}

// ── Main component ────────────────────────────────────────────────────────────

#[component]
pub fn PriceHistoryChart(
    #[prop(into)] sales: Signal<Vec<SaleHistory>>,
    #[prop(into)] filter_outliers: Signal<bool>,
    #[prop(into)] scope_name: Signal<String>,
    /// Selected days window from the parent (7 / 30 / 90 / 0 for All).
    /// Used to size quantity buckets and pick X-axis tick periods.
    #[prop(into)]
    days_range: Signal<i32>,
) -> impl IntoView {
    // Implementation uses leptos-chartistry 0.2 for axes/overlays. Sales stay in
    // one dense series because sparse category series produce misleading tooltip
    // rows and NaN path warnings; marker colours are applied from the grouping
    // metadata after chartistry renders the circles.

    let local_world_data = use_context::<LocalWorldData>().unwrap();
    let helper = local_world_data.0.unwrap();
    let i18n = use_i18n();
    let (show_market_average, set_show_market_average) = signal(true);
    let (show_trend, set_show_trend) = signal(false);
    let (show_quantity, set_show_quantity) = signal(false);
    let (color_by, set_color_by) = signal(ColorBy::World);

    // Optional IQR outlier filter applied to the incoming (pre-filtered) sales.
    let filtered = Memo::new(move |_| {
        let base = sales.get();
        if !filter_outliers.get() {
            return base;
        }
        let prices: Vec<i32> = base.iter().map(|s| s.price_per_item).collect();
        match iqr_band(&prices) {
            Some((lo, hi)) => base
                .into_iter()
                .filter(|s| s.price_per_item >= lo && s.price_per_item <= hi)
                .collect(),
            None => base,
        }
    });

    let helper_for_color_options = helper.clone();
    let color_by_options = Memo::new(move |_| {
        match helper_for_color_options.lookup_world_by_name(&scope_name.get()) {
            Some(result) if result.as_world().is_some() => vec![ColorBy::World],
            Some(result) if result.as_datacenter().is_some() => {
                vec![ColorBy::Datacenter, ColorBy::World]
            }
            Some(result) if result.as_region().is_some() => {
                vec![ColorBy::Region, ColorBy::Datacenter, ColorBy::World]
            }
            _ => vec![ColorBy::Region, ColorBy::Datacenter, ColorBy::World],
        }
    });

    let effective_color_by = Memo::new(move |_| {
        let selected = color_by.get();
        let options = color_by_options.get();
        if options.contains(&selected) {
            selected
        } else {
            *options.last().unwrap_or(&ColorBy::World)
        }
    });

    // Stats computed from the sales currently visible in the chart.
    let stats = Memo::new(move |_| {
        let data = filtered.get();
        if data.is_empty() {
            return None;
        }
        let prices: Vec<i32> = data.iter().map(|s| s.price_per_item).collect();
        let pq: Vec<(i32, i32)> = data
            .iter()
            .map(|s| (s.price_per_item, s.quantity))
            .collect();
        let min_val = *prices.iter().min().unwrap();
        let max_val = *prices.iter().max().unwrap();
        Some(ChartStats {
            n: data.len(),
            market_average_val: vwap(&pq),
            median_val: median(&prices),
            min_val,
            max_val,
        })
    });

    // Group by locale and build flat row vec for chartistry.
    // We produce a derived signal so chartistry's `data` prop stays reactive.
    // Series names come from group_sales_by_locale; order is stable (sorted).
    let helper_clone = helper.clone();
    let chart_data = Memo::new(move |_| {
        let data = filtered.get();
        let groups = group_sales_by_level(&helper_clone, &data, effective_color_by.get());
        if groups.is_empty() || data.is_empty() {
            return (vec![], vec![]);
        }
        // series_names: stable order (already sorted by group_sales_by_locale)
        let series_names: Vec<String> = groups.iter().map(|(n, _)| n.clone()).collect();

        // Flatten all points into (ts, price, qty). Chartistry does not skip
        // sparse series before building ranges, so keep the sales as one
        // contiguous scatter series and use locale names only for the legend.
        let mut flat: Vec<(chrono::DateTime<chrono::Utc>, usize, f64, i32)> = groups
            .iter()
            .enumerate()
            .flat_map(|(group_index, (_, points))| {
                points.iter().map(move |(dt, price, qty)| {
                    let utc = dt.with_timezone(&chrono::Utc);
                    (utc, group_index, *price as f64, *qty)
                })
            })
            .collect();
        // Sort by timestamp so chartistry's line renderer doesn't cross itself
        flat.sort_by(|a, b| a.0.cmp(&b.0));

        // ── Overlay computations ──────────────────────────────────────────

        // Market average (from filtered, same as stats strip). This is a
        // quantity-weighted average, but the UI avoids market-jargon labels.
        let pq_filtered: Vec<(i32, i32)> = data
            .iter()
            .map(|s| (s.price_per_item, s.quantity))
            .collect();
        let market_average_val = vwap(&pq_filtered).map(|v| v as f64).unwrap_or(f64::NAN);

        // Trendline via least-squares on (ts_secs, price) from filtered set
        // y = b + m*x  where x = timestamp in seconds
        let trend_points: Vec<(f64, f64)> = flat
            .iter()
            .map(|(ts, _, price, _)| (ts.timestamp() as f64, *price))
            .collect();
        let n = trend_points.len() as f64;
        let (sum_x, sum_y, sum_xx, sum_xy) = trend_points.iter().fold(
            (0.0f64, 0.0f64, 0.0f64, 0.0f64),
            |(sx, sy, sxx, sxy), (x, y)| (sx + x, sy + y, sxx + x * x, sxy + x * y),
        );
        let denom = n * sum_xx - sum_x * sum_x;
        let (trend_m, trend_b) = if denom.abs() > f64::EPSILON {
            let m = (n * sum_xy - sum_x * sum_y) / denom;
            let b = (sum_y - m * sum_x) / n;
            (m, b)
        } else {
            (f64::NAN, f64::NAN)
        };

        // ── Build rows ────────────────────────────────────────────────────
        let mut rows: Vec<SaleRow> = Vec::with_capacity(flat.len());
        for (ts, group_index, price, _qty) in flat {
            let trend_y = if trend_m.is_nan() || trend_b.is_nan() {
                f64::NAN
            } else {
                trend_b + trend_m * ts.timestamp() as f64
            };
            rows.push(SaleRow {
                ts,
                price,
                group_index,
                market_average_y: market_average_val,
                trend_y,
            });
        }
        (series_names, rows)
    });

    // Bucketed quantity data. Per-sale bars produce zero-width slivers when sales
    // cluster, so we aggregate into time buckets sized to the visible window.
    let quantity_data = Memo::new(move |_| {
        let data = filtered.get();
        if data.is_empty() {
            return (Vec::<QuantityBucket>::new(), 86_400i64);
        }
        let min_ts = data
            .iter()
            .map(|s| s.sold_date.and_utc().timestamp())
            .min()
            .unwrap_or(0);
        let max_ts = data
            .iter()
            .map(|s| s.sold_date.and_utc().timestamp())
            .max()
            .unwrap_or(0);
        let span_days = ((max_ts - min_ts) / 86_400).max(0);
        let bucket_secs = quantity_bucket_seconds(days_range.get(), span_days);
        (bucket_quantities(&data, bucket_secs), bucket_secs)
    });

    // X-axis tick periods sized to the visible window.
    let x_periods = Memo::new(move |_| {
        let data = filtered.get();
        let span_days = if data.is_empty() {
            0
        } else {
            let min_ts = data
                .iter()
                .map(|s| s.sold_date.and_utc().timestamp())
                .min()
                .unwrap_or(0);
            let max_ts = data
                .iter()
                .map(|s| s.sold_date.and_utc().timestamp())
                .max()
                .unwrap_or(0);
            (max_ts - min_ts) / 86_400
        };
        x_axis_periods(days_range.get(), span_days)
    });

    view! {
        <div class="flex flex-col gap-3">
            <StatsStrip stats=stats.into() />
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
                    t_string!(i18n, chart_aria_label)
                        .to_string()
                        .replace("{n}", &n.to_string())
                        .replace("{from}", "")
                        .replace("{to}", "")
                }
                class="price-history-chart w-full overflow-visible"
            >
                {move || {
                    let (series_names, rows) = chart_data.get();
                    if rows.is_empty() {
                        let msg = t_string!(i18n, chart_no_sales_in_window).to_string();
                        view! {
                            <div class="flex items-center justify-center w-full h-full text-[color:var(--color-text)]/60 text-sm">
                                {msg}
                            </div>
                        }
                            .into_any()
                    } else {
                        let marker_css = rows
                            .iter()
                            .enumerate()
                            .map(|(index, row)| {
                                let colour = CATEGORY_PALETTE[row.group_index % CATEGORY_PALETTE.len()];
                                format!(
                                    ".price-history-chart ._chartistry_line_markers circle:nth-of-type({}){{fill:{colour};}}",
                                    index + 1
                                )
                            })
                            .collect::<String>();

                        let sales_colour: Colour = "#60a5fa"
                            .parse()
                            .unwrap_or(Colour::from_rgb(96, 165, 250));
                        let mut reactive_series = Series::new(|row: &SaleRow| row.ts).line(
                            Line::new(|row: &SaleRow| row.price)
                                .with_name(t_string!(i18n, chart_legend_sales).to_string())
                                .with_width(1.0)
                                .with_colour(sales_colour)
                                .with_interpolation(Interpolation::Linear)
                                .with_marker(
                                    Marker::from_shape(MarkerShape::Circle)
                                        .with_colour(sales_colour)
                                        .with_border(
                                            "#dbeafe"
                                                .parse()
                                                .unwrap_or(Colour::from_rgb(219, 234, 254)),
                                        )
                                        .with_border_width(0.8)
                                        .with_scale(1.1),
                                ),
                        );

                        // ── Overlay lines ─────────────────────────────────────────────
                        if show_market_average.get() {
                            reactive_series = reactive_series.line(
                                Line::new(|r: &SaleRow| r.market_average_y)
                                    .with_name(t_string!(i18n, chart_legend_market_avg).to_string())
                                    .with_width(2.0)
                                    .with_interpolation(Interpolation::Linear)
                                    .with_colour(
                                        "#facc15"
                                            .parse()
                                            .unwrap_or(Colour::from_rgb(250, 204, 21)),
                                    ),
                            );
                        }
                        if show_trend.get() {
                            reactive_series = reactive_series.line(
                                Line::new(|r: &SaleRow| r.trend_y)
                                    .with_name(t_string!(i18n, chart_legend_trend).to_string())
                                    .with_width(1.5)
                                    .with_interpolation(Interpolation::Linear)
                                    .with_colour(
                                        "#94a3b8"
                                            .parse()
                                            .unwrap_or(Colour::from_rgb(148, 163, 184)),
                                    ),
                            );
                        }

                        let aspect_ratio = AspectRatio::from_inner_ratio(800.0, 450.0);
                        // X-axis: scale tick periods to the visible window so 7d shows hours/days,
                        // 30d shows days, etc. The tooltip uses long format for unambiguous dates.
                        let periods = x_periods.get();
                        let x_axis_labels = TickLabels::from_generator(
                            Timestamps::<chrono::Utc>::from_periods(periods.as_slice()),
                        );
                        let x_tooltip_labels = TickLabels::from_generator(
                            Timestamps::<chrono::Utc>::default().with_long_format(),
                        );
                        let y_tooltip = TickLabels::aligned_floats()
                            .with_format(|v: &f64, _state| short_number(*v as i32));
                        let tooltip = Tooltip::new(
                            TooltipPlacement::LeftCursor,
                            x_tooltip_labels,
                            y_tooltip,
                        )
                        .skip_missing(true)
                        .with_cursor_distance(14.0);
                        // Y-axis formatter: reuse short_number style for f64 prices.
                        let y_labels = TickLabels::aligned_floats()
                            .with_min_chars(7)
                            .with_format(|v: &f64, _state| short_number(*v as i32));
                        let grid_colour: Colour = "#3f3a4a"
                            .parse()
                            .unwrap_or(Colour::from_rgb(63, 58, 74));
                        view! {
                            <div class="flex flex-col gap-2">
                                <style>{marker_css}</style>
                                <Chart
                                    aspect_ratio=aspect_ratio
                                    font_height=12.0
                                    font_width=7.0
                                    series=reactive_series
                                    data=rows.clone()
                                    bottom=vec![x_axis_labels.into_edge()]
                                    left=vec![y_labels.into_edge()]
                                    inner=vec![
                                        XGridLine::default().with_colour(grid_colour).into_inner(),
                                        YGridLine::default().with_colour(grid_colour).into_inner(),
                                    ]
                                    tooltip=tooltip
                                />
                                {show_quantity.get().then(|| {
                                    let (buckets, _bucket_secs) = quantity_data.get();
                                    if buckets.is_empty() {
                                        return ().into_any();
                                    }
                                    let q_periods = x_periods.get();
                                    let quantity_colour: Colour = "#22c55e"
                                        .parse()
                                        .unwrap_or(Colour::from_rgb(34, 197, 94));
                                    let quantity_series = Series::new(|b: &QuantityBucket| b.ts)
                                        .bar(
                                            Bar::new(|b: &QuantityBucket| b.quantity)
                                                .with_name(t_string!(i18n, chart_legend_quantity).to_string())
                                                .with_colour(quantity_colour)
                                                .with_placement(BarPlacement::Zero)
                                                .with_gap(0.1),
                                        );
                                    let quantity_labels = TickLabels::aligned_floats()
                                        .with_min_chars(3)
                                        .with_format(|v: &f64, _state| (*v as i32).to_string());
                                    let quantity_x_axis = TickLabels::from_generator(
                                        Timestamps::<chrono::Utc>::from_periods(q_periods.as_slice()),
                                    );
                                    let quantity_x_tooltip = TickLabels::from_generator(
                                        Timestamps::<chrono::Utc>::default().with_long_format(),
                                    );
                                    let quantity_tooltip = Tooltip::new(
                                        TooltipPlacement::LeftCursor,
                                        quantity_x_tooltip,
                                        quantity_labels.clone(),
                                    )
                                    .with_cursor_distance(14.0);
                                    view! {
                                        <div class="border-t border-[color:var(--color-outline)]/70 pt-2 mt-2">
                                            <Chart
                                                aspect_ratio=AspectRatio::from_inner_ratio(800.0, 120.0)
                                                font_height=10.0
                                                font_width=6.0
                                                series=quantity_series
                                                data=buckets
                                                bottom=vec![quantity_x_axis.into_edge()]
                                                left=vec![quantity_labels.into_edge()]
                                                inner=vec![
                                                    XGridLine::default().with_colour(grid_colour).into_inner(),
                                                    YGridLine::default().with_colour(grid_colour).into_inner(),
                                                ]
                                                tooltip=quantity_tooltip
                                            />
                                        </div>
                                    }.into_any()
                                })}
                                <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-[color:var(--color-text-muted)]">
                                    {series_names.iter().enumerate().map(|(index, name)| {
                                        let colour = CATEGORY_PALETTE[index % CATEGORY_PALETTE.len()];
                                        view! {
                                            <span class="inline-flex items-center gap-1.5">
                                                <span
                                                    class="h-2.5 w-2.5 rounded-full ring-1 ring-blue-100/70"
                                                    style:background-color=colour
                                                ></span>
                                                {name.clone()}
                                            </span>
                                        }
                                    }).collect_view()}
                                    {show_market_average.get().then(|| view! {
                                        <span class="inline-flex items-center gap-1.5">
                                            <span class="h-0.5 w-5 bg-[#facc15]"></span>
                                            {t!(i18n, chart_legend_market_avg)}
                                        </span>
                                    })}
                                    {show_trend.get().then(|| view! {
                                        <span class="inline-flex items-center gap-1.5">
                                            <span class="h-0.5 w-5 bg-[#94a3b8]"></span>
                                            {t!(i18n, chart_legend_trend)}
                                        </span>
                                    })}
                                    {show_quantity.get().then(|| view! {
                                        <span class="inline-flex items-center gap-1.5">
                                            <span class="h-2.5 w-3 rounded-sm bg-[#22c55e]"></span>
                                            {t!(i18n, chart_legend_quantity)}
                                        </span>
                                    })}
                                </div>
                            </div>
                        }
                            .into_any()
                    }
                }}
            </div>
        </div>
    }
    .into_any()
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
    #[prop(into)] options: Signal<Vec<ColorBy>>,
    #[prop(into)] selected: Signal<ColorBy>,
    set_selected: WriteSignal<ColorBy>,
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
                                    ColorBy::Region => t_string!(i18n, chart_color_region).to_string(),
                                    ColorBy::Datacenter => t_string!(i18n, chart_color_datacenter).to_string(),
                                    ColorBy::World => t_string!(i18n, chart_color_world).to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use ultros_api_types::world::{Datacenter, Region, World, WorldData};
    use ultros_api_types::world_helper::WorldHelper;

    fn test_world_helper() -> WorldHelper {
        // Two regions; region 1 has two DCs; DC 10 has two worlds, DC 11 has one, DC 20 (region 2) has one.
        let world_data = WorldData {
            regions: vec![
                Region {
                    id: 1,
                    name: "North-America".into(),
                    datacenters: vec![
                        Datacenter {
                            id: 10,
                            name: "Aether".into(),
                            region_id: 1,
                            worlds: vec![
                                World {
                                    id: 100,
                                    name: "Gilgamesh".into(),
                                    datacenter_id: 10,
                                },
                                World {
                                    id: 101,
                                    name: "Adamantoise".into(),
                                    datacenter_id: 10,
                                },
                            ],
                        },
                        Datacenter {
                            id: 11,
                            name: "Crystal".into(),
                            region_id: 1,
                            worlds: vec![World {
                                id: 102,
                                name: "Balmung".into(),
                                datacenter_id: 11,
                            }],
                        },
                    ],
                },
                Region {
                    id: 2,
                    name: "Europe".into(),
                    datacenters: vec![Datacenter {
                        id: 20,
                        name: "Light".into(),
                        region_id: 2,
                        worlds: vec![World {
                            id: 200,
                            name: "Phoenix".into(),
                            datacenter_id: 20,
                        }],
                    }],
                },
            ],
        };
        WorldHelper::from(world_data)
    }

    fn sale(world_id: i32, price: i32, qty: i32, ts: i64) -> SaleHistory {
        SaleHistory {
            id: 0,
            quantity: qty,
            price_per_item: price,
            buying_character_id: 0,
            hq: false,
            sold_item_id: 1,
            sold_date: chrono::Utc.timestamp_opt(ts, 0).unwrap().naive_utc(),
            world_id,
            buyer_name: None,
        }
    }

    #[test]
    fn grouping_collapses_to_world_when_one_dc() {
        let helper = test_world_helper();
        // Both sales are on worlds inside Aether (DC 10) → one DC → group by world.
        let sales = vec![sale(100, 1000, 1, 0), sale(101, 1100, 1, 1)];
        let series = group_sales_by_level(&helper, &sales, ColorBy::World);
        let names: Vec<_> = series.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"Gilgamesh"));
        assert!(names.contains(&"Adamantoise"));
    }

    #[test]
    fn grouping_collapses_to_dc_when_one_region() {
        let helper = test_world_helper();
        // Two DCs (Aether, Crystal) both in NA → one region → group by DC.
        let sales = vec![sale(100, 1000, 1, 0), sale(102, 1100, 1, 1)];
        let series = group_sales_by_level(&helper, &sales, ColorBy::Datacenter);
        let names: Vec<_> = series.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"Aether"));
        assert!(names.contains(&"Crystal"));
    }

    #[test]
    fn grouping_collapses_to_region_when_multiple_regions() {
        let helper = test_world_helper();
        // Worlds from two regions → group by region.
        let sales = vec![sale(100, 1000, 1, 0), sale(200, 1100, 1, 1)];
        let series = group_sales_by_level(&helper, &sales, ColorBy::Region);
        let names: Vec<_> = series.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"North-America"));
        assert!(names.contains(&"Europe"));
    }

    #[test]
    fn vwap_weights_by_quantity() {
        let prices = vec![(100, 1), (200, 9)];
        assert_eq!(vwap(&prices), Some(190));
    }

    #[test]
    fn vwap_returns_none_for_empty() {
        assert_eq!(vwap(&[]), None);
    }

    #[test]
    fn vwap_returns_none_when_total_qty_zero() {
        let prices = vec![(100, 0), (200, 0)];
        assert_eq!(vwap(&prices), None);
    }

    #[test]
    fn median_of_odd_count() {
        let prices = vec![300, 100, 200];
        assert_eq!(median(&prices), Some(200));
    }

    #[test]
    fn median_of_even_count_averages_middle_two() {
        let prices = vec![400, 100, 300, 200];
        assert_eq!(median(&prices), Some(250));
    }

    #[test]
    fn median_returns_none_for_empty() {
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn iqr_band_returns_none_for_small_samples() {
        let prices: Vec<i32> = (0..9).collect();
        assert_eq!(iqr_band(&prices), None);
    }

    #[test]
    fn iqr_band_widens_with_25x_multiplier() {
        let prices: Vec<i32> = (0..20).collect();
        assert_eq!(iqr_band(&prices), Some((-20, 40)));
    }

    #[test]
    fn bucket_seconds_scales_with_window() {
        // 7d window → 6h buckets (~28 bars max)
        assert_eq!(quantity_bucket_seconds(7, 7), 6 * 3_600);
        // 30d window → daily
        assert_eq!(quantity_bucket_seconds(30, 30), 86_400);
        // 90d window → still daily
        assert_eq!(quantity_bucket_seconds(90, 90), 86_400);
        // "All" with multi-year span → weekly
        assert_eq!(quantity_bucket_seconds(0, 365), 86_400 * 7);
    }

    #[test]
    fn bucket_quantities_sums_within_same_day_bucket() {
        // Three sales of qty 5 within an hour all land in one daily bucket.
        let day_start_ts = 1_700_000_000i64 - (1_700_000_000 % 86_400);
        let sales = vec![
            sale(100, 1000, 5, day_start_ts + 60),
            sale(100, 1100, 5, day_start_ts + 600),
            sale(100, 1050, 5, day_start_ts + 3_500),
        ];
        let buckets = bucket_quantities(&sales, 86_400);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].quantity, 15.0);
    }

    #[test]
    fn bucket_quantities_separates_across_buckets() {
        let sales = vec![sale(100, 1000, 3, 0), sale(100, 1000, 7, 86_400 + 60)];
        let buckets = bucket_quantities(&sales, 86_400);
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].quantity, 3.0);
        assert_eq!(buckets[1].quantity, 7.0);
    }

    #[test]
    fn x_axis_periods_for_short_window_includes_day_or_hour() {
        let periods = x_axis_periods(7, 7);
        assert!(periods.contains(&Period::Day) || periods.contains(&Period::Hour));
    }

    #[test]
    fn x_axis_periods_for_all_falls_back_to_month_year() {
        let periods = x_axis_periods(0, 500);
        assert!(periods.contains(&Period::Month) || periods.contains(&Period::Year));
    }
}
