use std::borrow::Cow;

use chrono::TimeZone;
use chrono::Utc;
use itertools::Itertools;
use leptos::prelude::*;
use ultros_api_types::SaleHistory;

use crate::components::gil::Gil;
use crate::components::toggle::Toggle;

/// Returns a filter where Some((min, max))
fn get_iqr_filter(sales: &[SaleHistory]) -> Option<(i32, i32)> {
    if sales.len() < 10 {
        return None;
    }
    let sales_prices = sales
        .iter()
        .map(|sales| sales.price_per_item)
        .sorted()
        .collect::<Vec<_>>();
    let first_quartile_index = sales_prices.len() / 4;
    let last_quartile_index = sales_prices.len() - first_quartile_index;
    let first_quartile_value = sales_prices.get(first_quartile_index)?;
    let third_quartile_value = sales_prices.get(last_quartile_index)?;
    let interquartile_range = ((third_quartile_value - first_quartile_value) as f32 * 2.5) as i32;
    Some((
        first_quartile_value - interquartile_range,
        third_quartile_value + interquartile_range,
    ))
}

fn filter_outliers(sales: &[SaleHistory]) -> Cow<'_, [SaleHistory]> {
    if let Some((min, max)) = get_iqr_filter(sales) {
        let range = min..=max;
        Cow::Owned(
            sales
                .iter()
                .filter(|sales| range.contains(&sales.price_per_item))
                .cloned()
                .collect(),
        )
    } else {
        Cow::Borrowed(sales)
    }
}

#[component]
pub fn PriceHistoryChart(
    #[prop(into)] sales: Signal<Vec<SaleHistory>>,
    #[prop(into)] set_hovered_sale: WriteSignal<Option<SaleHistory>>,
) -> impl IntoView {
    let (filter_outliers_toggle, set_filter_outliers_toggle) = signal(true);

    let filtered_sales = Memo::new(move |_| {
        sales.with(|s| {
            if filter_outliers_toggle() {
                filter_outliers(s).into_owned()
            } else {
                s.clone()
            }
        })
    });

    view! {
        <div class="flex flex-col gap-4">
            <div class="w-full h-[400px] relative">
                <ChartSvg
                    sales=filtered_sales
                    set_hovered_sale=set_hovered_sale
                />
            </div>
            <div class="flex justify-end">
                <Toggle
                    checked=filter_outliers_toggle
                    set_checked=set_filter_outliers_toggle
                    checked_label="Filtering outliers"
                    unchecked_label="No filter"
                />
            </div>
        </div>
    }
}

#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
    sale_idx: usize,
}

#[component]
fn ChartSvg(
    sales: Memo<Vec<SaleHistory>>,
    set_hovered_sale: WriteSignal<Option<SaleHistory>>,
) -> impl IntoView {
    let (hover_point, set_hover_point) = signal::<Option<Point>>(None);

    let chart_data = Memo::new(move |_| {
        let sales = sales.get();
        if sales.is_empty() {
            return None;
        }

        let min_time = sales
            .iter()
            .map(|s| s.sold_date.and_utc().timestamp())
            .min()
            .unwrap_or(0);
        let max_time = sales
            .iter()
            .map(|s| s.sold_date.and_utc().timestamp())
            .max()
            .unwrap_or(0);

        let time_range = (max_time - min_time).max(1); // Avoid div by zero

        let min_price = 0;
        let max_price = sales
            .iter()
            .map(|s| s.price_per_item)
            .max()
            .unwrap_or(0);

        // Add some padding to max price (e.g. 10%)
        let max_price_padded = (max_price as f64 * 1.1).ceil() as i32;
        let price_range = max_price_padded.max(1);

        Some((sales, min_time, max_time, time_range, min_price, max_price_padded, price_range))
    });

    view! {
        <div class="w-full h-full select-none relative group">
            <svg
                viewBox="0 0 800 400"
                class="w-full h-full font-sans text-xs overflow-visible"
                preserveAspectRatio="none"
            >
                // Grid and Axes
                {move || {
                    if let Some((_, min_time, max_time, _, _, max_price, _)) = chart_data.get() {
                        let y_ticks = 5;
                        let x_ticks = 5;

                        let y_lines = (0..=y_ticks).map(|i| {
                            let y = 350.0 - (i as f64 / y_ticks as f64) * 350.0;
                            let price = (i as f64 / y_ticks as f64) * max_price as f64;
                            view! {
                                <line x1="50" y1=y x2="800" y2=y stroke="var(--color-outline)" stroke-width="1" stroke-dasharray="4" />
                                <text x="45" y=y dy="4" text-anchor="end" fill="var(--color-text-muted)">{format_price(price)}</text>
                            }
                        }).collect_view();

                        let x_lines = (0..=x_ticks).map(|i| {
                            let x = 50.0 + (i as f64 / x_ticks as f64) * 750.0;
                            let time = min_time + ((i as f64 / x_ticks as f64) * (max_time - min_time) as f64) as i64;
                            let dt = Utc.timestamp_opt(time, 0).unwrap();
                            let date_str = dt.format("%m-%d").to_string();
                            view! {
                                <line x1=x y1="0" x2=x y2="350" stroke="var(--color-outline)" stroke-width="1" stroke-dasharray="4" />
                                <text x=x y="365" text-anchor="middle" fill="var(--color-text-muted)">{date_str}</text>
                            }
                        }).collect_view();

                        view! {
                            <g class="grid-lines">
                                {y_lines}
                                {x_lines}
                            </g>
                            // Axis lines
                            <line x1="50" y1="0" x2="50" y2="350" stroke="var(--color-text)" stroke-width="1" />
                            <line x1="50" y1="350" x2="800" y2="350" stroke="var(--color-text)" stroke-width="1" />
                        }.into_any()
                    } else {
                        ().into_any()
                    }
                }}

                // Points
                {move || {
                    if let Some((sales, min_time, _, time_range, _, _, price_range)) = chart_data.get() {
                        sales.iter().enumerate().map(|(idx, sale)| {
                            let sale = sale.clone(); // Clone to own it for the closure
                            let x_rel = (sale.sold_date.and_utc().timestamp() - min_time) as f64 / time_range as f64;
                            let y_rel = sale.price_per_item as f64 / price_range as f64;

                            let cx = 50.0 + x_rel * 750.0;
                            let cy = 350.0 - y_rel * 350.0;

                            // Color based on HQ/NQ? or World? plotters version uses dynamic palette.
                            // Let's use brand color for now, maybe differ for HQ.
                            let color = if sale.hq { "#95c521" } else { "var(--brand-500)" };
                            let radius = (sale.quantity as f64 / 50.0 * 5.0).clamp(3.0, 6.0);

                            view! {
                                <circle
                                    cx=cx
                                    cy=cy
                                    r=radius
                                    fill=color
                                    stroke="var(--bg-card)"
                                    stroke-width="1"
                                    class="hover:stroke-white transition-all cursor-crosshair"
                                    on:mouseenter=move |_| {
                                        set_hovered_sale(Some(sale.clone()));
                                        set_hover_point(Some(Point { x: cx, y: cy, sale_idx: idx }));
                                    }
                                    on:mouseleave=move |_| {
                                        set_hovered_sale(None);
                                        set_hover_point(None);
                                    }
                                />
                            }
                        }).collect_view().into_any()
                    } else {
                        ().into_any()
                    }
                }}
            </svg>

            // Tooltip
            {move || {
                 if let Some(Point { x, y, sale_idx }) = hover_point.get()
                    && let Some((sales, ..)) = chart_data.get()
                    && let Some(sale) = sales.get(sale_idx) {

                    // Convert SVG coords to % for absolute positioning if possible,
                    // or just use pixels assuming the parent div matches SVG viewbox aspect ratio roughly?
                    // The SVG has viewBox="0 0 800 400", parent is h-[400px].
                    // We can use left: (x/800)*100 %, top: (y/400)*100 %

                    let left = format!("{}%", (x / 800.0) * 100.0);
                    let top = format!("{}%", (y / 400.0) * 100.0);

                    view! {
                        <div
                            class="absolute z-20 pointer-events-none transform -translate-x-1/2 -translate-y-full pb-2"
                            style:left=left
                            style:top=top
                        >
                            <div class="bg-[var(--bg-floating)] border border-[var(--color-outline)] rounded shadow-lg p-2 text-xs text-[var(--color-text)] whitespace-nowrap">
                                <div class="font-bold flex items-center gap-1">
                                    <Gil amount=sale.price_per_item />
                                    {sale.hq.then(|| view!{ <span class="text-[#95c521]">"HQ"</span> })}
                                </div>
                                <div>{format!("x{}", sale.quantity)}</div>
                                <div class="opacity-75">{sale.buyer_name.clone().unwrap_or_default()}</div>
                                <div class="opacity-75">{sale.sold_date.to_string()}</div>
                            </div>
                        </div>
                    }.into_any()
                 } else {
                     ().into_any()
                 }
            }}
        </div>
    }
}

fn format_price(price: f64) -> String {
    if price >= 1_000_000.0 {
        format!("{:.1}m", price / 1_000_000.0)
    } else if price >= 1_000.0 {
        format!("{:.1}k", price / 1_000.0)
    } else {
        format!("{:.0}", price)
    }
}
