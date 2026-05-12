use crate::global_state::xiv_data::tracked_data;
use crate::i18n::*;
use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        add_to_list::AddToList, clipboard::*, filter_card::*, gil::*, icon::Icon, item_icon::*,
        meta::*, query_button::QueryButton, skeleton::BoxSkeleton, toggle::Toggle, tooltip::*,
        virtual_scroller::*, world_picker::*,
    },
    error::AppError,
    global_state::LocalWorldData,
    math::filter_outliers_iqr_in_place,
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
    /// Time since the most-recent sale. `None` if no sales.
    days_since_last_sale: Option<Duration>,
    max_price: i32,
    avg_price: i32,
    /// Robust mid-point of the clamped sales — used as the realistic seller estimate.
    median_price: i32,
    /// Floor of the clamped sales — worst-case undercut.
    min_price: i32,
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
    profit_per_day: i32,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Roi,
    Profit,
    ProfitPerDay,
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

/// Sniper-clamp threshold: drop any sale priced below this fraction of the raw median.
const SNIPER_FRACTION: f64 = 0.1;

fn median_i32(sorted: &[i32]) -> i32 {
    if sorted.is_empty() {
        return 0;
    }
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        ((sorted[n / 2 - 1] as i64 + sorted[n / 2] as i64) / 2) as i32
    }
}

fn compute_summary(sale: SaleData, filter_outliers: bool) -> SaleSummary {
    let now = Utc::now().naive_utc();
    let SaleData { item_id, hq, sales } = sale;

    if sales.is_empty() {
        return SaleSummary {
            item_id,
            hq,
            num_sold: 0,
            avg_sale_duration: None,
            days_since_last_sale: None,
            max_price: 0,
            avg_price: 0,
            median_price: 0,
            min_price: 0,
        };
    }

    // 1. Raw-median pass for the sniper threshold.
    let mut raw: Vec<i32> = sales.iter().map(|s| s.price_per_unit).collect();
    raw.sort_unstable();
    let raw_median = median_i32(&raw);
    let floor = (raw_median as f64 * SNIPER_FRACTION) as i32;

    // 2. Build the clamped vector. If the clamp would remove everything, keep the raw set.
    let mut clamped: Vec<i32> = raw.iter().copied().filter(|p| *p >= floor).collect();
    if clamped.is_empty() {
        clamped = raw;
    }
    let median_price = median_i32(&clamped);
    let min_price = *clamped.first().unwrap_or(&0);
    let max_price = *clamped.last().unwrap_or(&0);

    // 3. Average price respects the existing IQR filter-outliers toggle.
    let avg_price = if filter_outliers {
        let mut prices = clamped.clone();
        let filtered = filter_outliers_iqr_in_place(&mut prices);
        if filtered.is_empty() {
            0
        } else {
            (filtered.iter().map(|&p| p as i64).sum::<i64>() / filtered.len() as i64) as i32
        }
    } else {
        (clamped.iter().map(|&p| p as i64).sum::<i64>() / clamped.len() as i64) as i32
    };

    // 4. Velocity. Newest first in the API's response.
    let newest = sales.first().map(|s| s.sale_date);
    let oldest = sales.last().map(|s| s.sale_date);
    let avg_sale_duration = oldest.map(|last| {
        let ms = (last - now).num_milliseconds().abs() / sales.len() as i64;
        Duration::milliseconds(ms)
    });
    let days_since_last_sale =
        newest.map(|n| Duration::milliseconds((now - n).num_milliseconds().max(0)));

    SaleSummary {
        item_id,
        hq,
        num_sold: sales.len(),
        avg_sale_duration,
        days_since_last_sale,
        max_price,
        avg_price,
        median_price,
        min_price,
    }
}

// Add FromStr and ToString implementations for SortMode
impl FromStr for SortMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "roi" => Ok(SortMode::Roi),
            "profit" => Ok(SortMode::Profit),
            "profit-per-day" => Ok(SortMode::ProfitPerDay),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = match self {
            SortMode::Roi => "roi",
            SortMode::Profit => "profit",
            SortMode::ProfitPerDay => "profit-per-day",
        };
        f.write_str(val)
    }
}

/// Listings whose price is at least this multiple of the row's median sale are treated as troll
/// listings and ignored when picking the world floor.
const TROLL_MULTIPLE: i64 = 50;

fn is_troll_listing(price: i32, median: i32) -> bool {
    median > 0 && (price as i64) > (median as i64).saturating_mul(TROLL_MULTIPLE)
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

        for cross in cross_region.into_iter().map(listings_to_map) {
            for (key, (new_price, world_id)) in cross {
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

        let table = sales
            .sales
            .into_iter()
            .flat_map(|sale| {
                let item_id = sale.item_id;
                let hq = sale.hq;
                let key = ProfitKey { item_id, hq };
                let (raw_region_price, region_world_id) = *region_cheapest.get(&key)?;
                let summary = compute_summary(sale, filter_outliers);

                // Troll-listing guard: if the region floor is implausibly high vs the median,
                // drop the row entirely — the displayed "deal" would be fictional.
                if is_troll_listing(raw_region_price, summary.median_price) {
                    return None;
                }

                // Same guard on the local world floor — if it's a troll, ignore it and fall
                // through to the median as the estimate.
                let world_floor = world_cheapest.get(&key).and_then(|(price, _)| {
                    if is_troll_listing(*price, summary.median_price) {
                        None
                    } else {
                        Some(*price)
                    }
                });

                let estimated_sale_price = match world_floor {
                    Some(floor) => summary.median_price.min(floor),
                    None => summary.median_price,
                };

                Some(ProfitData {
                    estimated_sale_price,
                    sale_summary: summary,
                    cheapest_world_id: region_world_id,
                    cheapest_price: raw_region_price,
                })
            })
            .map(Arc::new)
            .collect();

        ProfitTable(table)
    }
}

#[component]
fn PresetFilterButton(href: &'static str, #[prop(into)] label: String) -> impl IntoView {
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
    let i18n = use_i18n();
    let profits = ProfitTable::new(
        sales,
        global_cheapest_listings,
        world_cheapest_listings,
        cross_region,
        filter_outliers,
    );

    let items = &tracked_data().items;
    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let (minimum_profit_per_day, set_minimum_profit_per_day) = query_signal::<i32>("ppd");
    let (minimum_roi, set_minimum_roi) = query_signal::<i32>("roi");
    let (max_predicted_time, set_max_predicted_time) = query_signal::<String>("next-sale");
    let (world_filter, set_world_filter) = query_signal::<String>("world");
    let (datacenter_filter, set_datacenter_filter) = query_signal::<String>("datacenter");
    let (tax_enabled, set_tax_enabled) = query_signal::<bool>("tax");
    let (minimum_sales, set_minimum_sales) = query_signal::<usize>("sales");
    let (category_filter, set_category_filter) = query_signal::<i32>("category");
    let (max_purchase_price, set_max_purchase_price) = query_signal::<i32>("max-price");
    let (min_buy_price, set_min_buy_price) = query_signal::<i32>("min-buy");

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

    let (last_sold_within, set_last_sold_within) = query_signal::<String>("last-sold");
    let last_sold_duration =
        Memo::new(move |_| last_sold_within().and_then(|d| parse_duration(d.as_str()).ok()));
    let last_sold_string = Memo::new(move |_| {
        last_sold_duration()
            .map(|d| format_duration(d).to_string())
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
                let profit_per_day = data
                    .sale_summary
                    .avg_sale_duration
                    .map(|d| {
                        let days = d.num_seconds() as f32 / 86400.0;
                        let days = days.max(1.0);
                        (profit as f32 / days) as i32
                    })
                    .unwrap_or(0);
                CalculatedProfitData {
                    inner: data.clone(),
                    profit,
                    return_on_investment,
                    profit_per_day,
                }
            })
            .filter(move |data| {
                minimum_profit()
                    .map(|min| data.profit > min)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                minimum_profit_per_day()
                    .map(|min| data.profit_per_day > min)
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
                            .map(|item| item.item_search_category == cat_id)
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .filter(move |data| {
                max_purchase_price()
                    .map(|max| data.inner.cheapest_price <= max)
                    .unwrap_or(true)
            })
            .filter(move |data| {
                min_buy_price()
                    .map(|min| data.inner.cheapest_price >= min)
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
                last_sold_duration()
                    .map(|max_age| {
                        data.inner
                            .sale_summary
                            .days_since_last_sale
                            .and_then(|d| d.to_std().ok())
                            .map(|d| d <= max_age)
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
            SortMode::ProfitPerDay => sorted_data.sort_by_key(|data| Reverse(data.profit_per_day)),
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
                    title=t_string!(i18n, analyzer_minimum_profit).to_string()
                    description=t_string!(i18n, analyzer_minimum_profit_desc).to_string()
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
                    title=t_string!(i18n, analyzer_profit_per_day).to_string()
                    description=t_string!(i18n, analyzer_profit_per_day_desc).to_string()
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">
                            {move || {
                                minimum_profit_per_day()
                                    .map(|profit| Either::Left(view! { <Gil amount=profit /> }))
                                    .unwrap_or(Either::Right("---"))
                            }}
                        </div>
                        <input
                            class="input"
                            min=0
                            max=100000
                            step=1000
                            placeholder="e.g. 10000"
                            type="number"
                            prop:value=minimum_profit_per_day
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                if let Ok(profit) = value.parse::<i32>() {
                                    set_minimum_profit_per_day(Some(profit))
                                } else if value.is_empty() {
                                    set_minimum_profit_per_day(None);
                                }
                            }
                        />
                    </div>
                </FilterCard>

                <FilterCard
                    title=t_string!(i18n, analyzer_item_category).to_string()
                    description=t_string!(i18n, analyzer_item_category_desc).to_string()
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
                            <option value="">{t!(i18n, analyzer_all_categories)}</option>
                            {
                                let mut categories = tracked_data().item_search_categorys
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
                    title=t_string!(i18n, analyzer_minimum_sales).to_string()
                    description=t_string!(i18n, analyzer_minimum_sales_desc).to_string()
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
                    title=t_string!(i18n, analyzer_minimum_roi).to_string()
                    description=t_string!(i18n, analyzer_minimum_roi_desc).to_string()
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
                    title="Maximum Budget"
                    description="Set the maximum purchase price per item"
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">
                            {move || {
                                max_purchase_price()
                                    .map(|p| Either::Left(view! { <Gil amount=p /> }))
                                    .unwrap_or(Either::Right("---"))
                            }}
                        </div>
                        <input
                            class="input"
                            min=0
                            step=1000
                            placeholder="e.g. 500000"
                            type="number"
                            prop:value=max_purchase_price
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                if let Ok(p) = value.parse::<i32>() {
                                    set_max_purchase_price(Some(p));
                                } else if value.is_empty() {
                                    set_max_purchase_price(None);
                                }
                            }
                        />
                    </div>
                </FilterCard>

                <FilterCard
                    title=t_string!(i18n, analyzer_minimum_buy_price).to_string()
                    description=t_string!(i18n, analyzer_minimum_buy_price_desc).to_string()
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">
                            {move || {
                                min_buy_price()
                                    .map(|p| Either::Left(view! { <Gil amount=p /> }))
                                    .unwrap_or(Either::Right("---"))
                            }}
                        </div>
                        <input
                            class="input"
                            min=0
                            step=1000
                            placeholder="e.g. 5000"
                            type="number"
                            prop:value=min_buy_price
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                if let Ok(p) = value.parse::<i32>() {
                                    set_min_buy_price(Some(p));
                                } else if value.is_empty() {
                                    set_min_buy_price(None);
                                }
                            }
                        />
                    </div>
                </FilterCard>

                <FilterCard
                    title=t_string!(i18n, analyzer_sale_time_prediction).to_string()
                    description=t_string!(i18n, analyzer_sale_time_prediction_desc).to_string()
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
                    title=t_string!(i18n, analyzer_last_sold_within).to_string()
                    description=t_string!(i18n, analyzer_last_sold_within_desc).to_string()
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-brand-300">{last_sold_string}</div>
                        <input
                            class="input"
                            placeholder="e.g. 7d"
                            title="Accepts formats like 1h 30m, 7d, 1M (month), etc."
                            prop:value=move || last_sold_within().unwrap_or_default()
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                set_last_sold_within(Some(value))
                            }
                        />
                    </div>
                </FilterCard>

                <FilterCard
                    title=t_string!(i18n, analyzer_tax_calculation).to_string()
                    description=t_string!(i18n, analyzer_tax_calculation_desc).to_string()
                >
                    <div class="flex items-center">
                        <Toggle
                            checked=Signal::derive(move || tax_enabled().unwrap_or(true))
                            set_checked=SignalSetter::map(move |val: bool| set_tax_enabled(val.then_some(true)))
                            checked_label=Oco::Owned(t_string!(i18n, analyzer_tax_enabled).to_string())
                            unchecked_label=Oco::Owned(t_string!(i18n, analyzer_tax_disabled).to_string())
                        />
                    </div>
                </FilterCard>
            </div>

            // Results summary
            <div class="panel px-4 py-3 flex flex-col md:flex-row md:items-center gap-3 md:gap-0 md:justify-between">
                <div class="text-sm text-[color:var(--color-text)]">
                    <span class="text-brand-300 font-semibold">{move || sorted_data().len()}</span> {t!(i18n, analyzer_results)}
                </div>
                <div class="flex flex-wrap gap-2">
                    {move || {
                        let mut chips: Vec<_> = Vec::new();
                        if let Some(p) = minimum_profit() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_profit_gte)} <Gil amount=p />
                                    <button aria-label="Remove filter" class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_profit(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(p) = minimum_profit_per_day() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_profit_per_day_gte)} <Gil amount=p />
                                    <button aria-label="Remove filter" class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_profit_per_day(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(cat_id) = category_filter() {
                            let cat_name = tracked_data()
                                .item_search_categorys
                                .get(&xiv_gen::ItemSearchCategoryId(cat_id))
                                .map(|c| c.name.clone())
                                .unwrap_or_else(|| format!("Category {}", cat_id));
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_category_label)} {cat_name}
                                    <button aria-label="Remove filter" class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_category_filter(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(sales) = minimum_sales() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_sales_gte)} {sales}
                                    <button aria-label="Remove filter" class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_sales(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(roi) = minimum_roi() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_roi_gte)} {format!("{roi}%")}
                                    <button aria-label="Remove filter" class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_minimum_roi(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(p) = max_purchase_price() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    "Budget ≤ " <Gil amount=p />
                                    <button class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_max_purchase_price(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(p) = min_buy_price() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_min_buy_gte)} <Gil amount=p />
                                    <button aria-label="Remove filter" class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_min_buy_price(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(_ns) = max_predicted_time() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_next_sale_lte)} {predicted_time_string()}
                                    <button aria-label="Remove filter" class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_max_predicted_time(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if last_sold_within().is_some() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_last_sold_lte)} {last_sold_string()}
                                    <button aria-label="Remove filter" class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_last_sold_within(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(w) = world_filter() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_world_label)} {w.clone()}
                                    <button aria-label="Remove filter" class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_world_filter(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if let Some(dc) = datacenter_filter() {
                            chips.push(view! {
                                <span class="inline-flex items-center gap-2 rounded-full border px-3 py-1 text-sm text-[color:var(--color-text)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)] border-[color:var(--color-outline)]">
                                    {t!(i18n, analyzer_datacenter_label)} {dc.clone()}
                                    <button aria-label="Remove filter" class="ml-1 text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]" on:click=move |_| set_datacenter_filter(None)>
                                        <Icon icon=icondata::MdiClose />
                                    </button>
                                </span>
                            }.into_any());
                        }
                        if chips.is_empty() {
                            Either::Left(view! { <span class="text-sm text-[color:var(--color-text-muted)]">{t!(i18n, analyzer_no_active_filters)}</span> })
                        } else {
                            Either::Right(view! { <>{chips}</> })
                        }
                    }}
                </div>
                <button aria-label="Clear all filters" class="text-sm text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] self-start md:self-auto" on:click=move |_| {
                    set_minimum_profit(None);
                    set_minimum_profit_per_day(None);
                    set_minimum_roi(None);
                    set_max_predicted_time(None);
                    set_world_filter(None);
                    set_datacenter_filter(None);
                    set_minimum_sales(None);
                    set_category_filter(None);
                    set_max_purchase_price(None);
                    set_min_buy_price(None);
                    set_last_sold_within(None);
                }>
                    {t!(i18n, analyzer_clear_all)}
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
                                    {t!(i18n, analyzer_col_hq)}
                                </div>
                                <div role="columnheader" class="w-84 p-4">
                                    {t!(i18n, analyzer_col_item)}
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    <QueryButton
                                        class="!text-brand-300 hover:text-brand-200"
                                        active_classes="!text-[color:var(--brand-fg)] hover:!text-[color:var(--brand-fg)]"
                                        key="sort"
                                        value="profit"
                                    >
                                        <div class="flex items-center gap-2">
                                            {t!(i18n, analyzer_col_profit)}
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
                                        value="profit-per-day"
                                    >
                                        <div class="flex items-center gap-2">
                                            {t!(i18n, analyzer_col_profit_per_day)}
                                            {move || {
                                                (sort_mode() == Some(SortMode::ProfitPerDay))
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
                                            {t!(i18n, analyzer_col_roi)}
                                            {move || {
                                                (sort_mode() == Some(SortMode::Roi))
                                                    .then(|| view! { <Icon icon=i::BiSortDownRegular /> })
                                            }}
                                        </div>
                                    </QueryButton>
                                </div>
                                <div role="columnheader" class="w-30 p-4">
                                    {t!(i18n, analyzer_col_buy_price)}
                                </div>
                                <div role="columnheader" class="w-30 p-4 flex flex-row gap-2 hidden lg:flex">
                                    {t!(i18n, analyzer_col_world)}
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
                                    {t!(i18n, analyzer_col_datacenter)}
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
                                    {t!(i18n, analyzer_col_avg_sale_time)}
                                </div>
                                <div role="columnheader" class="w-30 p-4 hidden md:block">
                                    {t!(i18n, analyzer_col_last_sold)}
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
                                            Some(view! { <span class="px-2 py-0.5 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_14%,transparent)]">{t!(i18n, analyzer_col_hq)}</span> })
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
                                        <Gil amount=data.profit_per_day />
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
                                            t_string!(i18n, analyzer_only_show_world).to_string().replace("%world%", &world())
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
                                            t_string!(i18n, analyzer_only_show_world).to_string().replace("%world%", &datacenter())
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
                                    <div role="cell" class="px-4 py-2 w-30 truncate hidden md:block flex items-center">
                                        {data.inner
                                            .sale_summary
                                            .days_since_last_sale
                                            .and_then(|d| d.to_std().ok())
                                            .map(|d| {
                                                let secs = d.as_secs();
                                                let days = secs / 86_400;
                                                let hours = (secs % 86_400) / 3_600;
                                                if days > 0 { format!("{}d ago", days) }
                                                else if hours > 0 { format!("{}h ago", hours) }
                                                else { "just now".to_string() }
                                            })
                                            .unwrap_or_else(|| t_string!(i18n, analyzer_last_sold_never).to_string())}
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
    let i18n = use_i18n();
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
            <MetaTitle title=move || t_string!(i18n, analyzer_meta_title).to_string().replace("%world%", &world()) />
            <div class="container mx-auto max-w-7xl">
                <div class="flex flex-col gap-8">
                    // Header Section
                    <div class="panel p-4 sm:p-8 rounded-2xl">
                        <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-4">
                            {t!(i18n, analyzer_title_for)} {world}
                        </h1>
                        <div class="flex flex-col gap-4">
                            <MetaDescription text=move || {
                                t_string!(i18n, analyzer_meta_desc).to_string().replace("%world%", &world())
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
                                        checked_label=Oco::Owned(t_string!(i18n, analyzer_cross_region_enabled).to_string())
                                        unchecked_label=Oco::Owned(t_string!(i18n, analyzer_cross_region_disabled).to_string())
                                    />
                                    <Toggle
                                        checked=Signal::derive(move || {
                                            filter_outliers().unwrap_or_default()
                                        })
                                        set_checked=SignalSetter::map(move |val: bool| set_filter_outliers(
                                            val.then_some(true),
                                        ))
                                        checked_label=Oco::Owned(t_string!(i18n, analyzer_filter_outliers_enabled).to_string())
                                        unchecked_label=Oco::Owned(t_string!(i18n, analyzer_filter_outliers_disabled).to_string())
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
                                                                    checked_label=t_string!(i18n, analyzer_region_enabled).to_string().replace("%region%", region)
                                                                    unchecked_label=t_string!(i18n, analyzer_region_disabled).to_string().replace("%region%", region)
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
                                    href="?min-buy=5000&last-sold=7d&roi=30&sort=profit-per-day"
                                    label=t_string!(i18n, analyzer_preset_realistic).to_string()
                                />
                                <PresetFilterButton
                                    href="?min-buy=100000&last-sold=14d&roi=20&sort=profit"
                                    label=t_string!(i18n, analyzer_preset_big_ticket).to_string()
                                />
                                <PresetFilterButton
                                    href="?min-buy=1000&last-sold=3d&sort=profit-per-day"
                                    label=t_string!(i18n, analyzer_preset_volume).to_string()
                                />
                                <PresetFilterButton
                                    href="?min-buy=1000&last-sold=7d&roi=300&profit=0&sort=profit"
                                    label=t_string!(i18n, analyzer_preset_300_return).to_string()
                                />
                                <PresetFilterButton
                                    href="?min-buy=10000&last-sold=1M&roi=500&profit=200000"
                                    label=t_string!(i18n, analyzer_preset_500_return).to_string()
                                />
                                <PresetFilterButton
                                    href="?min-buy=1000&profit=100000"
                                    label=t_string!(i18n, analyzer_preset_100k_profit).to_string()
                                />
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
                                                    {t!(i18n, analyzer_failed_to_load)}
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
    let i18n = use_i18n();
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
                NavigateOptions {
                    scroll: false,
                    ..Default::default()
                },
            );
        }
    });

    view! {
        <div class="flex flex-col md:flex-row items-center gap-2">
            <label class="text-[color:var(--brand-fg)] font-semibold">{t!(i18n, analyzer_select_world)}</label>
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
    let i18n = use_i18n();
    view! {
        <MetaTitle title=t_string!(i18n, analyzer_index_meta_title).to_string() />
        <MetaDescription text=t_string!(i18n, analyzer_index_meta_desc).to_string() />

        <div class="main-content p-2 sm:p-6">
            <div class="container mx-auto max-w-7xl">
                <div class="flex flex-col gap-8">
                    // Hero Section
                    <div class="panel p-4 sm:p-8 rounded-2xl">
                        <h1 class="text-3xl font-bold text-[color:var(--brand-fg)] mb-4">
                            {t!(i18n, analyzer_index_title)}
                        </h1>
                        <p class="text-xl text-[color:var(--color-text)] leading-relaxed mb-6">
                            {t!(i18n, analyzer_index_desc_1)}
                        </p>
                        <p class="text-lg text-[color:var(--color-text)]/90 mb-8">
                            {t!(i18n, analyzer_index_desc_2)}
                        </p>

                        // World Selection
                        <div class="panel p-6 rounded-xl">
                            <h2 class="text-xl font-semibold text-[color:var(--brand-fg)] mb-4">
                                {t!(i18n, analyzer_index_choose_world)}
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
                            <h3 class="text-xl font-bold text-brand-300 mb-2">{t!(i18n, analyzer_feature_profit_tracking)}</h3>
                            <p class="text-gray-300">
                                {t!(i18n, analyzer_feature_profit_tracking_desc)}
                            </p>
                        </div>

                        <div class="card p-6 rounded-lg transition-colors duration-200">
                            <Icon
                                attr:class="text-brand-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaChartLineSolid
                            />
                            <h3 class="text-xl font-bold text-brand-300 mb-2">{t!(i18n, analyzer_feature_market_analysis)}</h3>
                            <p class="text-gray-300">
                                {t!(i18n, analyzer_feature_market_analysis_desc)}
                            </p>
                        </div>

                        <div class="card p-6 rounded-lg transition-colors duration-200">
                            <Icon
                                attr:class="text-brand-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaFilterSolid
                            />
                            <h3 class="text-xl font-bold text-brand-300 mb-2">{t!(i18n, analyzer_feature_custom_filters)}</h3>
                            <p class="text-gray-300">
                                {t!(i18n, analyzer_feature_custom_filters_desc)}
                            </p>
                        </div>
                    </div>

                    // Tips Section
                    <div class="panel p-6 rounded-2xl">
                        <h2 class="text-xl font-bold text-brand-300 mb-4">{t!(i18n, analyzer_tips_title)}</h2>
                        <ul class="list-disc list-inside text-gray-300 space-y-2">
                            <li>
                                {t!(i18n, analyzer_tip_1)}
                            </li>
                            <li>
                                {t!(i18n, analyzer_tip_2)}
                            </li>
                            <li>{t!(i18n, analyzer_tip_3)}</li>
                            <li>
                                {t!(i18n, analyzer_tip_4)}
                            </li>
                        </ul>
                    </div>
                </div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::recent_sales::{SaleData, Sales};

    fn sale(price: i32, days_ago: i64) -> Sales {
        let date = Utc::now()
            .naive_utc()
            .checked_sub_signed(Duration::days(days_ago))
            .unwrap();
        Sales {
            price_per_unit: price,
            sale_date: date,
        }
    }

    fn sales_row(item_id: i32, hq: bool, prices_and_days: &[(i32, i64)]) -> SaleData {
        SaleData {
            item_id,
            hq,
            sales: prices_and_days.iter().map(|(p, d)| sale(*p, *d)).collect(),
        }
    }

    #[test]
    fn median_price_is_middle_of_clamped_sales() {
        let row = sales_row(
            1,
            false,
            &[(100, 0), (110, 1), (120, 2), (130, 3), (140, 4), (150, 5)],
        );
        let summary = compute_summary(row, false);
        // Six even-length sample: median = (third + fourth) / 2 = (120 + 130) / 2 = 125
        assert_eq!(summary.median_price, 125);
    }

    #[test]
    fn sniper_sale_below_10pct_of_median_is_dropped() {
        // Raw median of [1, 100, 110, 120, 130, 140] sorted = (110+120)/2 = 115.
        // The "1" is well below 10% of 115 (=11), so it's dropped.
        let row = sales_row(
            2,
            false,
            &[(1, 0), (100, 1), (110, 2), (120, 3), (130, 4), (140, 5)],
        );
        let summary = compute_summary(row, false);
        // Median of remaining [100, 110, 120, 130, 140] = 120.
        assert_eq!(summary.median_price, 120);
        // min_price should also reflect the clamp, not the sniper.
        assert_eq!(summary.min_price, 100);
    }

    #[test]
    fn hq_prices_do_not_contaminate_nq_summary() {
        // An NQ row with normal prices. compute_summary no longer takes HQ context.
        let row = sales_row(
            3,
            false,
            &[(500, 0), (510, 1), (520, 2), (530, 3), (540, 4), (550, 5)],
        );
        let summary = compute_summary(row, false);
        assert_eq!(summary.min_price, 500);
        assert_eq!(summary.median_price, 525);
    }

    #[test]
    fn troll_region_floor_drops_row_entirely() {
        use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};
        use ultros_api_types::recent_sales::RecentSales;

        let sales = RecentSales {
            sales: vec![sales_row(
                100,
                false,
                &[
                    (1000, 0),
                    (1000, 1),
                    (1100, 2),
                    (1000, 3),
                    (1050, 4),
                    (1000, 5),
                ],
            )],
        };
        // Region cheapest = a troll 999,999,999 listing on a foreign world.
        let region = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: 100,
                hq: false,
                cheapest_price: 999_999_999,
                world_id: 42,
            }],
        };
        // Our own world has a sane cheapest at 1100.
        let world = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: 100,
                hq: false,
                cheapest_price: 1100,
                world_id: 1,
            }],
        };

        let table = ProfitTable::new(sales, region, world, vec![], false);
        // The troll 999M region listing should cause the row to be dropped entirely
        // (the displayed "deal" would be fictional). table.0 should be empty.
        assert_eq!(table.0.len(), 0);
    }

    #[test]
    fn troll_world_floor_falls_through_to_median() {
        use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};
        use ultros_api_types::recent_sales::RecentSales;

        // Sales settle at a stable median of 1000.
        let sales = RecentSales {
            sales: vec![sales_row(
                300,
                false,
                &[
                    (1000, 0),
                    (1000, 1),
                    (1000, 2),
                    (1000, 3),
                    (1000, 4),
                    (1000, 5),
                ],
            )],
        };
        // Region floor is sane (500 — below median, a real deal).
        let region = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: 300,
                hq: false,
                cheapest_price: 500,
                world_id: 42,
            }],
        };
        // Local world floor is a troll listing.
        let world = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: 300,
                hq: false,
                cheapest_price: 999_999_999,
                world_id: 1,
            }],
        };

        let table = ProfitTable::new(sales, region, world, vec![], false);
        // Row is kept (region floor is sane), but the troll world floor is ignored —
        // estimated_sale_price falls through to median, not the troll value.
        assert_eq!(table.0.len(), 1);
        assert_eq!(table.0[0].estimated_sale_price, 1000);
    }

    #[test]
    fn median_i32_odd_length() {
        // Direct unit test on the helper — exercises the n % 2 == 1 branch.
        assert_eq!(median_i32(&[100, 200, 300, 400, 500]), 300);
        assert_eq!(median_i32(&[100, 110, 120, 130, 140]), 120);
    }

    #[test]
    fn estimated_sale_price_uses_median_not_min() {
        use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};
        use ultros_api_types::recent_sales::RecentSales;

        let sales = RecentSales {
            sales: vec![sales_row(
                200,
                false,
                &[
                    (800, 0),
                    (1000, 1),
                    (1000, 2),
                    (1000, 3),
                    (1000, 4),
                    (1200, 5),
                ],
            )],
        };
        // Region floor is below median (a sane off-world deal).
        let region = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: 200,
                hq: false,
                cheapest_price: 700,
                world_id: 42,
            }],
        };
        // Local world floor is well above the median — the estimate should pin to median (=1000),
        // not min (=800) and not the world floor (=5000).
        let world = CheapestListings {
            cheapest_listings: vec![CheapestListingItem {
                item_id: 200,
                hq: false,
                cheapest_price: 5000,
                world_id: 1,
            }],
        };

        let table = ProfitTable::new(sales, region, world, vec![], false);
        assert_eq!(table.0.len(), 1);
        let row = &table.0[0];
        assert_eq!(row.sale_summary.median_price, 1000);
        assert_eq!(row.estimated_sale_price, 1000);
    }
}
