use std::ops::RangeInclusive;

use super::{datacenter_name::*, gil::*, relative_time::*, world_name::*};
use chrono::{Duration, NaiveDateTime, TimeDelta, Utc};
use humantime::format_duration;
use icondata as i;
use itertools::Itertools;
use leptos::prelude::*;
use leptos_icons::*;
use linregress::{FormulaRegressionBuilder, RegressionDataBuilder};
use log::{error, info};
use ultros_api_types::{world_helper::AnySelector, SaleHistory};

#[component]
pub fn SaleHistoryTable(sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
    let (show_more, set_show_more) = signal(false);
    let sale_history = Memo::new(move |_| {
        let mut sales = sales();
        if !show_more() {
            sales.truncate(10);
        }
        sales
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
                                            view! { <Icon icon=i::BsCheck /> }.into_view()
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
        if sales.is_empty() {
            return None;
        }
        let total_gil = sales
            .iter()
            .map(|sale| sale.price_per_item as u64 * sale.quantity as u64)
            .sum();

        let unit_prices: Vec<f64> = sales
            .iter()
            .map(|sale| sale.price_per_item as f64)
            .collect();
        let start_timestamp = date_range.start().and_utc().timestamp();
        let dates = sales
            .iter()
            .map(|sale| (sale.sold_date.and_utc().timestamp() - start_timestamp) as f64)
            .collect::<Vec<_>>();
        let data = RegressionDataBuilder::new()
            .build_from([("X", unit_prices), ("Y", dates)])
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

        let mut unit_prices = sales
            .iter()
            .map(|sale| sale.price_per_item)
            .collect::<Vec<_>>();
        unit_prices.sort();
        let median_unit_price = unit_prices[unit_prices.len() / 2];
        let total_sale_price: i32 = unit_prices.iter().sum();
        let avg_sale_price = total_sale_price as f64 / unit_prices.len() as f64;
        let stack_sizes = sales
            .iter()
            .map(|sale| sale.quantity)
            .sorted()
            .collect::<Vec<_>>();
        let median_stack_size = stack_sizes[stack_sizes.len() / 2];
        let duration = *date_range.start() - *date_range.end();
        let avg_duration = duration / sales.len() as i32;
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
            hq_percent: ((sales.iter().filter(|sale| sale.hq).count() as f64 / sales.len() as f64)
                * 100.0)
                .round() as i32,
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
    let (start, _) = sales
        .iter()
        .enumerate()
        .find(|(_, sale)| date_range.contains(&sale.sold_date))?;
    let (end, _) = sales
        .iter()
        .enumerate()
        .rev()
        .find(|(_, sale)| date_range.contains(&sale.sold_date))?;
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
    let p_value = Memo::new(move |_| sales.with(|s| s.p_value));
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

#[component]
pub fn SalesInsights(sales: Signal<Vec<SaleHistory>>) -> impl IntoView {
    let sales = Memo::new(move |_| sales.with(|sales| SalesSummaryData::new(&sales)));
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
