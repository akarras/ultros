use leptos::prelude::*;

use super::{datacenter_name::*, gil::*, world_name::*};
use ultros_api_types::{ActiveListing, world_helper::AnySelector};

/// Returns the subset of listings needed to fulfill the quantity.
/// Assumes `listings` is already sorted by price.
fn get_cheapest_subset(
    listings: &[ActiveListing],
    quantity: i32,
    hq: Option<bool>,
) -> Vec<ActiveListing> {
    if quantity <= 0 {
        return Vec::new();
    }

    let mut current_quantity = 0;
    let mut result = Vec::new();
    for listing in listings {
        if let Some(hq_req) = hq {
            if listing.hq != hq_req {
                continue;
            }
        }

        result.push(listing.clone());
        current_quantity += listing.quantity;
        if current_quantity >= quantity {
            break;
        }
    }
    result
}

#[component]
pub fn PriceViewer(
    #[prop(into)] quantity: Signal<i32>,
    #[prop(into)] hq: Signal<Option<bool>>,
    #[prop(into)] listings: Signal<Vec<ActiveListing>>,
) -> impl IntoView {
    // Memoize the calculation of cheapest listings to avoid re-computing on every render
    // if the inputs haven't changed.
    let cheapest_listings = Memo::new(move |_| {
        let q = quantity.get();
        let h = hq.get();
        // Use .with() to avoid cloning the source vector
        listings.with(|l| get_cheapest_subset(l, q, h))
    });

    view! {
        <div class="flex-column">
            <For
                each=move || cheapest_listings.get()
                key=|listing| listing.id
                children=move |listing| {
                    view! {
                        <div class="flex flex-row gap-1">
                            {listing.quantity} "x" <Gil amount=listing.price_per_unit /> " on "
                            <WorldName id=AnySelector::World(listing.world_id) /> "-"
                            <DatacenterName world_id=listing.world_id />
                        </div>
                    }
                }
            />
        </div>
    }
    .into_any()
}
