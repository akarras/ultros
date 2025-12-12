use leptos::prelude::*;
use xiv_gen::ItemId;

use super::{gil::*, world_name::*};
use crate::{
    components::skeleton::SingleLineSkeleton, global_state::cheapest_prices::CheapestPrices,
};
use ultros_api_types::world_helper::AnySelector;

/// Always shows the lowest price
#[component]
pub fn CheapestPrice(
    item_id: ItemId,
    #[prop(optional)] show_hq: Option<bool>,
    #[prop(optional, into)] label: Option<String>,
) -> impl IntoView {
    let cheapest = use_context::<CheapestPrices>().unwrap().read_listings;
    view! {
        <Suspense fallback=move || {
            view! { <SingleLineSkeleton /> }
        }>
            {move || {
                cheapest
                    .with(|data| {
                        data.as_ref()
                            .and_then(|data| {
                                data.as_ref()
                                    .ok()
                                    .and_then(|data| {
                                        let listing_data = data.find_matching_listings(item_id.0);
                                        let hq = listing_data.hq.map(|hq| ("HQ: ", hq));
                                        let lq = listing_data.lq.map(|lq| ("", lq));
                                        let data = match show_hq {
                                            Some(true) => hq,
                                            Some(false) => lq,
                                            None => hq.or(lq),
                                        };
                                        data.map(|(internal_label, listing)| {
                                            if let Some(label) = label.clone() {
                                                view! {
                                                    <div class="flex flex-col">
                                                         <span class="text-xs text-[color:var(--color-text-muted)] uppercase tracking-wider mb-0.5">{label}</span>
                                                         <div class="flex flex-row items-center gap-1.5">
                                                            <Gil amount=listing.price />
                                                            <span>
                                                                <WorldName id=AnySelector::World(listing.world_id) />
                                                            </span>
                                                        </div>
                                                    </div>
                                                }.into_any()
                                            } else {
                                                view! {
                                                    <div class="flex flex-row items-center gap-1.5">
                                                        {internal_label} <Gil amount=listing.price />
                                                        <span>
                                                            <WorldName id=AnySelector::World(listing.world_id) />
                                                        </span>
                                                    </div>
                                                }.into_any()
                                            }
                                        })
                                    })
                            })
                    })
            }}
        </Suspense>
    }
    .into_any()
}
