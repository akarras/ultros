use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        add_to_list::AddToList, clipboard::*, gil::*, item_icon::*, meta::*,
        query_button::QueryButton, skeleton::BoxSkeleton, toggle::Toggle, tooltip::*,
        virtual_scroller::*, world_picker::*,
    },
    error::AppError,
    global_state::LocalWorldData,
};
use chrono::{Duration, Utc};
use hooks::{query_signal, use_navigate, use_params_map, use_query_map};
use humantime::{format_duration, parse_duration};
use icondata as i;
use leptos::{either::Either, prelude::*, reactive::wrappers::write::SignalSetter};
use leptos_icons::*;
use leptos_meta::Title;
use leptos_router::*;
use std::{
    cmp::Reverse,
    collections::{hash_map::Entry, HashMap},
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
}

#[derive(Hash, Clone, Debug, PartialEq, Eq)]
struct ProfitKey {
    item_id: i32,
    hq: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct ProfitData {
    profit: i32,
    return_on_investment: i32,
    cheapest_price: i32,
    cheapest_world_id: i32,
    sale_summary: SaleSummary,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum SortMode {
    Roi,
    Profit,
}

#[derive(Clone, Debug)]
struct ProfitTable(Vec<ProfitData>);

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

fn compute_summary(sale: SaleData, hq_data: Option<&SaleData>) -> SaleSummary {
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
    let avg_price = (sales
        .iter()
        .map(|price| price.price_per_unit as i64)
        .sum::<i64>()
        / sales.len() as i64) as i32;
    let t = sales
        .last()
        .map(|last| (last.sale_date - now).num_milliseconds().abs() / sales.len() as i64);
    let avg_sale_duration = t.map(Duration::milliseconds);
    SaleSummary {
        item_id,
        hq,
        num_sold: sales.len(),
        avg_sale_duration,
        max_price,
        avg_price,
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
            _ => Err(()),
        }
    }
}

impl ToString for SortMode {
    fn to_string(&self) -> String {
        match self {
            SortMode::Roi => "roi".to_string(),
            SortMode::Profit => "profit".to_string(),
        }
    }
}

impl ProfitTable {
    fn new(
        sales: RecentSales,
        global_cheapest_listings: CheapestListings,
        world_cheapest_listings: CheapestListings,
        cross_region: Vec<CheapestListings>,
    ) -> Self {
        let mut region_cheapest = listings_to_map(global_cheapest_listings);
        let world_cheapest = listings_to_map(world_cheapest_listings);
        let cross_region = cross_region
            .into_iter()
            .map(|region| listings_to_map(region));

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
                let summary =
                    compute_summary(sale, (!hq).then(|| hq_sales.get(&item_id)).flatten());

                // Use the world's price as estimated sale price
                let estimated_sale_price =
                    if let Some((world_cheapest, _)) = world_cheapest.get(&key) {
                        summary.min_price.min(*world_cheapest)
                    } else {
                        summary.min_price
                    };

                Some(ProfitData {
                    profit: estimated_sale_price - cheapest_price,
                    return_on_investment: ((estimated_sale_price - cheapest_price) as f32
                        / cheapest_price as f32
                        * 100.0) as i32,
                    sale_summary: summary,
                    cheapest_world_id,
                    cheapest_price,
                })
            })
            .collect();

        ProfitTable(table)
    }
}

#[component]
fn FilterCard<T>(
    #[prop(into)] title: Oco<'static, str>,
    #[prop(into)] description: Oco<'static, str>,
    children: TypedChildren<T>,
) -> impl IntoView
where
    T: IntoView,
{
    view! {
        <div class="p-6 flex flex-col rounded-2xl
        backdrop-blur-sm backdrop-brightness-110
        border border-white/10
        bg-gradient-to-br from-violet-900/20 via-black/10 to-amber-500/10
        w-full">
            <h3 class="font-bold text-xl text-amber-200 mb-2">{title}</h3>
            <p class="text-gray-300 mb-4">{description}</p>
            {children.into_inner()().into_view()}
        </div>
    }
}

#[component]
fn PresetFilterButton(href: &'static str, label: &'static str) -> impl IntoView {
    view! {
        <a
            href=href
            class="px-4 py-2 rounded-lg bg-violet-900/30 hover:bg-violet-800/40
             border border-white/10 hover:border-yellow-200/30
             transition-all duration-300 text-amber-200 hover:text-amber-100
             hover:transform hover:scale-[1.02] hover:shadow-lg hover:shadow-violet-500/10"
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
) -> impl IntoView {
    let profits = ProfitTable::new(
        sales,
        global_cheapest_listings,
        world_cheapest_listings,
        cross_region,
    );

    let items = &xiv_gen_db::data().items;
    let (sort_mode, _set_sort_mode) = query_signal::<SortMode>("sort");
    let (minimum_profit, set_minimum_profit) = query_signal::<i32>("profit");
    let (minimum_roi, set_minimum_roi) = query_signal("roi");
    let (max_predicted_time, set_max_predicted_time) = query_signal::<String>("next-sale");
    let (world_filter, set_world_filter) = query_signal::<String>("world");
    let (datacenter_filter, set_datacenter_filter) = query_signal::<String>("datacenter");

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
        let mut sorted_data = profits
            .0
            .iter()
            .cloned()
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
                predicted_time()
                    .map(|time| {
                        data.sale_summary
                            .avg_sale_duration
                            .map(|dur| dur.to_std().ok().map(|dur| dur < time).unwrap_or(false))
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .filter(move |data| {
                world_filter_list()
                    .map(|world_filter| world_filter.contains(&data.cheapest_world_id))
                    .unwrap_or(true)
            })
            .filter(move |data| {
                data.cheapest_world_id
                    != lookup_world()
                        .and_then(|w| w.as_world_id())
                        .unwrap_or_default()
            })
            .collect::<Vec<_>>();

        match sort_mode().unwrap_or(SortMode::Roi) {
            SortMode::Roi => sorted_data.sort_by_key(|data| Reverse(data.return_on_investment)),
            SortMode::Profit => sorted_data.sort_by_key(|data| Reverse(data.profit)),
        }
        sorted_data.into_iter().enumerate().collect()
    });
    view! {
        <div class="flex flex-col gap-6">
            <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-6">
                <FilterCard
                    title="Minimum Profit"
                    description="Set the minimum profit margin you want to see"
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-amber-200">
                            {move || {
                                minimum_profit()
                                    .map(|profit| Either::Left(view! { <Gil amount=profit /> }))
                                    .unwrap_or(Either::Right("---"))
                            }}
                        </div>
                        <input
                            class="p-2 rounded-lg bg-violet-950/50 border border-white/10 w-full
                             focus:outline-none focus:border-yellow-200/30 transition-colors"
                            min=0
                            max=100000
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
                    title="Minimum ROI"
                    description="Set the minimum return on investment percentage"
                >
                    <div class="flex flex-col gap-2">
                        <div class="text-amber-200">
                            {move || {
                                minimum_roi()
                                    .map(|roi| format!("{roi}%"))
                                    .unwrap_or("---".to_string())
                            }}
                        </div>
                        <input
                            class="p-2 rounded-lg bg-violet-950/50 border border-white/10 w-full
                             focus:outline-none focus:border-yellow-200/30 transition-colors"
                            min=0
                            max=100000
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
                        <div class="text-amber-200">{predicted_time_string}</div>
                        <input
                            class="p-2 rounded-lg bg-violet-950/50 border border-white/10 w-full
                             focus:outline-none focus:border-yellow-200/30 transition-colors"
                            prop:value=move || max_predicted_time().unwrap_or_default()
                            on:input=move |input| {
                                let value = event_target_value(&input);
                                set_max_predicted_time(Some(value))
                            }
                        />
                    </div>
                </FilterCard>
            </div>

            // Results table
            <div class="rounded-2xl overflow-x-auto overflow-y-hidden border border-white/10 backdrop-blur-sm backdrop-brightness-110">
                <div class="grid-table" role="table">
                    <div class="flex flex-row align-top h-20 bg-violet-900/30" role="rowgroup">
                        <div role="columnheader" class="w-[25px] p-4">
                            "HQ"
                        </div>
                        <div role="columnheader" class="w-84 p-4">
                            "Item"
                        </div>
                        <div role="columnheader" class="w-30 p-4">
                            <QueryButton
                                class="!text-amber-300 hover:text-amber-200"
                                active_classes="!text-neutral-300 hover:text-neutral-200"
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
                                class="!text-amber-300 hover:text-amber-200"
                                active_classes="!text-neutral-300 hover:text-neutral-200"
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
                        <div role="columnheader" class="w-30 p-4 flex flex-row gap-2">
                            "World"
                            <div>
                            {move || {
                                world_filter().map(|_filter| {
                                    view!{
                                        <div class="hover:text-amber:200 transition-colors rounded-sm p-2 text-amber-300 cursor-pointer" on:click=move |_| {
                                            set_world_filter(None);
                                        }>
                                            <Icon icon=icondata::MdiFilterRemove />
                                        </div>
                                    }
                                }) 
                            }}
                            </div>
                        </div>
                        <div role="columnheader" class="w-30 p-4 flex flex-row gap-2">
                            "Datacenter"
                            <div>
                            {move || {
                                datacenter_filter().map(|_filter| {
                                    view!{
                                        <div class="hover:text-amber:200 transition-colors rounded-sm p-2 text-amber-300 cursor-pointer" on:click=move |_| {
                                            set_datacenter_filter(None);
                                        }>
                                            <Icon icon=icondata::MdiFilterRemove />
                                        </div>
                                    }
                                }) 
                            }}
                            </div>
                        </div>
                        <div role="columnheader" class="w-30 p-4">
                            "Avg Sale Time"
                        </div>
                    </div>

                    <VirtualScroller
                        viewport_height=1000.0
                        row_height=48.0
                        each=sorted_data.into()
                        key=move |(i, data)| (
                            *i,
                            data.sale_summary.item_id,
                            data.cheapest_world_id,
                            data.sale_summary.hq,
                        )
                        view=move |(i, data)| {
                            let world = worlds
                                .lookup_selector(AnySelector::World(data.cheapest_world_id));
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
                            let item_id = data.sale_summary.item_id;
                            let item = items
                                .get(&ItemId(item_id))
                                .map(|item| item.name.as_str())
                                .unwrap_or_default();
                            // if even
                            let classes = if (i % 2) == 0 {
                                "flex flex-row flex-nowrap h-10 hover:bg-violet-700/20 bg-violet-900/20 transition-colors"
                            } else {
                                "flex flex-row flex-nowrap h-10 hover:bg-violet-700/20 bg-violet-800/20 transition-colors"
                            };
                            view! {
                                <div
                                    class=classes
                                    role="row-group"
                                >
                                    <div role="cell" class="p-4 w-[25px]">
                                        {data.sale_summary.hq.then_some("✅")}
                                    </div>
                                    <div
                                        role="cell"
                                        class="p-4 flex flex-row w-84 items-center gap-2"
                                    >
                                        <a
                                            class="flex flex-row items-center gap-2 hover:text-amber-200 transition-colors truncate overflow-x-clip w-full"
                                            href=format!("/item/{}/{item_id}", world())
                                        >
                                            <div class="shrink-0"><ItemIcon item_id icon_size=IconSize::Small /></div>
                                            {item}
                                        </a>
                                        <AddToList item_id />
                                        <Clipboard clipboard_text=item.to_string() />
                                    </div>
                                    <div role="cell" class="p-4 w-30 text-right">
                                        <Gil amount=data.profit />
                                    </div>
                                    <div role="cell" class="p-4 w-30 text-right">
                                        {data.return_on_investment}
                                        "%"
                                    </div>
                                    <div role="cell" class="p-4 w-30 text-right">
                                        <Gil amount=data.cheapest_price />
                                    </div>
                                    <div role="cell" class="p-4 w-30">
                                        <Tooltip tooltip_text=Signal::derive(move || {
                                            format!("Only show {}", world())
                                        })>
                                            <QueryButton
                                                key="world"
                                                value=world.clone()
                                                class="!text-amber-300 hover:text-amber-200"
                                                active_classes="!text-neutral-300 hover:text-neutral-200"
                                                remove_queries=&["datacenter"]
                                            >
                                                {world}
                                            </QueryButton>
                                        </Tooltip>
                                    </div>
                                    <div role="cell" class="p-4 w-30">
                                        <Tooltip tooltip_text=Signal::derive(move || {
                                            format!("Only show {}", datacenter())
                                        })>
                                            <QueryButton
                                                key="datacenter"
                                                value=datacenter.clone()
                                                class="!text-amber-300 hover:text-amber-200"
                                                active_classes="!text-neutral-300 hover:text-neutral-200"
                                                remove_queries=&["world"]
                                            >
                                                {datacenter}
                                            </QueryButton>
                                        </Tooltip>
                                    </div>
                                    <div role="cell" class="p-4 w-30 truncate">
                                        {data
                                            .sale_summary
                                            .avg_sale_duration
                                            .and_then(|duration| duration.to_std().ok())
                                            .map(|duration| format_duration(duration).to_string())
                                            .unwrap_or_else(|| "---".to_string())}
                                    </div>
                                </div>
                            }
                                .into_any()
                        }
                    />
                </div>
            </div>
        </div>
    }.into_any()
}

#[component]
pub fn AnalyzerWorldView() -> impl IntoView {
    let params = use_params_map();
    let world = Memo::new(move |_| params.with(|p| p.get("world").clone()).unwrap_or_default());
    let sales = Resource::new(
        move || params.with(|p| p.get("world").clone()),
        move |world| async move {
            get_recent_sales_for_world(&world.ok_or(AppError::ParamMissing)?).await
        },
    );

    let world_cheapest_listings = Resource::new(
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

    let global_cheapest_listings = Resource::new(
        move || region(),
        move |region| async move { get_cheapest_listings(region?.as_str()).await },
    );

    let (cross_region_enabled, set_cross_region_enabled) = query_signal::<bool>("cross");
    let connected_regions = &["Europe", "Japan", "North-America", "Oceania"];
    let query = use_query_map();

    let enabled_regions = move || {
        let map = query();
        connected_regions
            .into_iter()
            .filter(|region| map.get(region).map(|value| value == "true").unwrap_or(true))
            .collect::<Vec<_>>()
    };

    let cross_region = Resource::new(
        move || (cross_region_enabled(), region(), enabled_regions()),
        move |(enabled, region, enabled_regions)| async move {
            let region = region?;
            if enabled.unwrap_or_default() && connected_regions.contains(&region.as_str()) {
                Ok(futures::future::join_all(
                    connected_regions
                        .into_iter()
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
        <div class="main-content p-6">
            <Title text=move || format!("Analyzer - {}", world()) />
            <div class="container mx-auto max-w-7xl">
                <div class="flex flex-col gap-8">
                    // Header Section
                    <div class="bg-gradient-to-br from-violet-900/30 to-amber-500/20
                    rounded-2xl p-8 border border-white/10 backdrop-blur-sm">
                        <h1 class="text-3xl font-bold text-amber-200 mb-4">
                            "Market Analysis for " {world}
                        </h1>
                        <div class="flex flex-col gap-4">
                            <MetaTitle title=move || format!("Price Analyzer - {}", world()) />
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
                                            val.then(|| true),
                                        ))
                                        checked_label=Oco::Borrowed("Cross region enabled")
                                        unchecked_label=Oco::Borrowed("Cross region disabled")
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
                                                        .into_iter()
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
                                                />
                                            },
                                        )
                                    }
                                    _ => {
                                        Either::Right(
                                            view! {
                                                <div class="text-xl text-amber-200 text-center p-8
                                                 bg-violet-900/20 rounded-2xl border border-white/10">
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
                &format!("/analyzer/{world}?{query}"),
                NavigateOptions::default(),
            );
        }
    });

    view! {
        <div class="flex flex-col md:flex-row items-center gap-2">
            <label class="text-amber-200 font-medium">"Select World:"</label>
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
        <MetaTitle title="Analyzer - Ultros" />
        <MetaDescription text="Find items on the Final Fantasy 14 marketboard that are great for resale. Used to earn gil quickly." />

        <div class="main-content p-6">
            <div class="container mx-auto max-w-7xl">
                <div class="flex flex-col gap-8">
                    // Hero Section
                    <div class="bg-gradient-to-br from-violet-900/30 to-amber-500/20
                    rounded-2xl p-8 border border-white/10 backdrop-blur-sm">
                        <h1 class="text-3xl font-bold text-amber-200 mb-4">
                            "Market Board Analyzer"
                        </h1>
                        <p class="text-xl text-gray-300 leading-relaxed mb-6">
                            "The analyzer helps find items on the Final Fantasy 14 marketboard that are
                             cheaper on other worlds that sell for more on your world, enabling you to
                             earn gil through market arbitrage."
                        </p>
                        <p class="text-lg text-gray-400 mb-8">
                            "Adjust parameters to find items that sell well and maximize your profits."
                        </p>

                        // World Selection
                        <div class="bg-black/20 rounded-xl p-6 border border-white/5">
                            <h2 class="text-xl font-medium text-amber-200 mb-4">
                                "Choose a world to get started:"
                            </h2>
                            <AnalyzerWorldNavigator />
                        </div>
                    </div>

                    // Features Grid
                    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
                        <div class="p-6 rounded-2xl bg-violet-900/20 border border-white/10
                        backdrop-blur-sm">
                            <Icon
                                attr:class="text-amber-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaMoneyBillTrendUpSolid
                            />
                            <h3 class="text-xl font-bold text-amber-200 mb-2">"Profit Tracking"</h3>
                            <p class="text-gray-300">
                                "Monitor profit margins and ROI across different worlds"
                            </p>
                        </div>

                        <div class="p-6 rounded-2xl bg-violet-900/20 border border-white/10
                        backdrop-blur-sm">
                            <Icon
                                attr:class="text-amber-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaChartLineSolid
                            />
                            <h3 class="text-xl font-bold text-amber-200 mb-2">"Market Analysis"</h3>
                            <p class="text-gray-300">
                                "Track market trends and identify profitable opportunities"
                            </p>
                        </div>

                        <div class="p-6 rounded-2xl bg-violet-900/20 border border-white/10
                        backdrop-blur-sm">
                            <Icon
                                attr:class="text-amber-300 mb-4"
                                width="2.5em"
                                height="2.5em"
                                icon=i::FaFilterSolid
                            />
                            <h3 class="text-xl font-bold text-amber-200 mb-2">"Custom Filters"</h3>
                            <p class="text-gray-300">
                                "Set custom parameters to find your perfect trades"
                            </p>
                        </div>
                    </div>

                    // Tips Section
                    <div class="bg-violet-900/20 rounded-2xl p-6 border border-white/10">
                        <h2 class="text-xl font-bold text-amber-200 mb-4">"Trading Tips"</h2>
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
