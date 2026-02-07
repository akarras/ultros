use std::ops::RangeInclusive;

use super::{datacenter_name::*, gil::*, relative_time::*, world_name::*};
use crate::components::icon::Icon;
use chrono::{Duration, NaiveDateTime, TimeDelta, Utc};
use icondata as i;
use leptos::prelude::*;
use linregress::{FormulaRegressionBuilder, RegressionDataBuilder};
use log::{error, info};
use ultros_api_types::{SaleHistory, world_helper::AnySelector};

#[component]
pub fn SaleHistoryTable(sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
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
                    <th>"hq"</th>
                    <th>"price"</th>
                    <th>"qty."</th>
                    <th>"total"</th>
                    <th>"purchaser"</th>
                    <th>"world"</th>
                    <th>"datacenter"</th>
                    <th>"time sold"</th>
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
                                                <span class="sr-only">"High Quality"</span>
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
                                            "Show more"
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

        unit_prices.sort_unstable();
        let median_unit_price = unit_prices[count / 2];
        let avg_sale_price = total_sale_price as f64 / count as f64;

        stack_sizes.sort_unstable();
        let median_stack_size = stack_sizes[count / 2];

        let duration = *date_range.start() - *date_range.end();
        let avg_duration = duration / count as i32;
        let next_sale_time = [(
            "Y",
            vec![
                ((*date_range.end() + avg_duration).and_utc().timestamp() - start_timestamp) as f64,
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

        Some(Self {
            total_gil,
            average_unit_price: avg_sale_price,
            max_unit_price: *unit_prices.last()?,
            min_unit_price: *unit_prices.first()?,
            median_stack_size,
            hq_percent: ((hq_count as f64 / count as f64) * 100.0).round() as i32,
            guessed_next_sale_price: next,
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
    // Assuming sales are sorted by date descending (newest first).
    // Find the first sale (newest) that is <= range.end (usually now).
    let start = sales
        .iter()
        .position(|sale| sale.sold_date <= *date_range.end())?;

    // Check if the found sale is actually >= range.start.
    // If not, then all subsequent sales are also < range.start (since sorted descending),
    // so no sales are in range.
    if sales[start].sold_date < *date_range.start() {
        return None;
    }

    // Find the last sale (oldest) that is >= range.start.
    // We search from `start` onwards.
    // We look for the first sale that is < range.start. The one before it is the last one in range.
    let end = sales[start..]
        .iter()
        .position(|sale| sale.sold_date < *date_range.start())
        .map(|pos| start + pos - 1)
        .unwrap_or(sales.len() - 1);

    Some(&sales[start..=end])
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
    let total_gil = Memo::new(move |_| sales.with(|s| s.total_gil));
    let average_unit_price =
        Memo::new(move |_| sales.with(|s| s.average_unit_price.round() as i32));
    let max_unit_price = Memo::new(move |_| sales.with(|s| s.max_unit_price));
    let median_unit_price = Memo::new(move |_| sales.with(|s| s.median_unit_price));
    let min_unit_price = Memo::new(move |_| sales.with(|s| s.min_unit_price));
    let median_stack_size = Memo::new(move |_| sales.with(|s| s.median_stack_size));
    let guessed_next_sale_price =
        Memo::new(move |_| sales.with(|s| s.guessed_next_sale_price.round() as i32));
    let time_between_sales = Memo::new(move |_| sales.with(|s| s.time_between_sales));
    let hq_percent = Memo::new(move |_| sales.with(|s| s.hq_percent));
    view! {
        <div class="flex flex-wrap gap-2">
            <div class="rounded-md px-3 py-1.5 text-sm border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_14%,_transparent)]">
                <span class="text-[color:var(--color-text-muted)] mr-1">"Gil sold"</span>
                <GenericGil<u64> amount=total_gil />
            </div>
            <div class="rounded-md px-3 py-1.5 text-sm border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_14%,_transparent)]">
                <span class="text-[color:var(--color-text-muted)] mr-1">"Avg price"</span>
                <Gil amount=average_unit_price />
            </div>
            <div class="rounded-md px-3 py-1.5 text-sm border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_14%,_transparent)]">
                <span class="text-[color:var(--color-text-muted)] mr-1">"Median price"</span>
                <Gil amount=median_unit_price />
            </div>
            <div class="rounded-md px-3 py-1.5 text-sm border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_14%,_transparent)]">
                <span class="text-[color:var(--color-text-muted)] mr-1">"Min"</span>
                <Gil amount=min_unit_price />
            </div>
            <div class="rounded-md px-3 py-1.5 text-sm border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_14%,_transparent)]">
                <span class="text-[color:var(--color-text-muted)] mr-1">"Max"</span>
                <Gil amount=max_unit_price />
            </div>
            <div class="rounded-md px-3 py-1.5 text-sm border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_14%,_transparent)]">
                <span class="text-[color:var(--color-text-muted)] mr-1">"Typical stack"</span>
                {median_stack_size}
            </div>
            <div class="rounded-md px-3 py-1.5 text-sm border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_14%,_transparent)]">
                <span class="text-[color:var(--color-text-muted)] mr-1">"HQ %"</span>
                {move || format!("{}%", hq_percent())}
            </div>
            <div class="rounded-md px-3 py-1.5 text-sm border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_14%,_transparent)]">
                <span class="text-[color:var(--color-text-muted)] mr-1">"Next sale (est.)"</span>
                <Gil amount=guessed_next_sale_price />
            </div>
            <div class="rounded-md px-3 py-1.5 text-sm border border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,_var(--brand-ring)_14%,_transparent)]">
                <span class="text-[color:var(--color-text-muted)] mr-1">"Time between sales"</span>
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
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeDelta, Utc};

    fn create_sale(seconds_ago: i64) -> SaleHistory {
        SaleHistory {
            id: 0,
            quantity: 1,
            price_per_item: 100,
            buying_character_id: 0,
            hq: false,
            sold_item_id: 0,
            sold_date: Utc::now().naive_utc() - TimeDelta::seconds(seconds_ago),
            world_id: 0,
            buyer_name: None,
        }
    }

    #[test]
    fn test_find_date_range() {
        // Sales are sorted descending (newest first).
        // create_sale(10) is newer than create_sale(20).
        let sales = vec![
            create_sale(10), // index 0
            create_sale(20), // index 1
            create_sale(30), // index 2
            create_sale(40), // index 3
            create_sale(50), // index 4
        ];

        let now = Utc::now().naive_utc();

        // Range covering index 1 to 3 (20s ago to 40s ago)
        let range = (now - TimeDelta::seconds(45))..=(now - TimeDelta::seconds(15));

        let result = find_date_range(range, &sales);
        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.len(), 3); // 20, 30, 40 are in range
        // Wait, 15s ago is END of range (newest allowed). 45s ago is START of range (oldest allowed).
        // 10s ago is > END (too new).
        // 20s ago is <= END (ok). >= START (ok).
        // 30s ago is ok.
        // 40s ago is ok.
        // 50s ago is < START (too old).

        // So expected indices: 1, 2, 3.
        assert_eq!(result[0].sold_date, sales[1].sold_date);
        assert_eq!(result[2].sold_date, sales[3].sold_date);
    }

    #[test]
    fn test_find_date_range_all() {
        let sales = vec![create_sale(10), create_sale(20)];
        let now = Utc::now().naive_utc();
        let range = (now - TimeDelta::seconds(100))..=(now + TimeDelta::seconds(100));

        let result = find_date_range(range, &sales).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_find_date_range_none() {
        let sales = vec![create_sale(10), create_sale(20)];
        let now = Utc::now().naive_utc();
        // Range in future
        let range = (now + TimeDelta::seconds(10))..=(now + TimeDelta::seconds(20));
        assert!(find_date_range(range, &sales).is_none());

        // Range in past
        let range = (now - TimeDelta::seconds(100))..=(now - TimeDelta::seconds(50));
        // All sales are newer than range end (-50).
        // Wait, sales are -10, -20.
        // Range end is -50.
        // sales[0] (-10) > -50.
        // sales[1] (-20) > -50.
        // So position returns None.
        assert!(find_date_range(range, &sales).is_none());
    }

    #[test]
    fn test_find_date_range_partial() {
        let sales = vec![create_sale(10), create_sale(20), create_sale(30)];
        let now = Utc::now().naive_utc();

        // Range covers first 2
        // End: -5 (newest allowed). Start: -25 (oldest allowed).
        // -10: ok.
        // -20: ok.
        // -30: too old.
        let range = (now - TimeDelta::seconds(25))..=(now - TimeDelta::seconds(5));
        let result = find_date_range(range, &sales).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].sold_date, sales[0].sold_date);
        assert_eq!(result[1].sold_date, sales[1].sold_date);
    }
}

#[component]
pub fn SalesInsights(sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
    let sales = Memo::new(move |_| sales.with(|sales| SalesSummaryData::new(sales)));
    let day_sales = Memo::new(move |_| sales.with(|s| s.past_day.clone()).unwrap_or_default());
    let month_sales = Memo::new(move |_| sales.with(|s| s.month.clone()).unwrap_or_default());
    view! {
        <h3 class="text-xl font-bold text-[color:var(--brand-fg)] mb-2">"Sales at a glance"</h3>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
            <div class:hidden=move || sales.with(|s| s.past_day.is_none())>
                <h4 class="text-sm text-[color:var(--color-text-muted)] mb-1">"Last 24 hours"</h4>
                <WindowStats sales=day_sales />
            </div>
            <div class:hidden=move || sales.with(|s| s.month.is_none())>
                <h4 class="text-sm text-[color:var(--color-text-muted)] mb-1">"Last 30 days"</h4>
                <WindowStats sales=month_sales />
            </div>
        </div>
    }
    .into_any()
}
