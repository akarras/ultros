use leptos::*;

use super::{datacenter_name::*, gil::*, world_name::*};
use ultros_api_types::{world_helper::AnySelector, ActiveListing};

fn get_cheapest_listing(
    mut listings: Vec<ActiveListing>,
    quantity: i32,
    hq: Option<bool>,
) -> Vec<ActiveListing> {
    listings.sort_by_key(|listing| listing.price_per_unit);
    let quantity_needed = quantity;
    let mut current_quantity = 0;
    listings
        .into_iter()
        .filter(|listing| {
            if let Some(hq) = hq {
                listing.hq == hq
            } else {
                true
            }
        })
        .take_while(|listings| {
            current_quantity += listings.quantity;
            current_quantity <= quantity_needed
        })
        .collect::<Vec<_>>()
}

#[component]
pub fn PriceViewer(quantity: i32, hq: Option<bool>, listings: Vec<ActiveListing>) -> impl IntoView {
    let cheapest_listings = get_cheapest_listing(listings, quantity, hq);
    view! {<div class="flex-column">
        {cheapest_listings.iter().map(|listing| view!{
            <div style="display: block;">
                {listing.quantity}"x"
                <Gil amount=listing.price_per_unit/>" on "
                <WorldName id=AnySelector::World(listing.world_id)/>"-"
                <DatacenterName world_id=listing.world_id/>
            </div>
        }).collect::<Vec<_>>()}
    </div>}
}
