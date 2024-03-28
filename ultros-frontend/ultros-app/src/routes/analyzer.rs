use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        ad::Ad, clipboard::*, gil::*, item_icon::*, meta::*, query_button::QueryButton,
        skeleton::BoxSkeleton, toggle::Toggle, tooltip::*, virtual_scroller::*, world_picker::*,
    },
    error::AppError,
    global_state::LocalWorldData,
};
use chrono::{Duration, Utc};
use humantime::{format_duration, parse_duration};
use icondata as i;
use leptos::{oco::Oco, *};
use leptos_icons::*;
use leptos_router::*;
use log::info;
use std::{
    cmp::Reverse,
    collections::{hash_map::Entry, HashMap},
    fmt::Display,
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

#[derive(Clone, Debug)]
struct ProfitTable(Vec<ProfitData>);

/// Computes a summary of the sales data.
/// For non hq sale data, we also want to compare against HQ sale data
fn compute_summary(sale: SaleData, hq_data: Option<&SaleData>) -> SaleSummary {
    let now = Utc::now().naive_utc();
    let SaleData { item_id, hq, sales } = sale;
    let min_price = hq_data
        .map(|sales| sales.sales.iter())
        .into_iter()
        .flatten() // Turn Option<Iter=Sales> into just Iter=Sales.
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

impl ProfitTable {
    fn new(
        sales: RecentSales,
        region_listings: CheapestListings,
        world_listings: CheapestListings,
        cross_region: Vec<CheapestListings>,
    ) -> Self {
        let mut region_cheapest = listings_to_map(region_listings);
        let world_cheapest = listings_to_map(world_listings);
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
                // Use the world's price as
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

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
enum SortMode {
    Roi,
    Profit,
}

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

impl Display for SortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                SortMode::Roi => "roi",
                SortMode::Profit => "profit",
            }
        )
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

    // get ranges of possible values for our sliders

    let items = &xiv_gen_db::data().items;
    let (sort_mode, _set_sort_mode) = create_query_signal::<SortMode>("sort");
    let (minimum_profit, set_minimum_profit) = create_query_signal::<i32>("profit");
    let (minimum_roi, set_minimum_roi) = create_query_signal("roi");
    let (max_predicted_time, set_max_predicted_time) = create_query_signal::<String>("next-sale");
    let (world_filter, _set_world_filter) = create_query_signal::<String>("world");
    let (datacenter_filter, _set_datacenter_filter) = create_query_signal::<String>("datacenter");
    let world_clone = worlds.clone(); // cloned to pass into closure
    let world_filter_list = create_memo(move |_| {
        let world = world_filter().or_else(datacenter_filter)?;
        let filter = world_clone
            .lookup_world_by_name(&world)?
            .all_worlds()
            .map(|w| w.id)
            .collect::<Vec<_>>();
        Some(filter)
    });
    let world_clone = worlds.clone();
    let lookup_world = create_memo(move |_| {
        Some(AnySelector::from(
            &world_clone.lookup_world_by_name(&world())?,
        ))
    });
    let predicted_time =
        create_memo(move |_| max_predicted_time().and_then(|d| parse_duration(&d).ok()));
    let predicted_time_string = create_memo(move |_| {
        predicted_time()
            .map(|duration| format_duration(duration).to_string())
            .unwrap_or("---".to_string())
    });
    let sorted_data = create_memo(move |_| {
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
                // don't show listings from our own world
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
    const DATACENTER_WIDTH: &str = "width: 130px";
    const WORLD_WIDTH: &str = "width: 180px";
    view! {

       <div class="flex flex-col md:flex-row gap-2">
            <span>"filter:"</span><br/>
           <div class="flex-column">
                <label for>"minimum profit:"<br/>
               {move || minimum_profit().map(|profit| {
                    view!{<Gil amount=profit /> }
               }.into_view()).unwrap_or("---".into_view())}
               </label><br/>
               <input class="p-1 w-40" id="minimum_profit" min=0 max=100000 type="number" prop:value=minimum_profit
                    on:input=move |input| { let value = event_target_value(&input);
                        if let Ok(profit) = value.parse::<i32>() {
                            set_minimum_profit(Some(profit))
                        } else if value.is_empty() {
                            info!("clearing profit");
                            set_minimum_profit(None);
                        } }/>
           </div>
           <div class="flex-column">
                <label for="minimum_roi">"minimum ROI:"<br/>{move || minimum_roi().map(|roi| format!("{roi}%")).unwrap_or("---".to_string())}</label><br/>
               <input class="p-1 w-40"  min=0 max=100000 type="number" prop:value=minimum_roi
                on:input=move |input| {
                    let value = event_target_value(&input);
                    if let Ok(roi) = value.parse::<i32>() {
                        set_minimum_roi(Some(roi));
                    } else if value.is_empty() {
                        info!("clearing roi");
                        set_minimum_roi(None);
                    }} />

           </div>
           <div class="flex-column">
               <label for="predicted_time">"minimum time (use time notation, ex. 1w 30m):"<br/>{predicted_time_string}</label><br/>
               <input class="p-1 w-40" id="predicted_time" prop:value=move || max_predicted_time().unwrap_or_default() on:input=move |input| {
                    let value = event_target_value(&input);
                    set_max_predicted_time(Some(value))
                } />
           </div>
       </div>
       <div class="flex flex-col-reverse md:flex-row">
        <div class="grid-table" role="table">
            <div class="grid-header" role="rowgroup">
                <div role="columnheader" class="w-[25px]">"HQ"</div>
                <div role="columnheader first" class="w-[450px]">"Item"</div>
                <div role="columnheader" style="width:100px;">
                    <Tooltip tooltip_text=Oco::from("Sort by profit")>
                        <QueryButton class="!text-fuchsia-300 hover:text-fuchsia-200" active_classes="!text-neutral-300 hover:text-neutral-200" query_name="sort" value="profit">
                            <div class="flex-row flex-space">
                                "Profit" {move || (sort_mode() == Some(SortMode::Profit)).then(|| { view!{<Icon icon=i::BiSortDownRegular /> }})}
                            </div>
                        </QueryButton>
                    </Tooltip>
                </div>
                <div role="columnheader" style="width:100px;">
                    <Tooltip tooltip_text=Oco::from("Sort by R.O.I")>
                        <QueryButton class="!text-fuchsia-300 hover:text-fuchsia-200" active_classes="!text-neutral-300 hover:text-neutral-200" query_name="sort" value="roi" default=true>
                            <div class="flex-row flex-space">
                                "R.O.I." {move || (sort_mode() == Some(SortMode::Roi)).then(|| { view!{<Icon icon=i::BiSortDownRegular /> }})}
                            </div>
                        </QueryButton>
                    </Tooltip>
                </div>
                <div role="columnheader" style=WORLD_WIDTH>
                    "World" <QueryButton query_name="world" value="" class="!text-fuchsia-300 hover:text-fuchsia-200" active_classes="hidden"><Tooltip tooltip_text=Oco::from("Clear this world filter")>{move || ["[", &world_filter().unwrap_or_default(), "]"].concat()}</Tooltip></QueryButton>
                </div>
                <div role="columnheader" style=DATACENTER_WIDTH>
                    "Datacenter" <QueryButton query_name="datacenter" value="" class="!text-fuchsia-300 hover:text-fuchsia-200" active_classes="hidden"><Tooltip tooltip_text=Oco::from("Clear this datacenter filter")>{move || ["[", &datacenter_filter().unwrap_or_default(), "]"].concat()}</Tooltip></QueryButton>
                </div>
                <div role="columnheader" style="width: 300px;">"Next sale"</div>
            </div>
            <VirtualScroller
                viewport_height=1000.0
                row_height=32.3333
                each=sorted_data.into()
                key=move |(i, data)| {
                    (*i, data.sale_summary.item_id, data.cheapest_world_id, data.sale_summary.hq)
                }
                view=move |(i, data)| {
                    let world = worlds.lookup_selector(AnySelector::World(data.cheapest_world_id));
                    let datacenter = world
                        .as_ref()
                        .and_then(|world| {
                            let datacenters = worlds.get_datacenters(world);
                            datacenters.first().map(|dc| dc.name.as_str())
                        })
                        .unwrap_or_default()
                        .to_string();
                    let world = world
                        .as_ref()
                        .map(|r| r.get_name())
                        .unwrap_or_default()
                        .to_string();
                    let world_event = world.clone();
                    let datacenter_event = datacenter.clone();
                    let item_id = data.sale_summary.item_id;
                    let item = items
                        .get(&ItemId(item_id))
                        .map(|item| item.name.as_str())
                        .unwrap_or_default();
                    view! {<div class="grid-row" role="row-group" class:even=move || (i % 2) == 0 class:odd=move || (i % 2) == 1>
                        <div role="cell" style="width: 25px;">{data.sale_summary.hq.then_some("✅")}</div>
                        <div role="cell" class="flex flex-row w-[450px]">
                            <a class="flex flex-row" href=format!("/item/{world}/{item_id}")>
                                <ItemIcon item_id icon_size=IconSize::Small/>
                                {item}
                            </a>
                            <Clipboard clipboard_text=item.to_string()/>
                        </div>
                        <div role="cell" style="width: 100px;"><Gil amount=data.profit /></div>
                        <div role="cell" style="width: 100px;">{data.return_on_investment}"%"</div>
                        <div role="cell" style=WORLD_WIDTH><Gil amount=data.cheapest_price/>" on "<QueryButton query_name="world" value=world_event class="!text-fuchsia-300" active_classes="!text-neutral-300 hover:text-neutral-200" remove_queries=&["datacenter"]>{&world}</QueryButton></div>
                        <div role="cell" style=DATACENTER_WIDTH><QueryButton query_name="datacenter" value=datacenter_event class="!text-fuchsia-300" active_classes="!text-neutral-300 hover:text-neutral-200" remove_queries=&["world"]>{&datacenter}</QueryButton></div>
                        <div role="cell" style="width: 300px;">{data.sale_summary
                                .avg_sale_duration
                                .and_then(|sale_duration| {
                                    let duration = sale_duration.to_std().ok()?;
                                    Some(format_duration(duration).to_string())
                                })
                        }</div>
                        </div>}
                }/>
        </div>
       </div>
    }
}

#[component]
pub fn AnalyzerWorldView() -> impl IntoView {
    let params = use_params_map();
    let world = create_memo(move |_| params.with(|p| p.get("world").cloned()).unwrap_or_default());
    let sales = create_resource(
        move || params.with(|p| p.get("world").cloned()),
        move |world| async move {
            get_recent_sales_for_world(&world.ok_or(AppError::ParamMissing)?).await
        },
    );

    let world_cheapest_listings = create_resource(
        move || params.with(|p| p.get("world").cloned()),
        move |world| async move {
            let world = world.ok_or(AppError::ParamMissing)?;
            get_cheapest_listings(&world).await
        },
    );

    let region = move || {
        let worlds = use_context::<LocalWorldData>()
            .expect("Worlds should always be populated here")
            .0
            .unwrap();
        let world = params.with(|p| p.get("world").cloned());
        // use the world cache to lookup the region for this world
        let world = world.ok_or(AppError::ParamMissing)?;
        let region = worlds
            .lookup_world_by_name(&world)
            .map(|world| {
                let region = worlds.get_region(world);
                AnyResult::Region(region).get_name().to_string()
            })
            .ok_or(AppError::ParamMissing)?;
        Result::<_, AppError>::Ok(region)
    };
    let global_cheapest_listings = create_resource(
        move || region(),
        move |region| async move { get_cheapest_listings(&region?).await },
    );
    let (cross_region_enabled, set_cross_region_enabled) = create_query_signal::<bool>("cross");
    let connected_regions = &["Europe", "Japan", "North-America", "Oceania"];
    let cross_region = create_resource(
        move || (cross_region_enabled(), region()),
        move |(enabled, region)| {
            async move {
                let region = region?;
                if enabled.unwrap_or_default() && connected_regions.contains(&region.as_str()) {
                    // get all regions except our current region
                    Ok(futures::future::join_all(
                        connected_regions
                            .into_iter()
                            .filter(|r| **r != region.as_str())
                            .map(|region| get_cheapest_listings(region)),
                    )
                    .await
                    .into_iter()
                    .filter_map(|l| l.ok())
                    .collect())
                } else {
                    Ok(vec![])
                }
            }
        },
    );
    view!{
        <div class="main-content">
            <div class="container mx-auto flex flex-col">
                <div class="flex flex-col md:flex-row">
                    <div class="flex flex-col">
                        <span class="title">"Resale Analyzer Results for "{world}</span><br/>
                        <MetaTitle title=move || format!("Price Analyzer - {}", world())/>
                        <MetaDescription text=move || format!("The analyzer enables FFXIV merchants to find the best items to buy on other worlds and sell on {}. Filter for the best profits or return, make gil through market arbitrage.", world())/>
                        <AnalyzerWorldNavigator /><br />
                        <Toggle checked=Signal::derive(move || cross_region_enabled().unwrap_or_default()) set_checked=SignalSetter::map(move |val: bool| set_cross_region_enabled(val.then(|| true))) checked_label=Oco::Borrowed("Cross region enabled") unchecked_label=Oco::Borrowed("Cross region disabled") />
                        <span>"The analyzer will show items that sell more on "{world}" than they can be purchased for, enabling market arbitrage."</span><br/>
                        <span>"These estimates aren't very accurate, but are meant to be easily accessible and fast to use."</span><br/>
                        <span>"Be extra careful to make sure that the price you buy things for matches"</span><br/>
                        <span>"Sample filters"</span>
                        <div class="flex flex-col md:flex-row flex-wrap">
                            <a class="btn p-1" href="?next-sale=7d&roi=300&profit=0&sort=profit&">"300% return - 7 days"</a>
                            <a class="btn p-1" href="?next-sale=1M&roi=500&profit=200000&">"500% return - 200K min profit - 1 month"</a>
                            <a class="btn p-1" href="?profit=100000">"100K profit"</a>
                        </div>
                    </div>
                    <Ad class="h-20" />
                </div>
                <div class="min-h-screen w-full">
                <Suspense fallback=BoxSkeleton>
                {move || {
                    let world_cheapest = world_cheapest_listings.get();
                    let sales = sales.get();
                    let global_cheapest_listings = global_cheapest_listings.get();
                    let cross_region = cross_region.get().and_then(|r: Result<_, AppError>| r.ok()).unwrap_or_default();
                    let worlds = use_context::<LocalWorldData>()
                    .expect("Worlds should always be populated here")
                    .0
                    .unwrap();
                    let values = world_cheapest
                        .and_then(|w| w.ok())
                        .and_then(|r| sales.and_then(|s| s.ok())
                        .and_then(|s| global_cheapest_listings.and_then(|g| g.ok()).map(|g| (r, s, g))));
                    match values {
                        Some((world_cheapest_listings, sales, global_cheapest_listings)) => {view!{<AnalyzerTable sales global_cheapest_listings world_cheapest_listings cross_region worlds world=world.into() />}.into_view()},
                        None => {view!{
                            <div class="h3">
                                "Failed to load analyzer - try again in 30 seconds"
                            </div>
                        }.into_view()}
                    }
                }}
                </Suspense>
                </div>
        </div>
    </div>}.into_view()
}

#[component]
pub fn AnalyzerWorldNavigator() -> impl IntoView {
    let nav = use_navigate();
    let params = use_params_map();
    let worlds = use_context::<LocalWorldData>()
        .expect("Should always have local world data")
        .0
        .unwrap();
    let initial_world = params.with_untracked(|p| {
        let world = p.get("world").map(|s| s.as_str()).unwrap_or_default();
        worlds
            .lookup_world_by_name(world)
            .and_then(|w| w.as_world().cloned())
    });
    info!("{initial_world:?}");
    let (current_world, set_current_world) = create_signal(initial_world);
    let query = use_query_map();
    create_effect(move |_| {
        if let Some(world) = current_world() {
            let world = world.name;
            let query_map = query.get_untracked();
            let query = serde_qs::to_string(&query_map).unwrap();
            nav(
                &format!("/analyzer/{world}?{query}"),
                NavigateOptions::default(),
            );
        }
    });
    view! {<label>"Analyzer World: "</label><WorldOnlyPicker current_world=current_world.into() set_current_world=set_current_world.into() />}
}

#[component]
pub fn Analyzer() -> impl IntoView {
    view! {
        <MetaTitle title="Analyzer - Ultros"/>
        <MetaDescription text="Find items on the Final Fantasy 14 marketboard that are great for resale. Used to earn gil quickly."/>
        <div class="main-content">
            <div class="mx-auto container">
                <span class="content-title">"Analyzer"</span>
                <div class="flex-column">
                    <span>"The analyzer helps find items on the Final Fantasy 14 marketboard that are cheaper on other worlds that sell for more on your world, enabling you to earn gil through market arbitrage."</span><br/>
                    <span>"Adjust parameters to try and find items that sell well"</span><br/>
                    "Choose a world to get started:"<br/>
                    <AnalyzerWorldNavigator />
                </div>
            </div>
        </div>
    }
}
