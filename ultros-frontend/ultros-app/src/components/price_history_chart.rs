use std::collections::HashSet;

use leptos::prelude::*;
use leptos_chartistry::*;
use ultros_api_types::SaleHistory;
use ultros_api_types::world_helper::AnySelector;

use crate::components::toggle::Toggle;
use crate::global_state::LocalWorldData;
use crate::i18n::{t_string, use_i18n};

type SeriesPoints = Vec<(chrono::DateTime<chrono::Local>, i32, i32)>;

/// Roll sales up to world / DC / region depending on how many distinct regions
/// and DCs are represented. Mirrors the rule in `ultros-charts::map_sale_history_to_line`.
fn group_sales_by_locale(
    helper: &ultros_api_types::world_helper::WorldHelper,
    sales: &[SaleHistory],
) -> Vec<(String, SeriesPoints)> {
    use itertools::Itertools;

    let world_ids: HashSet<AnySelector> = sales
        .iter()
        .map(|s| AnySelector::World(s.world_id))
        .collect();
    let datacenters: HashSet<AnySelector> = world_ids
        .iter()
        .flat_map(|w| {
            helper
                .lookup_selector(*w)
                .and_then(|r| r.as_world())
                .map(|w| AnySelector::Datacenter(w.datacenter_id))
        })
        .collect();
    let regions: HashSet<AnySelector> = datacenters
        .iter()
        .flat_map(|dc| {
            helper
                .lookup_selector(*dc)
                .and_then(|r| r.as_datacenter())
                .map(|dc| AnySelector::Region(dc.region_id))
        })
        .collect();
    let selectors = if datacenters.len() == 1 {
        world_ids
    } else if regions.len() == 1 {
        datacenters
    } else {
        regions
    };
    selectors
        .into_iter()
        .filter_map(|sel| {
            let result = helper.lookup_selector(sel)?;
            let name = result.get_name().to_string();
            let points: SeriesPoints = sales
                .iter()
                .filter(|s| {
                    helper
                        .lookup_selector(AnySelector::World(s.world_id))
                        .map(|w| w.is_in(&result))
                        .unwrap_or_default()
                })
                .filter_map(|s| {
                    Some((
                        s.sold_date.and_local_timezone(chrono::Local).single()?,
                        s.price_per_item,
                        s.quantity,
                    ))
                })
                .collect();
            Some((name, points))
        })
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TimeRange {
    Last24h,
    Last7d,
    Last30d,
    All,
}

impl TimeRange {
    fn label(self) -> &'static str {
        match self {
            TimeRange::Last24h => "24h",
            TimeRange::Last7d => "7d",
            TimeRange::Last30d => "30d",
            TimeRange::All => "All",
        }
    }

    fn cutoff(self, now: chrono::NaiveDateTime) -> Option<chrono::NaiveDateTime> {
        let delta = match self {
            TimeRange::Last24h => chrono::Duration::hours(24),
            TimeRange::Last7d => chrono::Duration::days(7),
            TimeRange::Last30d => chrono::Duration::days(30),
            TimeRange::All => return None,
        };
        Some(now - delta)
    }
}

/// Filter sales whose `sold_date` is on-or-after the range cutoff.
/// `now` is parameterized for tests.
fn filter_by_range(
    sales: &[SaleHistory],
    range: TimeRange,
    now: chrono::NaiveDateTime,
) -> Vec<SaleHistory> {
    match range.cutoff(now) {
        Some(cutoff) => sales
            .iter()
            .filter(|s| s.sold_date >= cutoff)
            .cloned()
            .collect(),
        None => sales.to_vec(),
    }
}

/// Computed statistics for the stats strip.
#[derive(Clone, Debug, PartialEq)]
struct ChartStats {
    n: usize,
    vwap_val: Option<i32>,
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
    /// Price per item (f64 for chartistry). One column per series; NAN if
    /// this row does not belong to that series.
    prices: Vec<f64>,
}

// ── Sub-components ────────────────────────────────────────────────────────────

#[component]
fn RangeChips(current: Signal<TimeRange>, set_current: SignalSetter<TimeRange>) -> impl IntoView {
    let ranges = [
        TimeRange::Last24h,
        TimeRange::Last7d,
        TimeRange::Last30d,
        TimeRange::All,
    ];
    view! {
        <div class="flex items-center gap-1" role="group" aria-label="Time range">
            {ranges
                .into_iter()
                .map(|r| {
                    view! {
                        <button
                            type="button"
                            aria-pressed=move || (current.get() == r).to_string()
                            class=move || {
                                let base = "px-3 py-1 rounded-full text-sm transition-colors duration-150 ";
                                if current.get() == r {
                                    format!(
                                        "{}bg-brand-500/25 font-medium text-[color:var(--color-text)]",
                                        base,
                                    )
                                } else {
                                    format!(
                                        "{}hover:bg-brand-500/10 text-[color:var(--color-text)]/70",
                                        base,
                                    )
                                }
                            }
                            on:click=move |_| set_current.set(r)
                        >
                            {r.label()}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

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
                    let vwap_label = t_string!(i18n, chart_stat_vwap).to_string();
                    let median_label = t_string!(i18n, chart_stat_median).to_string();
                    let min_label = t_string!(i18n, chart_stat_min).to_string();
                    let max_label = t_string!(i18n, chart_stat_max).to_string();
                    view! {
                        <div class="flex flex-wrap gap-x-4 gap-y-1 text-sm tabular-nums text-[color:var(--color-text)]/70 mb-3">
                            <span>{n_label}</span>
                            {s
                                .vwap_val
                                .map(|v| {
                                    view! {
                                        <span>
                                            {vwap_label} " " {short_number(v)}
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
pub fn PriceHistoryChart(#[prop(into)] sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
    // Implementation uses leptos-chartistry 0.2 for a multi-series scatter chart.
    // Scatter is achieved by setting line width to 0.0 and adding Circle markers.
    // Each series is a column of f64 prices in a flat Vec<SaleRow>, with NAN for
    // rows that don't belong to that series.

    let local_world_data = use_context::<LocalWorldData>().unwrap();
    let helper = local_world_data.0.unwrap();
    let i18n = use_i18n();

    let (range, set_range) = signal(TimeRange::All);
    let (filter_outliers, set_filter_outliers) = signal(true);

    // Step 1: time-range filter
    let range_filtered = Memo::new(move |_| {
        let now = chrono::Utc::now().naive_utc();
        filter_by_range(&sales.get(), range.get(), now)
    });

    // Step 2: optional IQR outlier filter on top of the time-range filter
    let filtered = Memo::new(move |_| {
        let base = range_filtered.get();
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

    // Step 3: stats
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
            vwap_val: vwap(&pq),
            median_val: median(&prices),
            min_val,
            max_val,
        })
    });

    // Step 4: group by locale and build flat row vec for chartistry
    // We produce a derived signal so chartistry's `data` prop stays reactive.
    // Series names come from group_sales_by_locale; order is stable (sorted).
    let helper_clone = helper.clone();
    let chart_data = Memo::new(move |_| {
        let data = filtered.get();
        let groups = group_sales_by_locale(&helper_clone, &data);
        // Build flat rows: one row per sale, prices indexed by series slot.
        // series_names drives the number of price columns.
        let n_series = groups.len();
        if n_series == 0 || data.is_empty() {
            return (vec![], vec![]);
        }
        // series_names: stable order (already sorted by group_sales_by_locale)
        let series_names: Vec<String> = groups.iter().map(|(n, _)| n.clone()).collect();

        // Flatten all points into (ts, series_idx, price)
        let mut flat: Vec<(chrono::DateTime<chrono::Utc>, usize, f64)> = groups
            .iter()
            .enumerate()
            .flat_map(|(idx, (_, points))| {
                points.iter().map(move |(dt, price, _qty)| {
                    let utc = dt.with_timezone(&chrono::Utc);
                    (utc, idx, *price as f64)
                })
            })
            .collect();
        // Sort by timestamp so chartistry's line renderer doesn't cross itself
        flat.sort_by(|a, b| a.0.cmp(&b.0));

        let mut rows: Vec<SaleRow> = Vec::with_capacity(flat.len());
        for (ts, series_idx, price) in flat {
            let mut prices = vec![f64::NAN; n_series];
            prices[series_idx] = price;
            rows.push(SaleRow { ts, prices });
        }
        (series_names, rows)
    });

    // Stable series objects: we build them once but read series_names reactively
    // via the chart_data signal. Chartistry requires fixed series count at
    // component build time, so we cap at a reasonable max and let extras be NAN.
    // In practice FFXIV has at most ~8 worlds per DC, so 12 is safe.
    const MAX_SERIES: usize = 12;

    // Build the PALETTE of colors for up to MAX_SERIES series.
    // These are Tailwind brand/chart-friendly hex colors.
    let palette = [
        "#60a5fa", // blue-400
        "#f97316", // orange-500
        "#34d399", // emerald-400
        "#a78bfa", // violet-400
        "#fb7185", // rose-400
        "#facc15", // yellow-400
        "#22d3ee", // cyan-400
        "#c084fc", // purple-400
        "#4ade80", // green-400
        "#f472b6", // pink-400
        "#94a3b8", // slate-400
        "#fdba74", // orange-300
    ];

    // The Series and AspectRatio are created fresh inside the reactive `move ||`
    // closure below so they don't need to be `Copy`. The palette array is `Copy`
    // (array of &'static str) and is captured by value.

    view! {
        <div class="panel p-4 md:p-6 text-[color:var(--color-text)]">
            // Header row: title on left, controls on right
            <div class="flex flex-wrap items-center justify-between gap-3 mb-3">
                <h3 class="text-lg font-semibold m-0">
                    {move || t_string!(i18n, sale_history).to_string()}
                </h3>
                <div class="flex flex-wrap items-center gap-3">
                    <RangeChips current=range.into() set_current=set_range.into() />
                    <Toggle
                        checked=Signal::from(filter_outliers)
                        set_checked=SignalSetter::map(move |v| set_filter_outliers.set(v))
                        checked_label=t_string!(i18n, filter_outliers_enabled)
                        unchecked_label=t_string!(i18n, filter_outliers_disabled)
                    />
                </div>
            </div>

            // Stats strip
            <StatsStrip stats=stats.into() />

            // Chart body
            <div class="w-full aspect-[16/9] max-h-[520px] overflow-hidden">
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
                        // Build reactive legend labels from series_names
                        // We only show series that have at least one real data point.
                        // Because Line entries are pre-built with fixed indices,
                        // the legend names are set on the Line; we override name
                        // reactively via a wrapper series below.
                        //
                        // Since chartistry's Series is not reactive post-construction,
                        // we build a new Series each render — this is acceptable
                        // because the chart re-mounts when rows changes.
                        let n_active = series_names.len().min(MAX_SERIES);
                        let mut reactive_series =
                            Series::new(|row: &SaleRow| row.ts);
                        for i in 0..n_active {
                            let colour_hex = palette[i % palette.len()];
                            let colour: Colour = colour_hex
                                .parse()
                                .unwrap_or(Colour::from_rgb(96, 165, 250));
                            let name = series_names
                                .get(i)
                                .cloned()
                                .unwrap_or_default();
                            let line = Line::new(move |row: &SaleRow| {
                                if i < row.prices.len() {
                                    row.prices[i]
                                } else {
                                    f64::NAN
                                }
                            })
                            .with_name(name)
                            .with_width(0.0)
                            .with_marker(
                                Marker::from_shape(MarkerShape::Circle)
                                    .with_colour(colour)
                                    .with_scale(4.0),
                            );
                            reactive_series = reactive_series.line(line);
                        }

                        let aspect_ratio = AspectRatio::from_inner_ratio(800.0, 450.0);
                        let tooltip = Tooltip::left_cursor().skip_missing(true);
                        // Y-axis formatter: reuse short_number style for f64 prices.
                        let y_labels = TickLabels::aligned_floats()
                            .with_format(|v: &f64, _state| short_number(*v as i32));
                        view! {
                            <Chart
                                aspect_ratio=aspect_ratio
                                series=reactive_series
                                data=rows
                                bottom=vec![TickLabels::timestamps().into_edge()]
                                left=vec![y_labels.into_edge()]
                                inner=vec![
                                    XGridLine::default().into_inner(),
                                    YGridLine::default().into_inner(),
                                ]
                                tooltip=tooltip
                                right=vec![Legend::end().into_edge()]
                            />
                        }
                            .into_any()
                    }
                }}
            </div>
        </div>
    }
    .into_any()
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
        let series = group_sales_by_locale(&helper, &sales);
        let names: Vec<_> = series.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"Gilgamesh"));
        assert!(names.contains(&"Adamantoise"));
    }

    #[test]
    fn grouping_collapses_to_dc_when_one_region() {
        let helper = test_world_helper();
        // Two DCs (Aether, Crystal) both in NA → one region → group by DC.
        let sales = vec![sale(100, 1000, 1, 0), sale(102, 1100, 1, 1)];
        let series = group_sales_by_locale(&helper, &sales);
        let names: Vec<_> = series.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"Aether"));
        assert!(names.contains(&"Crystal"));
    }

    #[test]
    fn grouping_collapses_to_region_when_multiple_regions() {
        let helper = test_world_helper();
        // Worlds from two regions → group by region.
        let sales = vec![sale(100, 1000, 1, 0), sale(200, 1100, 1, 1)];
        let series = group_sales_by_locale(&helper, &sales);
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
    fn time_range_all_keeps_everything() {
        let now = chrono::Utc::now().naive_utc();
        let sales = vec![
            sale(100, 1000, 1, now.and_utc().timestamp() - 60 * 60 * 24 * 60),
            sale(100, 2000, 1, now.and_utc().timestamp()),
        ];
        let filtered = filter_by_range(&sales, TimeRange::All, now);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn time_range_24h_filters_older_sales() {
        let now = chrono::Utc::now().naive_utc();
        let sales = vec![
            sale(100, 1000, 1, now.and_utc().timestamp() - 60 * 60 * 25),
            sale(100, 2000, 1, now.and_utc().timestamp() - 60 * 60),
        ];
        let filtered = filter_by_range(&sales, TimeRange::Last24h, now);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].price_per_item, 2000);
    }

    #[test]
    fn time_range_7d_filters_older_sales() {
        let now = chrono::Utc::now().naive_utc();
        let sales = vec![
            sale(100, 1000, 1, now.and_utc().timestamp() - 60 * 60 * 24 * 8),
            sale(100, 2000, 1, now.and_utc().timestamp() - 60 * 60 * 24 * 3),
        ];
        let filtered = filter_by_range(&sales, TimeRange::Last7d, now);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].price_per_item, 2000);
    }
}
