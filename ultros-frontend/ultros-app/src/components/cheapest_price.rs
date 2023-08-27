use leptos::*;
use xiv_gen::ItemId;

use super::{gil::*, loading::*, world_name::*};
use crate::global_state::cheapest_prices::CheapestPrices;
use ultros_api_types::{
    cheapest_listings::{CheapestListingData, CheapestListingMapKey, CheapestListingsMap},
    world_helper::AnySelector,
};

pub struct PriceSummary {
    pub lq: Option<CheapestListingData>,
    pub hq: Option<CheapestListingData>,
}

pub fn find_matching_listings(cheapest: &CheapestListingsMap, item_id: ItemId) -> PriceSummary {
    let hq = cheapest
        .map
        .get(&CheapestListingMapKey {
            hq: true,
            item_id: item_id.0,
        })
        .copied();
    let lq = cheapest
        .map
        .get(&CheapestListingMapKey {
            hq: false,
            item_id: item_id.0,
        })
        .copied();
    PriceSummary { lq, hq }
}

/// Always shows the lowest price
#[component]
pub fn CheapestPrice(item_id: ItemId) -> impl IntoView {
    let cheapest = use_context::<CheapestPrices>().unwrap().read_listings;
    view! {
        <Suspense fallback=move || view!{<Loading />}>
        {move || cheapest
        .with(|data| {
            data.as_ref().and_then(|data| data.as_ref().ok()).map(|data| {
                let listing_data = find_matching_listings(&data.1, item_id);
                let hq = listing_data.hq.map(|hq| ("HQ: ", hq));
                let lq = listing_data.lq.map(|lq| ("", lq));
                hq.or(lq)
                .map(|(label, listing)| {
                    view! {
                        {label}
                        <Gil amount=listing.price/>
                        <span style="padding-right: 5px"></span>
                        <span><WorldName id=AnySelector::World(listing.world_id)/></span>
                    }.into_view()
                }).unwrap_or(view!{"----"}.into_view())
            })
        })}
        </Suspense>
    }
}
