use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        add_to_list::AddToList, clipboard::*, filter_card::*, gil::*, icon::Icon, item_icon::*,
        meta::*, query_button::QueryButton, skeleton::BoxSkeleton, toggle::Toggle, tooltip::*,
        virtual_scroller::*, world_picker::*,
    },
    error::AppError,
    global_state::LocalWorldData,
    math::filter_outliers_iqr,
};
use chrono::{Duration, Utc};
use humantime::{format_duration, parse_duration};
use icondata as i;
use leptos::{either::Either, prelude::*, reactive::wrappers::write::SignalSetter};
use leptos_router::{
    NavigateOptions,
    hooks::{query_signal, use_navigate, use_params_map, use_query_map},
};
use std::{
    cmp::Reverse,
    collections::{HashMap, hash_map::Entry},
    str::FromStr,
    sync::Arc,
};
use ultros_api_types::{
    cheapest_listings::CheapestListings,
    recent_sales::{RecentSales, SaleData},
    world_helper::{AnyResult, AnySelector, WorldHelper},
};
use xiv_gen::ItemId;

/// Computed sale stats
#[derive(Hash, Clone, Debug, PartialEq)]
struct SaleSummary {
    item_id: i32,
    hq: bool,
    /// this value is limited by the summary returned by the API
    num_sold: usize,
    /// Represents the average time between sales within the `num_sold`
    avg_sale_duration: Option<Duration>,
    max_price: i32,
    avg_price: i32,
    min_price: i32,
    median_price: i32,
}

#[derive(Hash, Clone, Debug, PartialEq, Eq)]
struct ProfitKey {
    item_id: i32,
    hq: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct ProfitData {
    estimated_sale_price: i32,
    cheapest_price: i32,
    cheapest_world_id: i32,
    sale_summary: SaleSummary,
}

#[derive(Clone, Debug, PartialEq)]
struct CalculatedProfitData {
    inner: Arc<ProfitData>,
    profit: i32,
    return_on_investment: i32,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Roi,
    Profit,
}

#[derive(Clone, Debug)]
struct ProfitTable(Vec<Arc<ProfitData>>);

fn listings_to_map(listings: CheapestListings) -> HashMap<ProfitKey, (i32, i32)> {
    listings
        .cheapest_listings
        .into_iter()
        .map(|listing| {
            (
                ProfitKey {
                    item_id: listing.item_id,
                    hq: listing.hq,
                },
                (listing.cheapest_price, listing.world_id),
            )
        })
        .collect()
}

fn compute_summary(
    sale: SaleData,
    hq_data: Option<&SaleData>,
    filter_outliers: bool,
) -> SaleSummary {
    let now = Utc::now().naive_utc();
    let SaleData { item_id, hq, sales } = sale;
    let min_price = hq_data
        .map(|sales| sales.sales.iter())
        .into_iter()
        .flatten()
        .chain(sales.iter())
        .map(|price| price.price_per_unit)
        .min()
        .unwrap_or_default();
    let max_price = sales
        .iter()
        .map(|price| price.price_per_unit)
        .max()
        .unwrap_or_default();

    let avg_price = if filter_outliers {
        let prices: Vec<i32> = sales.iter().map(|s| s.price_per_unit).collect();
        let filtered = filter_outliers_iqr(&prices);
        if filtered.is_empty() {
            0
        } else {
            (filtered.iter().map(|&p| p as i64).sum::<i64>() / filtered.len() as i64) as i32
        }
    } else {
        (sales
            .iter()
            .map(|price| price.price_per_unit as i64)
            .sum::<i64>()
            / sales.len() as i64) as i32
    };

    let t = sales
        .last()
        .map(|last| (last.sale_date - now).num_milliseconds().abs() / sales.len() as i64);
    let avg_sale_duration = t.map(Duration::milliseconds);

    let median_price = if sales.is_empty() {
        0
    } else {
        let mut prices: Vec<i32> = sales.iter().map(|s| s.price_per_unit).collect();
        prices.sort_unstable();
        prices[prices.len() / 2]
    };

    SaleSummary {
        item_id,
        hq,
        num_sold: sales.len(),
        avg_sale_duration,
        max_price,
        avg_price,
        min_price,
        median_price,
    }
}

// Add FromStr and ToString implementations for SortMode
impl FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "roi" => Ok(SortMode::Roi),
            "profit" => Ok(SortMode::Profit),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            SortMode::Roi => "roi",
            SortMode::Profit => "profit",
        };
        f.write_str(val)
    }
}

impl ProfitTable {
    fn new(
        sales: RecentSales,
        global_cheapest_listings: CheapestListings,
        world_cheapest_listings: CheapestListings,
        cross_region: Vec<CheapestListings>,
        filter_outliers: bool,
    ) -> Self {
        let mut region_cheapest = listings_to_map(global_cheapest_listings);
        let world_cheapest = listings_to_map(world_cheapest_listings);
        let cross_region = cross_region.into_iter().map(listings_to_map);

        // merge cross regions into region cheapest
        for cross_region in cross_region {
            for (key, (new_price, world_id)) in cross_region {
                match region_cheapest.entry(key) {
                    Entry::Occupied(mut entry) => {
                        let (current_price, _) = entry.get();
                        if *current_price > new_price {
                            entry.insert((new_price, world_id));
                        }
                    }
                    Entry::Vacant(e) => {
                        e.insert((new_price, world_id));
                    }
                }
            }
        }

        let hq_sales: HashMap<i32, SaleData> = sales
            .sales
            .iter()
            .filter(|sales| sales.hq)
            .map(|sale| (sale.item_id, sale.clone()))
            .collect();

        let table = sales
            .sales
            .into_iter()
            .flat_map(|sale| {
                let item_id = sale.item_id;
                let hq = sale.hq;
                let key = ProfitKey { item_id, hq };
                let (cheapest_price, cheapest_world_id) = *region_cheapest.get(&key)?;
                let summary = compute_summary(
                    sale,
                    (!hq).then(|| hq_sales.get(&item_id)).flatten(),
                    filter_outliers,
                );

                // Use the world's price as estimated sale price
                let estimated_sale_price =
                    if let Some((world_cheapest, _)) = world_cheapest.get(&key) {
                        (*world_cheapest - 1).min(summary.median_price)
                    } else {
                        (summary.median_price as f32 * 1.2) as i32
                    };

                Some(ProfitData {
                    estimated_sale_price,
                    sale_summary: summary,
                    cheapest_world_id,
                    cheapest_price,
                })
            })
            .map(Arc::new)
            .collect();

        ProfitTable(table)
    }
}

#[component]
fn PresetFilterButton(href: &'static str, label: &'static str) -> impl IntoView {
    view! {
        <a
            href=href
            class="btn-secondary"
        >
            {label}
        </a>
    }
}

#[component]
fn AnalyzerTable(
    sales: RecentSales,
    global_cheapest_listings: CheapestListings,
    world_cheapest_listings: CheapestListings,
    cross_region: Vec<CheapestListings>,
    worlds: Arc<WorldHelper>,
    world: Signal<String>,
    filter_outliers: bool,
) -> impl IntoView {
    let profits = ProfitTable::new(
        sales,
        global_cheapest_listings,
        world_cheapest_listings,
        cross_region,
        filter_outliers,
    );

    let items = &xiv_gen_db::data().items;
    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let (minimum_roi, set_minimum_roi) = query_signal::<i32>("roi");
    let (max_predicted_time, set_max_predicted_time) = query_signal::<String>("next-sale");
    let (world_filter, set_world_filter) = query_signal::<String>("world");
    let (datacenter_filter, set_datacenter_filter) = query_signal::<String>("datacenter");
    let (tax_enabled, set_tax_enabled) = query_signal::<bool>("tax");
    let (minimum_sales, set_minimum_sales) = query_signal::<usize>("sales");
    let (category_filter, set_category_filter) = query_signal::<i32>("category");

    let world_clone = worlds.clone();
    let world_filter_list = Memo::new(move |_| {
        let world = world_filter().or_else(datacenter_filter)?;
        let filter = world_clone
            .lookup_world_by_name(&world)?
            .all_worlds()
            .map(|w| w.id)
            .collect::<Vec<_>>();
        Some(filter)
    });

    let world_clone = worlds.clone();
    let lookup_world = Memo::new(move |_| {
        Some(AnySelector::from(
            &world_clone.lookup_world_by_name(&world())?,
        ))
    });

    let predicted_time =
        Memo::new(move |_| max_predicted_time().and_then(|d| parse_duration(d.as_str()).ok()));
    let predicted_time_string = Memo::new(move |_| {
        predicted_time()
            .map(|duration| format_duration(duration).to_string())
            .unwrap_or("---".to_string())
    });

    let sorted_data = Memo::new(move |_| {
        let include_tax = tax_enabled().unwrap_or(true);
        let mut sorted_data = profits
            .0
            .iter()
            .map(|data| {
                let estimated = if include_tax {
                    (data.estimated_sale_price as f32 * 0.95) as i32
                } else {
                    data.estimated_sale_price
                };
                let profit = estimated - data.cheapest_price;
                let return_on_investment = if data.cheapest_price > 0 {
                    ((profit as f32 / data.cheapest_price as f32) * 100.0) as i32
                } else {
                    0
                };
                CalculatedProfitData {
                    inner: data.clone(),
                    profit,
                    return_on_investment,
                }
            })
            .filter(move |data| {
                minimum_profit()
                    .map(|min| data.profit > min)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                minimum_roi()
                    .map(|roi| data.return_on_investment > roi)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                minimum_sales()
                    .map(|sales| data.inner.sale_summary.num_sold >= sales)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                category_filter()
                    .map(|cat_id| {
                        items
                            .get(&ItemId(data.inner.sale_summary.item_id))
                            .map(|item| item.item_search_category.0 == cat_id)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .filter(move |data| {
                predicted_time()
                    .map(|time| {
                        data.inner
                            .sale_summary
                            .avg_sale_duration
                            .map(|dur| dur.to_std().ok().map(|dur| dur < time).unwrap_or(false))
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .filter(move |data| {
                world_filter_list()
                    .map(|world_filter| world_filter.contains(&data.inner.cheapest_world_id))
                    .unwrap_or(true)
            })
            .filter(move |data| {
                data.inner.cheapest_world_id
                    != lookup_world()
                        .and_then(|w| w.as_world_id())
                        .unwrap_or_default()
            })
            .collect::<Vec<_>>();

        match sort_mode().unwrap_or(SortMode::Roi) {
            SortMode::Roi => sorted_data.sort_by_key(|data| Reverse(data.return_on_investment)),
            SortMode::Profit => sorted_data.sort_by_key(|data| Reverse(data.profit)),
        }
        sorted_data
            .into_iter()
            .enumerate()
            .collect::<Vec<(usize, CalculatedProfitData)>>()
    });
    view! {
        <div class="flex flex-col gap-6">
            <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-6">
                <FilterCard
                    title="Minimum Profit"
                    description="Set the minimum profit margin you want to see"
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">
                            {move || {
                                minimum_profit()
                                    .map(|profit| Either::Left(view! { <Gil amount=profit /> }))
                                    .unwrap_or(Either::Right("---"))
                            }}
                        </div>
                        <input
                            class="input"
                            min=0
                            max=100000
                            step=1000
                            placeholder="e.g. 100000"
                            type="number"
                            prop:value=minimum_profit
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                if let Ok(profit) = value.parse::<i32>() {
                                    set_minimum_profit(Some(profit))
                                } else if value.is_empty() {
                                    set_minimum_profit(None);
                                }
                            }
                        />
                    </div>
                </FilterCard>

                <FilterCard
                    title="Item Category"
                    description="Filter by item category"
                >
                    <div class="flex flex-col gap-2">
                         <select
                            class="input"
                            on:change=move |ev| {
                                let val = event_target_value(&ev);
                                if let Ok(id) = val.parse::<i32>() {
                                    set_category_filter(Some(id));
                                } else {
                                    set_category_filter(None);
                                }
                            }
                            prop:value=move || category_filter().map(|c| c.to_string()).unwrap_or_default()
                        >
                            <option value="">"All Categories"</option>
                            {
                                let mut categories = xiv_gen_db::data().item_search_categorys
                                    .iter()
                                    .filter(|(_, cat)| !cat.name.is_empty())
                                    .map(|(id, cat)| (id.0, cat.name.clone()))
                                    .collect::<Vec<_>>();
                                categories.sort_by(|a, b| a.1.cmp(&b.1));
                                categories.into_iter().map(|(id, name)| {
                                    view! { <option value=id.to_string() selected=move || category_filter() == Some(id)>{name}</option> }
                                }).collect_view()
                            }
                        </select>
                    </div>
                </FilterCard>

                <FilterCard
                    title="Minimum Sales"
                    description="Filter by minimum number of recent sales"
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">
                            {move || {
                                minimum_sales()
                                    .map(|sales| format!("{} sales", sales))
                                    .unwrap_or("---".to_string())
                            }}
                        </div>
                        <input
                            class="input"
                            min=0
                            max=1000
                            step=1
                            placeholder="e.g. 5"
                            type="number"
                            prop:value=minimum_sales
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                if let Ok(sales) = value.parse::<usize>() {
                                    set_minimum_sales(Some(sales));
                                } else if value.is_empty() {
                                    set_minimum_sales(None);
                                }
                            }
                        />
                    </div>
                </FilterCard>

                <FilterCard
                    title="Minimum ROI"
                    description="Set the minimum return on investment percentage"
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">
                            {move || {
                                minimum_roi()
                                    .map(|roi| format!("{roi}%"))
                                    .unwrap_or("---".to_string())
                            }}
                        </div>
                        <input
                            class="input"
                            min=0
                            max=100000
                            step=10
                            placeholder="e.g. 200"
                            type="number"
                            prop:value=minimum_roi
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                if let Ok(roi) = value.parse::<i32>() {
                                    set_minimum_roi(Some(roi));
                                } else if value.is_empty() {
                                    set_minimum_roi(None);
                                }
                            }
                        />
                    </div>
                </FilterCard>

                <FilterCard
                    title="Sale Time Prediction"
                    description="Filter by predicted time to next sale (e.g., 1w 30m)"
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">{predicted_time_string}</div>
                        <input
                            class="input"
                            placeholder="e.g. 7d 12h"
                            title="Accepts formats like 1h 30m, 7d, 1M (month), etc."
                            prop:value=move || max_predicted_time().unwrap_or_default()
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                set_max_predicted_time(Some(value))
                            }
                        />
                    </div>
                </FilterCard>

                <FilterCard
                    title="Tax Calculation"
                    description="Include 5% market tax in profit calculations"
                >
                    <div class="flex items-center">
                        <Toggle
                            checked=Signal::derive(move || tax_enabled().unwrap_or(true))
                            set_checked=SignalSetter::map(move |val: bool| set_tax_enabled(val.then_some(true)))
                            checked_label=Oco::Borrowed("Tax enabled (5%)")
                            unchecked_label=Oco::Borrowed("Tax disabled")
                        />
                    </div>
                </FilterCard>
            </div>

            // Results summary
            <div class="panel px-4 py-3 flex flex-col md:flex-row md:items-center gap-3 md:gap-0 md:justify-between">
                <div class="text-sm text-[color:var(--color-text)]">
                    <span class="text-brand-300 font-semibold">{move || sorted_data().len()}</span> " results"
                </div>
                <div class="flex flex-wrap gap-2">
                    {move || {
                        let mut chips: Vec<_> = Vec::new();
                        if let Some(p) = minimum_profit() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "Profit ≥ " <Gil amount=p />
                                    <button class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_profit(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(cat_id) = category_filter() {
                            let cat_name = xiv_gen_db::data()
                                .item_search_categorys
                                .get(&xiv_gen::ItemSearchCategoryId(cat_id))
                                .map(|c| c.name.clone())
                                .unwrap_or_else(|| format!("Category {}", cat_id));
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "Category: " {cat_name}
                                    <button class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_category_filter(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(sales) = minimum_sales() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "Sales ≥ " {sales}
                                    <button class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_sales(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(roi) = minimum_roi() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "ROI ≥ " {format!("{roi}%")}
                                    <button class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_roi(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(_ns) = max_predicted_time() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "Next Sale ≤ " {predicted_time_string()}
                                    <button class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_max_predicted_time(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(w) = world_filter() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "World: " {w.clone()}
                                    <button class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_world_filter(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(dc) = datacenter_filter() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "Datacenter: " {dc.clone()}
                                    <button class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_datacenter_filter(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if chips.is_empty() {
                            Either::Left(view! { <span class="text-sm text-[color:var(--color-text-muted)]">"no active filters"</span> })
                        } else {
                            Either::Right(view! { <>{chips}</> })
                        }
                    }}
                </div>
                <button class="text-sm text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] self-start md:self-auto" on:click=move |_| {
                    set_minimum_profit(None);
                    set_minimum_roi(None);
                    set_max_predicted_time(None);
                    set_world_filter(None);
                    set_datacenter_filter(None);
                    set_minimum_sales(None);
                    set_category_filter(None);
                }>
                    "Clear all"
                </button>
            </div>

            // Results table
            <div class="rounded-2xl overflow-x-auto panel content-visible contain-layout contain-paint will-change-scroll forced-layer">
                <VirtualScroller
                        viewport_height=720.0
                        row_height=40.0
                        overscan=8
                        header_height=64.0
                        variable_height=false
                        header=view! {
                            <div class="flex flex-row align-top h-16 bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]" role="rowgroup">
                                <div role="columnheader" class="w-[40px] p-4 text-center">
                                    "HQ"
                                </div>
                                <div role="columnheader" class="w-84 p-4">
                                    "Item"
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    <QueryButton
                                        class="!text-brand-300 hover:text-brand-200"
                                        active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                        key="sort"
                                        value="profit"
                                    >
                                        <div class="flex items-center gap-2">
                                            "Profit"
                                            {move || {
                                                (sort_mode() == Some(SortMode::Profit))
                                                    .then(|| view! { <Icon icon=i::BiSortDownRegular /> })
                                            }}
                                        </div>
                                    </QueryButton>
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    <QueryButton
                                        class="!text-brand-300 hover:text-brand-200"
                                        active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                        key="sort"
                                        value="roi"
                                        default=true
                                    >
                                        <div class="flex items-center gap-2">
                                            "ROI"
                                            {move || {
                                                (sort_mode() == Some(SortMode::Roi))
                                                    .then(|| view! { <Icon icon=i::BiSortDownRegular /> })
                                            }}
                                        </div>
                                    </QueryButton>
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    "Buy Price"
                                </div>
                                <div role="columnheader" class="w-30 p-4 flex flex-row gap-2 hidden lg:flex">
                                    "World"
                                    <div>
                                        {move || {
                                            world_filter()
                                                .map(|_filter| {
                                                    view! {
                                                        <div
                                                            class="hover:text-brand-200 transition-colors rounded-sm p-2 text-brand-300 cursor-pointer"
                                                            on:click=move |_| {
                                                                set_world_filter(None);
                                                            }
                                                        >
                                                            <Icon icon=icondata::MdiFilterRemove />
                                                        </div>
                                                    }
                                                })
                                        }}
                                    </div>
                                </div>
                                <div role="columnheader" class="w-30 p-4 flex flex-row gap-2 hidden xl:flex">
                                    "Datacenter"
                                    <div>
                                        {move || {
                                            datacenter_filter()
                                                .map(|_filter| {
                                                    view! {
                                                        <div
                                                            class="hover:text-brand-200 transition-colors rounded-sm p-2 text-brand-300 cursor-pointer"
                                                            on:click=move |_| {
                                                                set_datacenter_filter(None);
                                                            }
                                                        >
                                                            <Icon icon=icondata::MdiFilterRemove />
                                                        </div>
                                                    }
                                                })
                                        }}
                                    </div>
                                </div>
                                <div role="columnheader" class="w-30 p-4 hidden md:block">
                                    "Avg Sale Time"
                                </div>
                            </div>
                        }.into_any()
                        each=sorted_data.into()
                        key=move |(index, data): &(usize, CalculatedProfitData)| (
                            *index,
                            data.inner.sale_summary.item_id,
                            data.inner.cheapest_world_id,
                            data.inner.sale_summary.hq,
                            data.profit,
                        )
                        view=move |(index, data): (usize, CalculatedProfitData)| {
                            let data_clone = data.clone();
                            let world = worlds
                                .lookup_selector(AnySelector::World(data.inner.cheapest_world_id));
                            let datacenter = world
                                .as_ref()
                                .and_then(|world| {
                                    let datacenters = worlds.get_datacenters(world);
                                    datacenters.first().map(|dc| dc.name.as_str())
                                })
                                .unwrap_or_default()
                                .to_string();
                            let datacenter = Signal::derive(move || datacenter.clone());
                            let world = world
                                .as_ref()
                                .map(|r| r.get_name())
                                .unwrap_or_default()
                                .to_string();
                            let world = Signal::derive(move || world.clone());
                            let item_id = data.inner.sale_summary.item_id;
                            let item = items
                                .get(&ItemId(item_id))
                                .map(|item| item.name.as_str())
                                .unwrap_or_default();
                            let icon_loading = if index < 20 { "eager" } else { "" };
                            let classes = if (index % 2) == 0 {
                                "flex flex-row items-center flex-nowrap h-10 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_6%,transparent)] transition-colors"
                            } else {
                                "flex flex-row items-center flex-nowrap h-10 hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] hover:ring-1 hover:ring-[color:color-mix(in_srgb,var(--brand-ring)_30%,transparent)] bg-[color:color-mix(in_srgb,var(--color-text)_8%,transparent)] transition-colors"
                            };
                            view! {
                                <div class=classes role="row-group">
                                    <div role="cell" class="px-2 py-2 w-[40px] flex items-center justify-center">
                                        {if data.inner.sale_summary.hq {
                                            Some(view! { <span class="px-2 py-0.5 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]">"HQ"</span> })
                                        } else {
                                            None
                                        }}
                                    </div>
                                    <div role="cell" class="px-4 py-2 flex flex-row w-84 items-center gap-2">
                                        <a
                                            class="flex flex-row items-center gap-2 hover:text-brand-300 transition-colors truncate overflow-x-clip w-full"
                                            href=format!("/item/{}/{item_id}", world())
                                        >
                                            <div class="shrink-0">
                                                <ItemIcon item_id icon_size=IconSize::Small loading=icon_loading />
                                            </div>
                                            {item}
                                        </a>
                                        <AddToList item_id />
                                        <Clipboard clipboard_text=item.to_string() />
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 text-right flex items-center justify-end">
                                        <Gil amount=data.profit />
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 text-right flex items-center justify-end">
                                        <span class={
                                            let data = data_clone.clone();
                                            move || {
                                                let roi = data.return_on_investment;
                                                let tint = if roi >= 500 {
                                                    "24%"
                                                } else if roi >= 200 {
                                                    "20%"
                                                } else if roi >= 100 {
                                                    "16%"
                                                } else if roi >= 50 {
                                                    "12%"
                                                } else {
                                                    "10%"
                                                };
                                                format!(
                                                    "inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_{tint},transparent)]"
                                                )
                                            }
                                        }>
                                            {format!("{}%", data.return_on_investment)}
                                        </span>
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 text-right flex items-center justify-end">
                                        <Gil amount=data.inner.cheapest_price />
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 hidden lg:block flex items-center">
                                        <Tooltip tooltip_text=Signal::derive(move || {
                                            format!("Only show {}", world())
                                        })>
                                            <QueryButton
                                                key="world"
                                                value=world
                                                class="!text-brand-300 hover:text-brand-200"
                                                active_classes="!text-neutral-300 hover:text-neutral-200"
                                                remove_queries=&["datacenter"]
                                            >
                                                {world}
                                            </QueryButton>
                                        </Tooltip>
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 hidden xl:block flex items-center">
                                        <Tooltip tooltip_text=Signal::derive(move || {
                                            format!("Only show {}", datacenter())
                                        })>
                                            <QueryButton
                                                key="datacenter"
                                                value=datacenter
                                                class="!text-brand-300 hover:text-brand-200"
                                                active_classes="!text-neutral-300 hover:text-neutral-200"
                                                remove_queries=&["world"]
                                            >
                                                {datacenter}
                                            </QueryButton>
                                        </Tooltip>
                                    </div>
                                    <div role="cell" class="px-4 py-2 w-30 truncate hidden md:block flex items-center">
                                        {data.inner
                                            .sale_summary
                                            .avg_sale_duration
                                            .and_then(|duration| duration.to_std().ok())
                                            .map(|duration| {
                                                let secs = duration.as_secs();
                                                let days = secs / 86_400;
                                                let hours = (secs % 86_400) / 3_600;
                                                let minutes = (secs % 3_600) / 60;
                                                let seconds = secs % 60;
                                                let mut parts = Vec::new();
                                                if days > 0 { parts.push(format!("{}d", days)); }
                                                if hours > 0 { parts.push(format!("{}h", hours)); }
                                                if minutes > 0 && parts.len() < 2 { parts.push(format!("{}m", minutes)); }
                                                if seconds > 0 && parts.len() < 2 { parts.push(format!("{}s", seconds)); }
                                                if parts.is_empty() { "0s".to_string() } else { parts[..parts.len().min(2)].join(" ") }
                                            })
                                            .unwrap_or_else(|| "---".to_string())}
                                    </div>
                                </div>
                            }
                                .into_any()
                        }
                    />
            </div>
        </div>
    }.into_any()
}

#[component]
pub fn AnalyzerWorldView() -> impl IntoView {
    let params = use_params_map();
    let world = Memo::new(move |_| params.with(|p| p.get("world").clone()).unwrap_or_default());
    let sales = ArcResource::new(
        move || params.with(|p| p.get("world").clone()),
        move |world| async move {
            get_recent_sales_for_world(&world.ok_or(AppError::ParamMissing)?).await
        },
    );

    let world_cheapest_listings = ArcResource::new(
        move || params.with(|p| p.get("world").clone()),
        move |world| async move {
            let world = world.ok_or(AppError::ParamMissing)?;
            get_cheapest_listings(&world).await
        },
    );

    let region = Memo::new(move |_| {
        let worlds = use_context::<LocalWorldData>()
            .expect("Worlds should always be populated here")
            .0
            .unwrap();
        let world = params.with(|p| p.get("world").clone());
        let world = world.ok_or(AppError::ParamMissing)?;
        let region = worlds
            .lookup_world_by_name(&world)
            .map(|world| {
                let region = worlds.get_region(world);
                AnyResult::Region(region).get_name().to_string()
            })
            .ok_or(AppError::ParamMissing)?;
        Result::<_, AppError>::Ok(region)
    });

    let global_cheapest_listings = ArcResource::new(region, move |region| async move {
        get_cheapest_listings(region?.as_str()).await
    });

    let (cross_region_enabled, set_cross_region_enabled) = query_signal::<bool>("cross");
    let (filter_outliers, set_filter_outliers) = query_signal::<bool>("filter-outliers");
    let connected_regions = &["Europe", "Japan", "North-America", "Oceania"];
    let query = use_query_map();

    let enabled_regions = move || {
        let map = query();
        connected_regions
            .iter()
            .filter(|region| map.get(region).map(|value| value == "true").unwrap_or(true))
            .collect::<Vec<_>>()
    };

    let cross_region = ArcResource::new(
        move || (cross_region_enabled(), region(), enabled_regions()),
        move |(enabled, region, enabled_regions)| async move {
            let region = region?;
            if enabled.unwrap_or_default() && connected_regions.contains(&region.as_str()) {
                Ok(futures::future::join_all(
                    connected_regions
                        .iter()
                        .filter(|r| **r != region.as_str())
                        .filter(|r| enabled_regions.contains(r))
                        .map(|region| get_cheapest_listings(region)),
                )
                .await
                .into_iter()
                .filter_map(|l| l.ok())
                .collect())
            } else {
                Ok(vec![])
            }
        },
    );

    view! {
        <div class="main-content p-2 sm:p-6">
            <MetaTitle title=move || format!("Flip Finder - {}", world()) />
            <div class="container mx-auto max-w-7xl">
                <div class="flex flex-col gap-8">
                    // Header Section
                    <div class="panel p-4 sm:p-8 rounded-2xl">
                        <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-4">
                            "Flip Finder for " {world}
                        </h1>
                        <div class="flex flex-col gap-4">
                            <MetaDescription text=move || {
                                format!(
                                    "The analyzer enables FFXIV merchants to find the best items to buy on other worlds and sell on {}. Filter for the best profits or return, make gil through market arbitrage.",
                                    world(),
                                )
                            } />

                            // World Navigator
                            <div class="flex flex-col md:flex-row gap-4 items-center">
                                <AnalyzerWorldNavigator />
                                <div class="flex flex-col gap-2">
                                    <Toggle
                                        checked=Signal::derive(move || {
                                            cross_region_enabled().unwrap_or_default()
                                        })
                                        set_checked=SignalSetter::map(move |val: bool| set_cross_region_enabled(
                                            val.then_some(true),
                                        ))
                                        checked_label=Oco::Borrowed("Cross region enabled")
                                        unchecked_label=Oco::Borrowed("Cross region disabled")
                                    />
                                    <Toggle
                                        checked=Signal::derive(move || {
                                            filter_outliers().unwrap_or_default()
                                        })
                                        set_checked=SignalSetter::map(move |val: bool| set_filter_outliers(
                                            val.then_some(true),
                                        ))
                                        checked_label=Oco::Borrowed("Filter outliers enabled")
                                        unchecked_label=Oco::Borrowed("Filter outliers disabled")
                                    />

                                    <div
                                        class="flex flex-wrap gap-2"
                                        class:hidden=move || {
                                            !cross_region_enabled().unwrap_or_default()
                                        }
                                    >
                                        {move || {
                                            region()
                                                .map(|region| move || {
                                                    connected_regions
                                                        .iter()
                                                        .filter(|r| **r != region.as_str())
                                                        .map(|region| {
                                                            let (enabled, set_enabled) = query_signal::<
                                                                bool,
                                                            >(region.to_string());
                                                            view! {
                                                                <Toggle
                                                                    checked=Signal::derive(move || enabled().unwrap_or(true))
                                                                    set_checked=SignalSetter::map(move |checked: bool| {
                                                                        set_enabled(Some(checked));
                                                                    })
                                                                    checked_label=format!("{} enabled", region)
                                                                    unchecked_label=format!("{} disabled", region)
                                                                />
                                                            }
                                                        })
                                                        .collect::<Vec<_>>()
                                                })
                                                .ok()
                                        }}
                                    </div>
                                </div>
                            </div>

                            // Preset Filters
                            <div class="flex flex-wrap gap-4">
                                <PresetFilterButton
                                    href="?next-sale=7d&roi=300&profit=0&sort=profit&"
                                    label="300% return - 7 days"
                                />
                                <PresetFilterButton
                                    href="?next-sale=1M&roi=500&profit=200000&"
                                    label="500% return - 200K min profit - 1 month"
                                />
                                <PresetFilterButton href="?profit=100000" label="100K profit" />
                            </div>
                        </div>
                    </div>

                    // Main Content
                    <div class="min-h-screen">
                        <Suspense fallback=BoxSkeleton>
                            {move || {
                                let world_cheapest = world_cheapest_listings.get();
                                let sales = sales.get();
                                let global_cheapest_listings = global_cheapest_listings.get();
                                let cross_region = cross_region
                                    .get()
                                    .and_then(|r: Result<_, AppError>| r.ok())
                                    .unwrap_or_default();
                                let worlds = use_context::<LocalWorldData>()
                                    .expect("Worlds should always be populated here")
                                    .0
                                    .unwrap();
                                match (world_cheapest, sales, global_cheapest_listings) {
                                    (Some(Ok(w)), Some(Ok(s)), Some(Ok(g))) => {
                                        Either::Left(

                                            view! {
                                                <AnalyzerTable
                                                    sales=s
                                                    global_cheapest_listings=g
                                                    world_cheapest_listings=w
                                                    cross_region
                                                    worlds
                                                    world=world.into()
                                                    filter_outliers=filter_outliers().unwrap_or(false)
                                                />
                                            },
                                        )
                                    }
                                    _ => {
                                        Either::Right(
                                            view! {
                                                <div class="text-xl text-[color:var(--color-text)] text-center p-8
                                                bg-brand-900/20 rounded-2xl border border-white/10">
                                                    "Failed to load analyzer - try again in 30 seconds"
                                                </div>
                                            },
                                        )
                                    }
                                }
                            }}
                        </Suspense>
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
fn AnalyzerWorldNavigator() -> impl IntoView {
    let nav = use_navigate();
    let params = use_params_map();
    let worlds = use_context::<LocalWorldData>()
        .expect("Should always have local world data")
        .0
        .unwrap();

    let initial_world = params.with_untracked(|p| {
        let world = p.get_str("world").unwrap_or_default();
        worlds
            .lookup_world_by_name(world)
            .and_then(|w| w.as_world().cloned())
    });

    let (current_world, set_current_world) = signal(initial_world);
    let query = use_query_map();

    Effect::new(move |_| {
        if let Some(world) = current_world() {
            let world = world.name;
            let query_map = query.get_untracked();
            let query = query_map.to_query_string();
            nav(
                &format!("/flip-finder/{world}?{query}"),
                NavigateOptions::default(),
            );
        }
    });

    view! {
        <div class="flex flex-col md:flex-row items-center gap-2">
            <label class="text-[color:var(--brand-fg)] font-semibold">"Select World:"</label>
            <div class="w-full md:w-auto">
                <WorldOnlyPicker
                    current_world=current_world.into()
                    set_current_world=set_current_world.into()
                />
            </div>
        </div>
    }
}

#[component]
pub fn Analyzer() -> impl IntoView {
    view! {
        <MetaTitle title="Flip Finder - Ultros" />
        <MetaDescription text="Find items on the Final Fantasy 14 marketboard that are great for resale. Used to earn gil quickly." />

        <div class="main-content p-2 sm:p-6">
            <div class="container mx-auto max-w-7xl">
                <div class="flex flex-col gap-8">
                    // Hero Section
                    <div class="panel p-4 sm:p-8 rounded-2xl">
                        <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-4">
                            "Flip Finder"
                        </h1>
                        <p class="text-xl text-[color:var(--color-text)] leading-relaxed mb-6">
                            "The analyzer helps find items on the Final Fantasy 14 marketboard that are
                             cheaper on other worlds that sell for more on your world, enabling you to
                             earn gil through market arbitrage."
                        </p>
                        <p class="text-lg text-[color:var(--color-text)]/90 mb-8">
                            "Adjust parameters to find items that sell well and maximize your profits."
                        </p>

                        // World Selection
                        <div class="panel p-6 rounded-xl">
                            <h2 class="text-xl font-semibold text-[color:var(--brand-fg)] mb-4">
                                "Choose a world to get started:"
                            </h2>
                            <AnalyzerWorldNavigator />
                        </div>
                    </div>

                    // Features Grid
                    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
                        <div class="card p-6 rounded-lg transition-colors duration-200">
                            <Icon
                                attr:class="text-brand-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaMoneyBillTrendUpSolid
                            />
                            <h3 class="text-xl font-bold text-brand-300 mb-2">"Profit Tracking"</h3>
                            <p class="text-gray-300">
                                "Monitor profit margins and ROI across different worlds"
                            </p>
                        </div>

                        <div class="card p-6 rounded-lg transition-colors duration-200">
                            <Icon
                                attr:class="text-brand-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaChartLineSolid
                            />
                            <h3 class="text-xl font-bold text-brand-300 mb-2">"Market Analysis"</h3>
                            <p class="text-gray-300">
                                "Track market trends and identify profitable opportunities"
                            </p>
                        </div>

                        <div class="card p-6 rounded-lg transition-colors duration-200">
                            <Icon
                                attr:class="text-brand-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaFilterSolid
                            />
                            <h3 class="text-xl font-bold text-brand-300 mb-2">"Custom Filters"</h3>
                            <p class="text-gray-300">
                                "Set custom parameters to find your perfect trades"
                            </p>
                        </div>
                    </div>

                    // Tips Section
                    <div class="panel p-6 rounded-2xl">
                        <h2 class="text-xl font-bold text-brand-300 mb-4">"Trading Tips"</h2>
                        <ul class="list-disc list-inside text-gray-300 space-y-2">
                            <li>
                                "Use the ROI filter to find items with the best return on investment"
                            </li>
                            <li>
                                "Check the sale frequency to ensure items will sell in a reasonable time"
                            </li>
                            <li>"Consider transportation costs when calculating profits"</li>
                            <li>
                                "Start with smaller investments until you understand the market"
                            </li>
                        </ul>
                    </div>
                </div>
            </div>
        </div>
    }
}
