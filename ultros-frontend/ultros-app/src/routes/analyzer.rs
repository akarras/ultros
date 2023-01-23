use chrono::{Duration, Utc};
use itertools::Itertools;
use leptos::*;
use leptos_router::*;
use std::{cmp::Reverse, collections::HashMap, rc::Rc};
use timediff::TimeDiff;
use ultros_api_types::{
    cheapest_listings::CheapestListings,
    recent_sales::{RecentSales, SaleData},
    world_helper::{AnyResult, AnySelector, WorldHelper},
};
use xiv_gen::ItemId;

use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    components::{clipboard::*, gil::*, item_icon::*, loading::*, tooltip::*},
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

fn compute_summary(sale: SaleData) -> SaleSummary {
    let now = Utc::now().naive_utc();
    let SaleData { item_id, hq, sales } = sale;
    let min_price = sales
        .iter()
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

impl ProfitTable {
    fn new(sales: RecentSales, listings: CheapestListings) -> Self {
        let cheap_map: HashMap<ProfitKey, (i32, i32)> = listings
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
            .collect();
        let table = sales
            .sales
            .into_iter()
            .flat_map(|sale| {
                let key = ProfitKey {
                    item_id: sale.item_id,
                    hq: sale.hq,
                };
                let (cheapest_price, cheapest_world_id) = *cheap_map.get(&key)?;
                let summary = compute_summary(sale);
                // TODO come back to the return_on_investment check
                Some(ProfitData {
                    profit: summary.min_price - cheapest_price,
                    return_on_investment: (summary.min_price as f32 / cheapest_price as f32 * 100.0)
                        as i32,
                    sale_summary: summary,
                    cheapest_world_id,
                    cheapest_price,
                })
            })
            // .take(100)
            // .filter(|data| data.profit > 0) // filter items that don't return any profit
            .collect();
        ProfitTable(table)
    }

    fn min_max_profit(&self) -> (i32, i32) {
        self.0
            .iter()
            .map(|i| i.profit)
            .minmax()
            .into_option()
            .unwrap_or((i32::MIN, i32::MAX))
    }

    fn min_max_roi(&self) -> (i32, i32) {
        self.0
            .iter()
            .map(|i| i.return_on_investment)
            .minmax()
            .into_option()
            .unwrap_or((i32::MIN, i32::MAX))
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
    listings: CheapestListings,
    worlds: Rc<WorldHelper>,
) -> impl IntoView {
    let profits = ProfitTable::new(sales, listings);

    // get ranges of possible values for our sliders
    let (m_profit, max_profit) = profits.min_max_profit();
    let (m_roi, max_roi) = profits.min_max_roi();

    let items = &xiv_gen_db::decompress_data().items;
    let (sort_mode, set_sort_mode) = create_signal(cx, SortMode::Roi);
    let (minimum_profit, set_minimum_profit) = create_signal(cx, 0);
    let (minimum_roi, set_minimum_roi) = create_signal(cx, 0);
    let (max_predicted_time, set_max_predicted_time) = create_signal(cx, i64::pow(10, 10));
    let predicted_time = move || Duration::seconds(max_predicted_time());
    let sorted_data = create_memo(cx, move |_| {
        let mut sorted_data = profits
            .0
            .iter()
            .cloned()
            .filter(move |data| data.profit > minimum_profit())
            .filter(move |data| data.return_on_investment > minimum_roi())
            .filter(move |data| data.sale_summary.avg_sale_duration < Some(predicted_time()))
            .collect::<Vec<_>>();
        match sort_mode() {
            SortMode::Roi => sorted_data.sort_by_key(|data| Reverse(data.return_on_investment)),
            SortMode::Profit => sorted_data.sort_by_key(|data| Reverse(data.profit)),
        }

        sorted_data.truncate(100);
        sorted_data
    });

    view! { cx, <div class="flex flex-wrap">
        <div class="slidecontainer">
            <p>"minimum profit:" <span>{move || view!{cx, <Gil amount=minimum_profit() />}}</span></p>
            <input type="range" min=0 max=10 prop:value=minimum_profit class="slider" on:input=move |input| set_minimum_profit(i32::pow(10, event_target_value(&input).parse::<u32>().expect("Shouldn't have non integer"))) />
        </div>
        <div class="slidecontainer">
            <p>"minimum ROI:" <span>{move || minimum_roi()}"%"</span></p>
            <input type="range" min=0 max=10 prop:value=minimum_roi class="slider" on:input=move |input| set_minimum_roi(i32::pow(10, event_target_value(&input).parse::<u32>().expect("Shouldn't have non integer"))) />
        </div>
        <div class="slidecontainer">
            <p>"minimum time:" <span>{move || predicted_time().to_std().ok().and_then(|duration| TimeDiff::to_diff_duration(duration).parse().ok()).unwrap_or_default()}</span></p>
            <input type="range" min=0 max=10 prop:value=max_predicted_time class="slider" on:input=move |input| set_max_predicted_time(i64::pow(10, event_target_value(&input).parse::<u32>().expect("Shouldn't have non integer"))) />
        </div>
        </div>
        <div>
        <table>
            <tr>
                <th>"Item"</th>
                <th on:click=move |_| set_sort_mode(SortMode::Profit)>
                    <div class="flex-row flex-space">
                    {move || {
                        match sort_mode() {
                            SortMode::Profit => {
                                view!{cx, "Profit"<i class="fa-solid fa-sort-down"></i>}.into_view(cx)
                            },
                            _ => view!{cx, <Tooltip tooltip_text="Sort by profit".to_string()>"profit"</Tooltip>}.into_view(cx),
                        }
                    }}
                    </div>
                </th>
                <th on:click=move |_| set_sort_mode(SortMode::Roi)>
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
                </th>
                <th>"World"</th>
                <th>"Datacenter"</th>
                <th>"Next predicted sale"</th>
            </tr>
            <For each=sorted_data
                 key=move |data| {
                    (data.sale_summary.item_id, data.cheapest_world_id, data.sale_summary.hq)
                 }
                 view=move |data| {
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
                    let item_id = data.sale_summary.item_id;
                    let item = items
                        .get(&ItemId(item_id))
                        .map(|item| item.name.as_str())
                        .unwrap_or_default();
                    view! {cx, <tr>
                        <td class="flex flex-row">
                            <a href=format!("/listings/{world}/{item_id}")>
                                <ItemIcon item_id icon_size=IconSize::Small/>
                                {item}
                            </a>
                            <Clipboard clipboard_text=item.to_string()/>
                        </td>
                        <td><Gil amount=data.profit /></td>
                        <td>{data.return_on_investment}"%"</td>
                        <td><Gil amount=data.cheapest_price/>" on "{world}</td>
                        <td>{datacenter}</td>
                        <td>{data.sale_summary
                                .avg_sale_duration
                                .and_then(|sale_duration| {
                                    let duration = sale_duration.to_std().ok()?;
                                    if sale_duration.num_minutes() < 1 {
                                        None
                                    } else {
                                        TimeDiff::to_diff_duration(duration).parse().ok()
                                    }
                                })
                        }</td>
                        </tr>}
                 }/>
        </table>
    </div> }
}

#[component]
pub fn Analyzer(cx: Scope) -> impl IntoView {
    let worlds = use_context::<LocalWorldData>(cx).expect("Local world data");
    let params = use_params_map(cx);
    let listings = create_resource(
        cx,
        move || params.with(|p| p.get("world").cloned()),
        move |world| async move {
            // use the world cache to lookup the region for this world
            let world = world?;
            let region = worlds.0().flatten().and_then(|worlds| {
                worlds.lookup_world_by_name(&world).map(|world| {
                    let region = worlds.get_region(world);
                    AnyResult::Region(region).get_name().to_string()
                })
            })?;
            get_cheapest_listings(cx, &region).await
        },
    );
    let sales = create_resource(
        cx,
        move || params.with(|p| p.get("world").cloned()),
        move |world| async move { get_recent_sales_for_world(cx, &world?).await },
    );

    view! {
        cx,
        <div class="container">
            <div class="main-content flex flex-center">
                <div>
                    <span class="content-title">"Analyzer"</span>
                    <div class="flex-column">
                        <span>"The analyzer helps find items that are cheaper on other worlds that sell for more on your world."</span>
                        <span>"Adjust parameters to try and find items that sell well"</span>
                        {if params.with(|p| p.get("world").is_none()) {
                            view!{cx, <a href="/analyzer/Gilgamesh" class="btn">"Gilgamesh"</a>}.into_view(cx)
                        } else {
                            view!{cx, <Suspense fallback=move || view!{cx, <Loading />}>
                            {move || {
                                let sales = sales();
                                let listings = listings();
                                let worlds = use_context::<LocalWorldData>(cx).expect("Worlds should always be populated here").0();
                                match (sales, listings, worlds) {
                                    (Some(Some(sales)), Some(Some(listings)), Some(Some(worlds))) => {
                                        view!{cx, <AnalyzerTable sales listings worlds />}.into_view(cx)
                                    },
                                    (Some(_), Some(_), Some(_)) => {
                                        view!{cx, "Failed to get listings/sales"}.into_view(cx)
                                    },
                                    _ => {
                                        view!{cx, <Loading/>}.into_view(cx)
                                    }
                                }
                            }}
                            </Suspense>}.into_view(cx)
                        }}
                    </div>
                </div>
            </div>
        </div>
    }
}
