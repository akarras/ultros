use leptos::either::Either;
use leptos::prelude::*;

use super::{datacenter_name::*, gil::*, world_name::*};
use ultros_api_types::{ActiveListing, world_helper::AnySelector};

fn get_cheapest_listing(
    mut listings: Vec<ActiveListing>,
    quantity: i32,
    hq: Option<bool>,
) -> Vec<ActiveListing> {
    // Optimization: Filter out unwanted quality types *before* sorting.
    // This significantly reduces the N in O(N log N) sorting time when filtering by HQ/NQ.
    if let Some(hq) = hq {
        listings.retain(|listing| listing.hq == hq);
    }
    listings.sort_by_key(|listing| listing.price_per_unit);

    let quantity_needed = quantity;
    let mut current_quantity = 0;
    listings
        .into_iter()
        .take_while(|listings| {
            current_quantity += listings.quantity;
            current_quantity <= quantity_needed
        })
        .collect::<Vec<_>>()
}

#[component]
pub fn PriceViewer(quantity: i32, hq: Option<bool>, listings: Vec<ActiveListing>) -> impl IntoView {
    let cheapest_listings = get_cheapest_listing(listings, quantity, hq);
    view! {
        <div class="flex flex-col gap-1">
            {if cheapest_listings.is_empty() {
                Either::Left(
                    view! {
                        <span class="text-[color:var(--color-text-muted)]">"No listing data"</span>
                    },
                )
            } else {
                Either::Right(
                    cheapest_listings
                        .iter()
                        .map(|listing| {
                            view! {
                                <div class="flex flex-wrap items-center gap-x-1 gap-y-0 text-sm">
                                    <span>{listing.quantity} "x"</span>
                                    <Gil amount=listing.price_per_unit />
                                    <span>"on"</span>
                                    <WorldName id=AnySelector::World(listing.world_id) />
                                    <span class="text-[color:var(--color-text-muted)]">"-"</span>
                                    <DatacenterName world_id=listing.world_id />
                                </div>
                            }
                        })
                        .collect::<Vec<_>>(),
                )
            }}
        </div>
    }
    .into_any()
}
