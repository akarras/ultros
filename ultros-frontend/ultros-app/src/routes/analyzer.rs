use chrono::{Duration, Utc};
use humantime::{format_duration, parse_duration};
use leptos::*;
use leptos_router::*;
use std::{cmp::Reverse, collections::HashMap, rc::Rc};
use ultros_api_types::{
    cheapest_listings::CheapestListings,
    recent_sales::{RecentSales, SaleData},
    world_helper::{AnyResult, AnySelector, WorldHelper},
};
use xiv_gen::ItemId;

use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{clipboard::*, gil::*, item_icon::*, loading::*, tooltip::*, virtual_scroller::*},
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
    let avg_sale_duration = t.map(|t| Duration::milliseconds(t));
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
                let key = ProfitKey {
                    item_id,
                    hq,
                };
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
                    return_on_investment: ((estimated_sale_price - cheapest_price) as f32 / cheapest_price as f32
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

#[component]
fn AnalyzerTable(
    cx: Scope,
    sales: RecentSales,
    global_cheapest_listings: CheapestListings,
    world_cheapest_listings: CheapestListings,
    worlds: Rc<WorldHelper>,
    world: Signal<String>,
) -> impl IntoView {
    let profits = ProfitTable::new(sales, global_cheapest_listings, world_cheapest_listings);

    // get ranges of possible values for our sliders

    let items = &xiv_gen_db::decompress_data().items;
    let (sort_mode, set_sort_mode) = create_signal(cx, SortMode::Roi);
    let (minimum_profit, set_minimum_profit) = create_signal(cx, Ok(0));
    let (minimum_roi, set_minimum_roi) = create_signal(cx, Ok(0));
    let (max_predicted_time, set_max_predicted_time) = create_signal(cx, "1 week".to_string());
    let (world_filter, set_world_filter) = create_signal(cx, Option::<String>::None);
    let (datacenter_filter, set_datacenter_filter) = create_signal(cx, Option::<String>::None);
    let world_clone = worlds.clone(); // cloned to pass into closure
    let world_filter_list = create_memo(cx, move |_| {
        let world = world_filter().or_else(move || datacenter_filter())?;
        let filter = world_clone
            .lookup_world_by_name(&world)?
            .all_worlds()
            .map(|w| w.id)
            .collect::<Vec<_>>();
        Some(filter)
    });
    let world_clone = worlds.clone();
    let lookup_world = create_memo(cx, move |_| {
        Some(AnySelector::from(
            &world_clone.lookup_world_by_name(&world())?,
        ))
    });
    let predicted_time = create_memo(cx, move |_| parse_duration(&max_predicted_time()));
    let predicted_time_string = move || {
        predicted_time()
            .map(|duration| format_duration(duration).to_string())
            .unwrap_or_default()
    };
    let sorted_data = create_memo(cx, move |_| {
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
        match sort_mode() {
            SortMode::Roi => sorted_data.sort_by_key(|data| Reverse(data.return_on_investment)),
            SortMode::Profit => sorted_data.sort_by_key(|data| Reverse(data.profit)),
        }

        // sorted_data.truncate(100);
        sorted_data
    });
    view! { cx,
       <div class="flex flex-row">
           <div>
               {move || if let Ok(minimum_profit) = minimum_profit() {
                   view!{cx, <p>"minimum profit:"<br/><Gil amount=minimum_profit/></p>}
               } else {
                   view!{cx, <p>"Minimum profit not set"</p>}
               }}
               <input min=0 max=100000 type="number" on:input=move |input| set_minimum_profit(event_target_value(&input).parse::<i32>()) />
           </div>
           <div>
               {move || if let Ok(minimum_roi) = minimum_roi() {
                   view!{cx, <p>"minimum ROI:"<br/>{minimum_roi}"%"</p>}
               } else {
                   view!{cx, <p>"Minimum ROI not set"</p>}
               }}
               <input min=0 max=100000 type="number" on:input=move |input| set_minimum_roi(event_target_value(&input).parse::<i32>()) />
           </div>
           <div>
               <p>"minimum time (use time notation, ex. 1w 30m) :" <br/>{predicted_time_string}</p>
               <input on:input=move |input| set_max_predicted_time(event_target_value(&input)) />
           </div>
       </div>
       <div class="grid-table" role="table">
        <div class="grid-header" role="rowgroup">
            <div role="columnheader" style="width: 25px">"HQ"</div>
            <div role="columnheader first" style="width: 450px">"Item"</div>
            <div role="columnheader" style="width:100px;" on:click=move |_| set_sort_mode(SortMode::Profit)>
                <div class="flex-row flex-space">
                {move || {
                    match sort_mode() {
                        SortMode::Profit => {
                            view!{cx, "Profit"<i class="fa-solid fa-sort-down"></i>}.into_view(cx)
                        },
                        _ => view!{cx, <Tooltip tooltip_text="Sort by profit".to_string()>"Profit"</Tooltip>}.into_view(cx),
                    }
                }}
                </div>
            </div>
            <div role="columnheader" style="width: 100px;" on:click=move |_| set_sort_mode(SortMode::Roi)>
                <div class="flex-row flex-space">
                {move || {
                    match sort_mode() {
                        SortMode::Roi => {
                            view!{cx, "R.O.I"<i class="fa-solid fa-sort-down"></i>}.into_view(cx)
                        },
                        _ => view!{cx, <Tooltip tooltip_text="Sort by return on investment".to_string()>"R.O.I."</Tooltip>}.into_view(cx),
                    }
                }}
                </div>
            </div>
            <div role="columnheader" style="width: 180px;">"World" {move || world_filter().map(move |world| view!{cx, <a on:click=move |_| set_world_filter(None)>"[" {world} "]"</a>})}</div>
            <div role="columnheader" style="width: 180px;">"Datacenter" {move || datacenter_filter().map(move |datacenter| view!{cx, <a on:click=move |_| set_datacenter_filter(None)>"[" {datacenter} "]"</a>})}</div>
            <div role="columnheader" style="widht: 300px;">"Next sale"</div>
        </div>
        <VirtualScroller
            viewport_height=1000.0
            row_height=37.3
            each=sorted_data.into()
            key=move |data| {
                (data.sale_summary.item_id, data.cheapest_world_id, data.sale_summary.hq)
            }
            view=move |cx, data| {
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
                template! {cx, <div class="grid-row" role="row-group">
                    <div role="cell" style="width: 25px;">{data.sale_summary.hq.then(|| "✅")}</div>
                    <div role="cell" class="flex flex-row" style="width: 450px;">
                        <a href=format!("/item/{world}/{item_id}")>
                            <ItemIcon item_id icon_size=IconSize::Small/>
                            {item}
                        </a>
                        <Clipboard clipboard_text=item.to_string()/>
                    </div>
                    <div role="cell" style="width: 100px;"><Gil amount=data.profit /></div>
                    <div role="cell" style="width: 100px;">{data.return_on_investment}"%"</div>
                    <div role="cell" style="width: 180px;"><Gil amount=data.cheapest_price/>" on "<a on:click=move |_| { set_datacenter_filter(None); set_world_filter(Some(world_event.clone())); }>{world}</a></div>
                    <div role="cell" style="width: 180px;"><a on:click= move |_| { set_world_filter(None); set_datacenter_filter(Some(datacenter_event.clone())) }>{&datacenter}</a></div>
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
pub fn AnalyzerWorldView(cx: Scope) -> impl IntoView {
    let worlds = use_context::<LocalWorldData>(cx).expect("Local world data");
    let params = use_params_map(cx);
    let world = create_memo(cx, move |_| {
        params.with(|p| p.get("world").cloned()).unwrap_or_default()
    });
    let sales = create_resource(
        cx,
        move || params.with(|p| p.get("world").cloned()),
        move |world| async move {
            get_recent_sales_for_world(cx, &world.ok_or(AppError::ParamMissing)?).await
        },
    );
    let global_cheapest_listings = create_resource(
        cx,
        move || {
            (
                params.with(|p| p.get("world").cloned()),
                worlds.0.read(cx).is_some(),
            )
        },
        move |(world, _world_data)| async move {
            // use the world cache to lookup the region for this world
            let world = world.ok_or(AppError::ParamMissing)?;
            let region = worlds
                .0
                .read(cx)
                .map(|w| w.ok())
                .flatten()
                .and_then(|worlds| {
                    worlds.lookup_world_by_name(&world).map(|world| {
                        let region = worlds.get_region(world);
                        AnyResult::Region(region).get_name().to_string()
                    })
                })
                .ok_or(AppError::ParamMissing)?;
            get_cheapest_listings(cx, &region).await
        },
    );

    let world_cheapest_listings = create_resource(
        cx,
        move || params.with(|p| p.get("world").cloned()),
        move |world| async move {
            let world = world.ok_or(AppError::ParamMissing)?;
            get_cheapest_listings(cx, &world).await
        },
    );

    view!{cx, <div class="container">
            <div class="main-content">
                <span class="title">"Analyzer Results for "{world}</span>
                <Suspense fallback=move || view!{cx, <Loading />}>
                {move || {
                    let global_cheapest_listings = global_cheapest_listings.read(cx);
                    let world_cheapest = world_cheapest_listings.read(cx);
                    let sales = sales.read(cx);
                    let worlds = use_context::<LocalWorldData>(cx).expect("Worlds should always be populated here").0.read(cx);
                    match (sales, global_cheapest_listings, world_cheapest, worlds) {
                        (Some(Ok(sales)), Some(Ok(global_cheapest_listings)), Some(Ok(world_cheapest_listings)), Some(Ok(worlds))) => {
                            view!{cx, <AnalyzerTable sales global_cheapest_listings world_cheapest_listings worlds world=world.into() />}.into_view(cx)
                        },
                        (Some(sales), Some(listings), Some(world_cheapest), Some(worlds)) => {
                            format!("Failed to get listings/sales {:?} {:?} {:?} {:?}", sales.err(), listings.err(), world_cheapest, worlds.err()).into_view(cx)
                        },
                        _ => {
                            view!{cx, <Loading/>}.into_view(cx)
                        }
                    }
                }}
                </Suspense>
        </div>
    </div>}.into_view(cx)
}

#[component]
pub fn Analyzer(cx: Scope) -> impl IntoView {
    // let worlds = use_context::<LocalWorldData>(cx).expect("Local world data");
    view! {
        cx,
        <div class="container">
            <div class="main-content">
                <span class="content-title">"Analyzer"</span>
                <div class="flex-column">
                    <span>"The analyzer helps find items that are cheaper on other worlds that sell for more on your world."</span>
                    <span>"Adjust parameters to try and find items that sell well"</span>
                    <a href="/analyzer/Gilgamesh" class="btn">"Gilgamesh"</a>
                </div>
            </div>
        </div>
    }
}
