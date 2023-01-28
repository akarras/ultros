use leptos::*;

use super::{gil::*, loading::*, world_name::*};
use crate::api::get_listings;
use ultros_api_types::{world_helper::AnySelector, ActiveListing, Retainer};

fn get_cheapest_listing(
    mut listings: Vec<(ActiveListing, Retainer)>,
    quantity: i32,
    hq: Option<bool>,
) -> Vec<ActiveListing> {
    listings.sort_by_key(|(listing, _)| listing.price_per_unit);
    let quantity_needed = quantity;
    let mut current_quantity = 0;
    listings
        .into_iter()
        .map(|(l, _)| l)
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
pub fn PriceViewer(
    cx: Scope,
    item_id: i32,
    quantity: i32,
    world: String,
    hq: Option<bool>,
) -> impl IntoView {
    let listings = create_resource(
        cx,
        move || (item_id, world.clone(), hq),
        move |(item_id, world, _)| async move { get_listings(cx, item_id, &world).await },
    );
    view! {cx,
    <div>
        <Suspense fallback=move || view!{cx, <Loading/>}>
        {move || listings().map(|listings| match listings {
            Some(listing) => {
                let cheapest_listings = get_cheapest_listing(listing.listings, quantity, hq);
                view!{cx, <div class="flex-column">
                    {cheapest_listings.iter().map(|listing| view!{cx,
                        <div class="flex-row">
                            {listing.quantity}" "
                            <Gil amount=listing.price_per_unit/>" "
                            <WorldName id=AnySelector::World(listing.world_id)/>
                        </div>
                    }).collect::<Vec<_>>()}
                </div>}.into_view(cx)
            },
            None => view!{cx, "No listings"}.into_view(cx)
        })}
        </Suspense>
    </div>}
}
