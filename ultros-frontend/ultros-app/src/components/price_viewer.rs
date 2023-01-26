use leptos::*;

use super::{gil::*, loading::*};
use crate::api::get_listings;
use ultros_api_types::{ActiveListing, Retainer};

fn get_cheapest_listing(
    mut listings: Vec<(ActiveListing, Retainer)>,
    hq: Option<bool>,
) -> Option<ActiveListing> {
    listings.sort_by_key(|(listing, _)| listing.price_per_unit);

    listings.into_iter().map(|(l, _)| l).find(|(listing)| {
        if let Some(hq) = hq {
            listing.hq == hq
        } else {
            true
        }
    })
}

#[component]
pub fn PriceViewer(cx: Scope, item_id: i32, world: String, hq: Option<bool>) -> impl IntoView {
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
                match get_cheapest_listing(listing.listings, hq) {
                    Some(listing) => view!{cx, <Gil amount=listing.price_per_unit/>}.into_view(cx),
                    None => view!{cx, "No listings"}.into_view(cx)
                }
            },
            None => view!{cx, "No listings"}.into_view(cx)
        })}
        </Suspense>
    </div>}
}
