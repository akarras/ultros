use std::ops::RangeInclusive;

use super::{datacenter_name::*, gil::*, relative_time::*, world_name::*};
use crate::components::icon::Icon;
use chrono::{Duration, NaiveDateTime, TimeDelta, Utc};
use icondata as i;
use leptos::prelude::*;
use linregress::{FormulaRegressionBuilder, RegressionDataBuilder};
use log::{error, info};
use ultros_api_types::{SaleHistory, world_helper::AnySelector};

use crate::i18n::*;

#[component]
pub fn SaleHistoryTable(sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
    let i18n = use_i18n();
    let (show_more, set_show_more) = signal(false);
    // Optimization: Avoid cloning the entire sales vector when we only need a slice.
    // Using `sales.with` allows us to inspect the vector without cloning it.
    // If show_more is false, we only clone the first 10 items.
    let sale_history = Memo::new(move |_| {
        sales.with(|sales| {
            if show_more() {
                sales.clone()
            } else {
                sales.iter().take(10).cloned().collect()
            }
        })
    });
    view! {
        <div class="overflow-x-auto max-h-[60vh] overflow-y-auto rounded-lg">
            <table class="w-full text-sm min-w-[720px]">
            <thead class="text-xs uppercase">
                <tr>
                    <th scope="col">{t!(i18n, sale_history_col_hq)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_price)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_qty)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_total)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_purchaser)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_world)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_datacenter)}</th>
                    <th scope="col">{t!(i18n, sale_history_col_time_sold)}</th>
                </tr>
            </thead>
            <tbody class="divide-y divide-[color:var(--color-outline)]">
                <For
                    each=sale_history
                    key=move |sale| sale.sold_date.and_utc().timestamp()
                    children=move |sale| {
                        let total = sale.price_per_item * sale.quantity;
                        view! {
                            <tr>
                                <td>
                                    {sale
                                        .hq
                                        .then(|| {
                                            view! {
                                                <span class="sr-only">{t!(i18n, sale_history_high_quality_sr)}</span>
                                                <Icon icon=i::BsCheck aria_hidden=true />
                                            }
                                            .into_view()
                                        })}
                                </td>
                                <td>
                                    <Gil amount=sale.price_per_item />
                                </td>
                                <td>{sale.quantity}</td>
                                <td>
                                    <Gil amount=total />
                                </td>
                                <td>{sale.buyer_name}</td>
                                <td>
                                    <WorldName id=AnySelector::World(sale.world_id) />
                                </td>
                                <td>
                                    <DatacenterName world_id=sale.world_id />
                                </td>
                                <td>
                                    <RelativeToNow timestamp=sale.sold_date />
                                </td>
                            </tr>
                        }
                    }
                />

                {move || {
                    (!show_more() && sales.with(|sales| sales.len() > 10))
                        .then(|| {
                            view! {
                                <tr>
                                    <td colspan="8">
                                        <button
                                            class="btn btn-primary w-full"
                                            on:click=move |_| set_show_more(true)
                                        >
                                            {t!(i18n, sale_history_show_more)}
                                        </button>
                                    </td>
                                </tr>
                            }
                        })
                }}

            </tbody>
        </table>
        </div>
    }
}

#[derive(Clone, PartialEq, PartialOrd, Default)]
struct SalesWindow {
    /// Total amount of gil sold in this window
    total_gil: u64,
    average_unit_price: f64,
    max_unit_price: i32,
    median_unit_price: i32,
    min_unit_price: i32,
    median_stack_size: i32,
    hq_percent: i32,
    guessed_next_sale_price: f64,
    p_value: f64,
    time_between_sales: Duration,
}

impl SalesWindow {
    fn try_new(date_range: RangeInclusive<NaiveDateTime>, sales: &[SaleHistory]) -> Option<Self> {
        let sales = find_date_range(date_range.clone(), sales)?;
        let count = sales.len();
        if count == 0 {
            return None;
        }

        let mut total_gil = 0u64;
        let mut hq_count = 0usize;
        let mut total_sale_price = 0i64;

        let mut unit_prices = Vec::with_capacity(count);
        let mut stack_sizes = Vec::with_capacity(count);
        let mut dates = Vec::with_capacity(count);
        let mut unit_prices_f64 = Vec::with_capacity(count);

        let start_timestamp = date_range.start().and_utc().timestamp();

        for sale in sales {
            let price = sale.price_per_item;
            let qty = sale.quantity;
            let date_val = (sale.sold_date.and_utc().timestamp() - start_timestamp) as f64;

            total_gil += price as u64 * qty as u64;
            total_sale_price += price as i64;
            if sale.hq {
                hq_count += 1;
            }

            unit_prices.push(price);
            stack_sizes.push(qty);
            dates.push(date_val);
            unit_prices_f64.push(price as f64);
        }

        // ⚡ Bolt: Optimization: Use select_nth_unstable instead of sort_unstable for median calculation.
        // This reduces time complexity from O(N log N) to O(N).
        let (_, &mut median_unit_price, _) = unit_prices.select_nth_unstable(count / 2);
        let avg_sale_price = total_sale_price as f64 / count as f64;

        let (_, &mut median_stack_size, _) = stack_sizes.select_nth_unstable(count / 2);

        let duration = *date_range.end() - *date_range.start();
        let avg_duration = duration / count as i32;

        let (guessed_next_sale_price, p_value) = (|| {
            let data = RegressionDataBuilder::new()
                .build_from([("X", unit_prices_f64), ("Y", dates)])
                .inspect_err(|e| {
                    error!("{e:?}");
                })
                .ok()?;
            let model = FormulaRegressionBuilder::new()
                .data(&data)
                .data_columns("X", ["Y"])
                .fit()
                .inspect_err(|e| {
                    error!("{e:?}");
                })
                .ok()?;

            let next_sale_time = [(
                "Y",
                vec![
                    ((*date_range.end() + avg_duration).and_utc().timestamp() - start_timestamp)
                        as f64,
                ],
            )];
            let next = model.predict(next_sale_time).ok()?[0];
            // let paremeters = model.parameters()[0];
            let p_value = model.p_values()[0];
            info!(
                "{:?} {:?}",
                model.iter_parameter_pairs().collect::<Vec<_>>(),
                model.iter_p_value_pairs().collect::<Vec<_>>()
            );
            Some((next, p_value))
        })()
        .unwrap_or((avg_sale_price, 1.0));

        Some(Self {
            total_gil,
            average_unit_price: avg_sale_price,
            max_unit_price: *unit_prices.last()?,
            min_unit_price: *unit_prices.first()?,
            median_stack_size,
            hq_percent: ((hq_count as f64 / count as f64) * 100.0).round() as i32,
            guessed_next_sale_price,
            time_between_sales: avg_duration,
            median_unit_price,
            p_value,
        })
    }
}

/// The SalesSummaryData should provide generic market analytics
#[derive(PartialOrd, PartialEq)]
struct SalesSummaryData {
    past_day: Option<SalesWindow>,
    month: Option<SalesWindow>,
}

fn find_date_range(
    date_range: RangeInclusive<NaiveDateTime>,
    sales: &[SaleHistory],
) -> Option<&[SaleHistory]> {
    if sales.is_empty() {
        return None;
    }

    // Optimization: Assume sales are sorted descending (newest first).
    // This allows using binary search (partition_point) for O(log N) instead of O(N).

    // Find start index: first element where date <= range.end
    // Elements before start are > range.end (too new)
    let start = sales.partition_point(|s| s.sold_date > *date_range.end());

    if start >= sales.len() {
        return None;
    }

    // Check if the start element is actually within range
    // Since we know it is <= end, we just need to check if it is >= start
    if sales[start].sold_date < *date_range.start() {
        return None;
    }

    // Find length of the slice: elements starting from `start` that are >= range.start
    // sales[start..] contains elements <= range.end.
    // We want elements where date >= range.start.
    // Since sorted descending, these elements are at the beginning of the slice.
    let length = sales[start..].partition_point(|s| s.sold_date >= *date_range.start());

    if length == 0 {
        return None;
    }

    Some(&sales[start..start + length])
}

impl SalesSummaryData {
    fn new(sale_history: &[SaleHistory]) -> Self {
        let now = Utc::now().naive_utc();
        let yesterday = now - TimeDelta::days(1);
        let day_range = yesterday..=now;
        let month_ago = now - TimeDelta::days(31);
        let month_range = month_ago..=now;
        Self {
            past_day: SalesWindow::try_new(day_range, sale_history),
            month: SalesWindow::try_new(month_range, sale_history),
        }
    }
}

#[component]
fn WindowStats(#[prop(into)] sales: Signal<SalesWindow>) -> impl IntoView {
    let i18n = use_i18n();
    // ⚡ Bolt Optimization:
    // Replaced `Memo::new(...)` with closures `Signal::derive(move || ...)` for these 9 fields.
    // Memoizing extremely cheap operations (like accessing a field or basic math)
    // adds more overhead in reactive node creation, equality checking, and memory
    // allocation than it saves. `Signal::derive` is the correct leptos type matching into-props.
    let total_gil = Signal::derive(move || sales.with(|s| s.total_gil));
    let average_unit_price =
        Signal::derive(move || sales.with(|s| s.average_unit_price.round() as i32));
    let max_unit_price = Signal::derive(move || sales.with(|s| s.max_unit_price));
    let median_unit_price = Signal::derive(move || sales.with(|s| s.median_unit_price));
    let min_unit_price = Signal::derive(move || sales.with(|s| s.min_unit_price));
    let median_stack_size = Signal::derive(move || sales.with(|s| s.median_stack_size));
    let guessed_next_sale_price =
        Signal::derive(move || sales.with(|s| s.guessed_next_sale_price.round() as i32));
    let time_between_sales = Signal::derive(move || sales.with(|s| s.time_between_sales));
    let hq_percent = Signal::derive(move || sales.with(|s| s.hq_percent));
    view! {
        <div class="grid grid-cols-2 gap-2 sm:grid-cols-3 xl:grid-cols-5">
            <div class="col-span-2 rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_12%,_transparent)] px-3 py-2 sm:col-span-1 xl:col-span-2">
                <div class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, sale_history_stat_gil_sold)}</div>
                <div class="mt-1 text-lg font-bold tabular-nums text-[color:var(--brand-fg)]">
                    <GenericGil<u64> amount=total_gil />
                </div>
            </div>
            <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--color-text)_5%,_transparent)] px-3 py-2">
                <div class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, sale_history_stat_avg_price)}</div>
                <div class="mt-1 font-semibold tabular-nums"><Gil amount=average_unit_price /></div>
            </div>
            <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--color-text)_5%,_transparent)] px-3 py-2">
                <div class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, sale_history_stat_median_price)}</div>
                <div class="mt-1 font-semibold tabular-nums"><Gil amount=median_unit_price /></div>
            </div>
            <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--color-text)_5%,_transparent)] px-3 py-2">
                <div class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, sale_history_stat_min)}</div>
                <div class="mt-1 font-semibold tabular-nums"><Gil amount=min_unit_price /></div>
            </div>
            <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--color-text)_5%,_transparent)] px-3 py-2">
                <div class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, sale_history_stat_max)}</div>
                <div class="mt-1 font-semibold tabular-nums"><Gil amount=max_unit_price /></div>
            </div>
            <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--color-text)_5%,_transparent)] px-3 py-2">
                <div class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, sale_history_stat_typical_stack)}</div>
                <div class="mt-1 font-semibold tabular-nums">{median_stack_size}</div>
            </div>
            <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--color-text)_5%,_transparent)] px-3 py-2">
                <div class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, sale_history_stat_hq_percent)}</div>
                <div class="mt-1 font-semibold tabular-nums">{move || hq_percent()} "%"</div>
            </div>
            <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] px-3 py-2">
                <div class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, sale_history_stat_next_sale)}</div>
                <div class="mt-1 font-semibold tabular-nums"><Gil amount=guessed_next_sale_price /></div>
            </div>
            <div class="col-span-2 rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_10%,_transparent)] px-3 py-2 sm:col-span-1 xl:col-span-2">
                <div class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, sale_history_stat_time_between_sales)}</div>
                <div class="mt-1 font-semibold tabular-nums">
                    {move || {
                        time_between_sales()
                            .abs()
                            .to_std()
                            .map(|d| {
                                let secs = d.as_secs();
                                let days = secs / 86_400;
                                let hours = (secs % 86_400) / 3_600;
                                let mins = (secs % 3_600) / 60;
                                let seconds = secs % 60;
                                let mut parts = Vec::new();
                                if days > 0 { parts.push(format!("{}d", days)); }
                                if hours > 0 { parts.push(format!("{}h", hours)); }
                                if mins > 0 { parts.push(format!("{}m", mins)); }
                                if seconds > 0 && parts.len() < 2 { parts.push(format!("{}s", seconds)); }
                                if parts.len() > 2 { parts.truncate(2); }
                                if parts.is_empty() { "0s".to_string() } else { parts.join(" ") }
                            })
                            .unwrap_or_default()
                    }}
                </div>
            </div>
        </div>
    }
    .into_any()
}

#[component]
pub fn SalesInsights(sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
    let i18n = use_i18n();
    // `SalesSummaryData::new` reads `Utc::now()`, which is non-deterministic
    // across SSR (server clock at render time) and CSR (client clock at
    // hydration). For items with sales right at the day/month window edge,
    // `past_day` / `month` can flip between Some and None across the two
    // renders, which in turn flips `class:hidden` on the wrapper div below.
    // Pair that with the matching deferred-cutoff in `ChartWrapper` and the
    // whole sale-history half of `/item/<world>/<id>` stays structurally
    // stable through hydration. Effect flips `hydrated` post-render (client
    // only), then the memo re-runs with the real summary data.
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| {
        hydrated.set(true);
    });
    let sales = Memo::new(move |_| {
        if hydrated.get() {
            sales.with(|sales| SalesSummaryData::new(sales))
        } else {
            SalesSummaryData {
                past_day: None,
                month: None,
            }
        }
    });
    let day_sales = Signal::derive(move || sales.with(|s| s.past_day.clone()).unwrap_or_default());
    let month_sales = Signal::derive(move || sales.with(|s| s.month.clone()).unwrap_or_default());
    view! {
        <div class="mb-4 flex flex-wrap items-end justify-between gap-2">
            <h3 class="text-xl font-bold text-[color:var(--brand-fg)]">{t!(i18n, sale_history_insights_title)}</h3>
            <span class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, sale_history_insights_subtitle)}</span>
        </div>
        <div class="grid grid-cols-1 gap-4 xl:grid-cols-2">
            <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--color-text)_3%,_transparent)] p-3" class:hidden=move || sales.with(|s| s.past_day.is_none())>
                <h4 class="mb-3 text-sm font-semibold text-[color:var(--color-text)]">{t!(i18n, sale_history_last_24h)}</h4>
                <WindowStats sales=day_sales />
            </div>
            <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--color-text)_3%,_transparent)] p-3" class:hidden=move || sales.with(|s| s.month.is_none())>
                <h4 class="mb-3 text-sm font-semibold text-[color:var(--color-text)]">{t!(i18n, sale_history_last_30d)}</h4>
                <WindowStats sales=month_sales />
            </div>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, NaiveDate};

    fn create_sale(date: NaiveDateTime) -> SaleHistory {
        SaleHistory {
            id: 0,
            quantity: 1,
            price_per_item: 100,
            buying_character_id: 0,
            hq: false,
            sold_item_id: 0,
            sold_date: date,
            world_id: 0,
            buyer_name: None,
        }
    }

    #[test]
    fn test_find_date_range() {
        let base_date = NaiveDate::from_ymd_opt(2023, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let one_hour = Duration::hours(1);

        // Create sales: Newest first (descending)
        let sales: Vec<SaleHistory> = (0..10)
            .map(|i| create_sale(base_date - (one_hour * i)))
            .collect();

        // Sales dates:
        // 0: 12:00 (Newest)
        // 1: 11:00
        // ...
        // 9: 03:00 (Oldest)

        // Range: 09:00 to 11:00
        // Should include indices 1 (11:00), 2 (10:00), 3 (09:00)
        let start_range = base_date - Duration::hours(3); // 09:00
        let end_range = base_date - Duration::hours(1); // 11:00
        let range = start_range..=end_range;

        let result = find_date_range(range, &sales);
        assert!(result.is_some());
        let slice = result.unwrap();
        assert_eq!(slice.len(), 3);
        assert_eq!(slice[0].sold_date, end_range); // 11:00
        assert_eq!(slice.last().unwrap().sold_date, start_range); // 09:00
    }

    #[test]
    fn test_find_date_range_empty() {
        let range = NaiveDateTime::default()..=NaiveDateTime::default();
        let sales = vec![];
        assert!(find_date_range(range, &sales).is_none());
    }

    #[test]
    fn test_find_date_range_no_match() {
        let base_date = NaiveDate::from_ymd_opt(2023, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let sales = vec![create_sale(base_date)];

        let range = (base_date - Duration::hours(2))..=(base_date - Duration::hours(1));
        assert!(find_date_range(range, &sales).is_none());
    }

    #[test]
    fn test_sales_window_try_new() {
        let base_date = NaiveDate::from_ymd_opt(2023, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();

        // Let's create some sales!
        let mut sale1 = create_sale(base_date - Duration::hours(1));
        sale1.price_per_item = 100;
        sale1.quantity = 10;
        sale1.hq = false;

        let mut sale2 = create_sale(base_date - Duration::hours(2));
        sale2.price_per_item = 200;
        sale2.quantity = 5;
        sale2.hq = true;

        let mut sale3 = create_sale(base_date - Duration::hours(3));
        sale3.price_per_item = 300;
        sale3.quantity = 15;
        sale3.hq = false;

        let mut sale4 = create_sale(base_date - Duration::hours(4));
        sale4.price_per_item = 400;
        sale4.quantity = 10;
        sale4.hq = true;

        let mut sale5 = create_sale(base_date - Duration::hours(5));
        sale5.price_per_item = 500;
        sale5.quantity = 20;
        sale5.hq = false;

        let sales = vec![sale1, sale2, sale3, sale4, sale5];

        let start_range = base_date - Duration::hours(6);
        let end_range = base_date;
        let range = start_range..=end_range;

        let window = SalesWindow::try_new(range, &sales).unwrap();

        assert_eq!(
            window.total_gil,
            (100 * 10) + (200 * 5) + (300 * 15) + (400 * 10) + (500 * 20)
        );
        assert_eq!(
            window.average_unit_price,
            (100.0 + 200.0 + 300.0 + 400.0 + 500.0) / 5.0
        );
        assert_eq!(window.max_unit_price, 500);
        assert_eq!(window.min_unit_price, 100);
        // median of 100, 200, 300, 400, 500 is 300
        assert_eq!(window.median_unit_price, 300);
        // median of 5, 10, 10, 15, 20 is 10
        assert_eq!(window.median_stack_size, 10);
        // 2 out of 5 are HQ, so 40%
        assert_eq!(window.hq_percent, 40);
        // avg_duration is (6 hours) / 5 = 1 hour 12 minutes
        assert_eq!(window.time_between_sales, Duration::minutes(72));
    }

    #[test]
    fn test_sales_window_regression_error() {
        let base_date = NaiveDate::from_ymd_opt(2023, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();

        // Create two identical sales
        let sale1 = create_sale(base_date);
        let sale2 = create_sale(base_date);

        let sales = vec![sale1, sale2];

        let start_range = base_date - Duration::hours(1);
        let end_range = base_date + Duration::hours(1);
        let range = start_range..=end_range;

        let window = SalesWindow::try_new(range, &sales);

        assert!(window.is_some());
        let window = window.unwrap();
        assert_eq!(window.guessed_next_sale_price, window.average_unit_price);
        assert_eq!(window.p_value, 1.0);
    }
}
