use std::{collections::HashMap};
use futures::future::join;
use chrono::Duration;
use leptos::*;
use leptos_router::use_params_map;
use ultros_api_types::{
    cheapest_listings::CheapestListings,
    recent_sales::{RecentSales, SaleData},
    world_helper::AnyResult,
};

use crate::{
    api::{get_cheapest_listings, get_recent_sales_for_world},
    global_state::LocalWorldData,
};

/// Computed sale stats
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

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
struct ProfitKey {
    item_id: i32,
    hq: bool,
}

struct ProfitData {
    profit: i32,
    return_on_investment: i32,
    sale_summary: SaleSummary,
}

struct ProfitTable(Vec<ProfitData>);

fn compute_summary(sale: SaleData) -> SaleSummary {
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
    let t = sales.first().and_then(|first| {
        sales.last().map(|last| {
            (last.sale_date - first.sale_date).num_milliseconds().abs() / sales.len() as i64
        })
    });
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
        let cheap_map: HashMap<ProfitKey, i32> = listings
            .cheapest_listings
            .into_iter()
            .map(|listing| {
                (
                    ProfitKey {
                        item_id: listing.item_id,
                        hq: listing.hq,
                    },
                    listing.cheapest_price,
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
                let cheap_map = cheap_map.get(&key)?;
                let summary = compute_summary(sale);
                // TODO come back to the return_on_investment check
                Some(ProfitData {
                    profit: summary.min_price - *cheap_map,
                    return_on_investment: summary.min_price / *cheap_map,
                    sale_summary: summary,
                })
            })
            .collect();
        ProfitTable(table)
    }
}

#[component]
fn AnalyzerTable(cx: Scope, sales: RecentSales, listings: CheapestListings) -> impl IntoView {
    let profits = ProfitTable::new(sales, listings);
    
    // let sales_data : Vec<_> = sales.sales.into_iter().map(compute_summary).collect();
    view! { cx, <div></div> }
}

#[component]
pub fn Analyzer(cx: Scope) -> impl IntoView {
    let worlds = use_context::<LocalWorldData>(cx).expect("Local world data");
    let params = use_params_map(cx);
    let recent_sales = create_resource(
        cx,
        move || params().get("world").cloned().unwrap_or_default(),
        move |world| async move {
            // use the world cache to lookup the region for this world
            let region = worlds.0().flatten().and_then(|worlds| {
                worlds.lookup_world_by_name(&world).map(|world| {
                    let region = worlds.get_region(world);
                    AnyResult::Region(region).get_name().to_string()
                })
            })?;
            Some(join(get_recent_sales_for_world(cx, &region), get_cheapest_listings(cx, &world)).await)
        },
    );
    view! {
        cx,
        <div class="container">
            <div class="main-content flex flex-center">
                <div>
                    <span class="content-title">"Analyzer"</span>
                    <div>
                        <span>"The analyzer helps find items that are cheaper on other worlds that sell for more on your world."</span>
                        <span>"Adjust parameters to try and find items that sell well"</span>
                        <Suspense fallback=move || view!{cx, <div class="loading">"Loading..."</div>}>
                        {move || {
                            let sales = recent_sales();
                            match sales {
                                Some(Some((Some(sales), Some(listings)))) => {
                                    view!{cx, <AnalyzerTable sales listings />}.into_view(cx)
                                },
                                _ => {
                                    view!{cx, <div class="loading">"loading"</div>}.into_view(cx)
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
