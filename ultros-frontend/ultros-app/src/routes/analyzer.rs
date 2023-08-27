use chrono::{Duration, Utc};
use humantime::{format_duration, parse_duration};
use leptos::*;
use leptos_router::*;
use log::info;
use std::{cmp::Reverse, collections::HashMap, fmt::Display, str::FromStr, sync::Arc};
use ultros_api_types::{
    cheapest_listings::CheapestListings,
    recent_sales::{RecentSales, SaleData},
    world_helper::{AnyResult, AnySelector, WorldHelper},
};
use xiv_gen::ItemId;

use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{
        clipboard::*, gil::*, item_icon::*, meta::*, tooltip::*, virtual_scroller::*,
        world_picker::*,
    },
    error::AppError,
    global_state::LocalWorldData,
};

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
    ) -> Self {
        let region_cheapest = listings_to_map(region_listings);
        let world_cheapest = listings_to_map(world_listings);
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

fn use_query_item<T>(parameter: &'static str) -> (Signal<Option<T>>, SignalSetter<Option<T>>)
where
    T: FromStr + ToString + PartialEq,
{
    let router = use_router();
    let query_map = use_query_map();

    let read = create_memo(move |_| {
        query_map.with(|query| query.get(parameter).and_then(|s| s.parse().ok()))
    });
    let navigate = use_navigate();
    let set = move |value: Option<T>| {
        let mut query_map = query_map();
        let path_name = router.pathname()();
        match value {
            Some(value) => {
                query_map.insert(parameter.to_string(), value.to_string());
            }
            None => {
                query_map.remove(parameter);
            }
        }
        let query_string = query_map.to_query_string();

        navigate(
            &format!("{path_name}{query_string}"),
            NavigateOptions {
                resolve: false,
                replace: true,
                scroll: true,
                state: State(None),
            },
        )
    };
    (read.into(), set.mapped_signal_setter())
}

#[component]
fn AnalyzerTable(
    sales: RecentSales,
    global_cheapest_listings: CheapestListings,
    world_cheapest_listings: CheapestListings,
    worlds: Arc<WorldHelper>,
    world: Signal<String>,
) -> impl IntoView {
    let profits = ProfitTable::new(sales, global_cheapest_listings, world_cheapest_listings);

    // get ranges of possible values for our sliders

    let items = &xiv_gen_db::data().items;
    let (sort_mode, set_sort_mode) = use_query_item::<SortMode>("sort");
    let (minimum_profit, set_minimum_profit) = use_query_item::<i32>("profit");
    let (minimum_roi, set_minimum_roi) = use_query_item("roi");
    // let (max_predicted_time, set_max_predicted_time) = create_signal("1 week".to_string());
    let (max_predicted_time, set_max_predicted_time) = use_query_item::<String>("next-sale");
    let (world_filter, set_world_filter) = use_query_item::<String>("world");
    let (datacenter_filter, set_datacenter_filter) = use_query_item::<String>("datacenter");
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
        create_memo(move |_| parse_duration(&max_predicted_time().unwrap_or_default()));
    let predicted_time_string = move || {
        predicted_time()
            .map(|duration| format_duration(duration).to_string())
            .unwrap_or("---".to_string())
    };
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
        <MetaTitle title="Price Analayzer"/>
        <MetaDescription text="The analyzer finds the best items to buy on other worlds and sell on your own world."/>
       <div class="flex flex-row content-well">
            <span>"filter:"</span><br/>
           <div class="flex-column">
                <label for>"minimum profit:"<br/>
               {move || minimum_profit().map(|profit| {
                    view!{<Gil amount=profit /> }
               }.into_view()).unwrap_or("---".into_view())}
               </label><br/>
               <input id="minimum_profit" min=0 max=100000 type="number" prop:value=minimum_profit
                    on:input=move |input| set_minimum_profit(event_target_value(&input).parse::<i32>().ok()) />
           </div>
           <div class="flex-column">
                <label for="minimum_roi">"minimum ROI:"<br/>{move || minimum_roi().map(|roi| format!("{roi}%")).unwrap_or("---".to_string())}</label><br/>
               <input min=0 max=100000 type="number" prop:value=minimum_roi
                on:input=move |input| set_minimum_roi(event_target_value(&input).parse::<i32>().ok()) />
           </div>
           <div class="flex-column">
               <label for="predicted_time">"minimum time (use time notation, ex. 1w 30m) :" <br/>{predicted_time_string}</label><br/>
               <input id="predicted_time" prop:value=max_predicted_time on:input=move |input| set_max_predicted_time(Some(event_target_value(&input))) />
           </div>
       </div>
       <div class="grid-table" role="table">
        <div class="grid-header" role="rowgroup">
            <div role="columnheader" style="width: 25px">"HQ"</div>
            <div role="columnheader first" style="width: 450px">"Item"</div>
            <div role="columnheader" style="width:100px;" on:click=move |_| set_sort_mode(Some(SortMode::Profit))>
                <div class="flex-row flex-space">
                {move || {
                    match sort_mode().unwrap_or(SortMode::Roi) {
                        SortMode::Profit => {
                            view!{"Profit"<i class="fa-solid fa-sort-down"></i>}.into_view()
                        },
                        _ => view!{<Tooltip tooltip_text="Sort by profit".to_string()>"Profit"</Tooltip>}.into_view(),
                    }
                }}
                </div>
            </div>
            <div role="columnheader" style="width: 100px;" on:click=move |_| set_sort_mode(Some(SortMode::Roi))>
                <div class="flex-row flex-space">
                {move || {
                    match sort_mode().unwrap_or(SortMode::Roi) {
                        SortMode::Roi => {
                            view!{"R.O.I"<i class="fa-solid fa-sort-down"></i>}.into_view()
                        },
                        _ => view!{<Tooltip tooltip_text="Sort by return on investment".to_string()>"R.O.I."</Tooltip>}.into_view(),
                    }
                }}
                </div>
            </div>
            <div role="columnheader" style=WORLD_WIDTH>
                "World" {move || world_filter().map(move |world| view!{<a on:click=move |_| set_world_filter(None)><Tooltip tooltip_text="Clear this world filter".to_string()>"[" {&world} "]"</Tooltip></a>})}
            </div>
            <div role="columnheader" style=DATACENTER_WIDTH>
                "Datacenter" {move || datacenter_filter().map(move |datacenter| view!{<a on:click=move |_| set_datacenter_filter(None)><Tooltip tooltip_text="Clear this datacenter filter".to_string()>"[" {&datacenter} "]"</Tooltip></a>})}
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
                    <div role="cell" class="flex flex-row" style="width: 450px;">
                        <a href=format!("/item/{world}/{item_id}")>
                            <ItemIcon item_id icon_size=IconSize::Small/>
                            {item}
                        </a>
                        <Clipboard clipboard_text=item.to_string()/>
                    </div>
                    <div role="cell" style="width: 100px;"><Gil amount=data.profit /></div>
                    <div role="cell" style="width: 100px;">{data.return_on_investment}"%"</div>
                    <div role="cell" style=WORLD_WIDTH><Gil amount=data.cheapest_price/>" on "<a on:click=move |_| { set_datacenter_filter(None); set_world_filter(Some(world_event.clone())); }>{world}</a></div>
                    <div role="cell" style=DATACENTER_WIDTH><a on:click= move |_| { set_world_filter(None); set_datacenter_filter(Some(datacenter_event.clone())) }>{&datacenter}</a></div>
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
    }
}

#[component]
pub fn AnalyzerWorldView() -> impl IntoView {
    let params = use_params_map();
    let world = create_memo(move |_| params.with(|p| p.get("world").cloned()).unwrap_or_default());
    let sales = create_local_resource(
        move || params.with(|p| p.get("world").cloned()),
        move |world| async move {
            get_recent_sales_for_world(&world.ok_or(AppError::ParamMissing)?).await
        },
    );

    let world_cheapest_listings = create_local_resource(
        move || params.with(|p| p.get("world").cloned()),
        move |world| async move {
            let world = world.ok_or(AppError::ParamMissing)?;
            get_cheapest_listings(&world).await
        },
    );
    let worlds = use_context::<LocalWorldData>()
        .expect("Worlds should always be populated here")
        .0;

    view!{
        <div class="main-content">
            <span class="title">"Resale Analyzer Results for "{world}</span><br/>
            <AnalyzerWorldNavigator /><br />
            <span>"The analyzer will show items that sell more on "{world}" than they can be purchased for."</span><br/>
            <span>"These estimates aren't very accurate, but are meant to be easily accessible and fast to use."</span><br/>
            <span>"Be extra careful to make sure that the price you buy things for matches the price"</span><br/>
            <span>"Sample filters"</span>
            <a class="btn" href="?next-sale=7d&roi=300&profit=0&sort=profit&">"300% return within 7 days"</a>
            <a class="btn" href="?next-sale=1M&roi=500&profit=200000&">"500% return with 200K min gil profit within 1 month"</a>
            {worlds.ok().map(|worlds| {
                let world_value = store_value(worlds);
                let global_cheapest_listings = create_local_resource(
                    move || params.with(|p| p.get("world").cloned()),
                    move |world| async move {
                        let worlds = world_value();
                        // use the world cache to lookup the region for this world
                        let world = world.ok_or(AppError::ParamMissing)?;
                        let region = worlds.lookup_world_by_name(&world).map(|world| {
                            let region = worlds.get_region(world);
                            AnyResult::Region(region).get_name().to_string()
                        }).ok_or(AppError::ParamMissing)?;
                        get_cheapest_listings(&region).await
                    },
                );
                view!{
                        {move || {
                            let world_cheapest = world_cheapest_listings.get();
                            let sales = sales.get();
                            let global_cheapest_listings = global_cheapest_listings.get();
                            let worlds = world_value();
                            let values = world_cheapest
                                .and_then(|w| w.ok())
                                .and_then(|r| sales.and_then(|s| s.ok())
                                .and_then(|s| global_cheapest_listings.and_then(|g| g.ok()).map(|g| (r, s, g))));
                            values.map(|(world_cheapest_listings, sales, global_cheapest_listings)| {
                            view!{<AnalyzerTable sales global_cheapest_listings world_cheapest_listings worlds world=world.into() />
                            } }
                        )}}
                }
            })}
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
    create_effect(move |_| {
        if let Some(world) = current_world() {
            let world = world.name;
            nav(&format!("/analyzer/{world}"), NavigateOptions::default());
        }
    });
    view! {<label>"Analyzer World: "</label><WorldOnlyPicker current_world=current_world.into() set_current_world=set_current_world.into() />}
}

#[component]
pub fn Analyzer() -> impl IntoView {
    view! {

        <div class="container">
            <div class="main-content">
                <span class="content-title">"Analyzer"</span>
                <div class="flex-column">
                    <span>"The analyzer helps find items that are cheaper on other worlds that sell for more on your world."</span><br/>
                    <span>"Adjust parameters to try and find items that sell well"</span><br/>
                    "Choose a world to get started:"<br/>
                    <AnalyzerWorldNavigator />
                </div>
            </div>
        </div>
    }
}
