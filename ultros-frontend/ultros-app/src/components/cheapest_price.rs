use leptos::*;
use xiv_gen::ItemId;

use super::{gil::*, world_name::*};
use crate::global_state::cheapest_prices::CheapestPrices;
use ultros_api_types::{
    cheapest_listings::{CheapestListingItem, CheapestListings},
    world_helper::AnySelector,
};

fn find_matching_listings(
    cheapest: &CheapestListings,
    item_id: ItemId,
    hq: Option<bool>,
) -> Option<&CheapestListingItem> {
    cheapest
        .cheapest_listings
        .iter()
        .filter(|listing| listing.item_id == item_id.0)
        .filter(|listing| hq.map(|hq| hq == listing.hq).unwrap_or(true))
        .min_by_key(|listing| listing.cheapest_price)
}

/// Always shows the lowest price
#[component]
pub fn CheapestPrice(cx: Scope, item_id: ItemId, hq: Option<bool>) -> impl IntoView {
    use_context::<CheapestPrices>(cx)
        .unwrap()
        .read_listings
        .with(|data| {
            find_matching_listings(&data, item_id, hq)
                .map(|listing| {
                    view! {cx,
                        <span style="padding-right: 5px"><Gil amount=listing.cheapest_price/></span>
                        <span><WorldName id=AnySelector::World(listing.world_id)/></span>
                    }
                })
                .into_view(cx)
        })
}
