use super::gil::*;
use super::relative_time::*;
use crate::components::{datacenter_name::*, world_name::*};
use leptos::prelude::*;
use leptos_router::components::A;
use std::sync::Arc;
use ultros_api_types::{ActiveListing, retainer::Retainer, world_helper::AnySelector};

#[component]
pub fn ListingsTable(
    #[prop(into)] listings: Signal<Vec<(ActiveListing, Arc<Retainer>)>>,
) -> impl IntoView {
    let (show_more, set_show_more) = signal(false);
    let listing_count = move || listings.with(|l| l.len());
    let show_click = move |_| set_show_more(true);
    // Optimization: Split sorting from slicing.
    // This memo handles the expensive sorting operation and only updates when the source `listings` signal changes.
    // Note: We use Arc<Retainer> to make cloning cheap (pointer copy vs string copy).
    let sorted_listings = Memo::new(move |_| {
        let mut listings = listings();
        listings.sort_by_key(|(listing, _)| listing.price_per_unit);
        listings
    });
    // This memo handles the cheap slicing/view logic.
    // When `show_more` toggles, we re-slice the already sorted list instead of re-sorting everything.
    let listings = Memo::new(move |_| {
        sorted_listings.with(|listings| {
            if show_more() {
                listings.clone()
            } else {
                listings.iter().take(10).cloned().collect()
            }
        })
    });
    view! {
        <div class="overflow-x-auto">
            <table class="w-full min-w-[720px]">
            <tr>
                <th>"price"</th>
                <th>"qty."</th>
                <th>"total"</th>
                <th>"retainer name"</th>
                <th>"world"</th>
                <th>"datacenter"</th>
                <th>"first seen"</th>
            </tr>
            <tbody>
                {move || {
                    view! {
                        <For
                            each=listings
                            key=move |(listing, _retainer)| listing.id
                            children=move |(listing, retainer)| {
                                let total = listing.price_per_unit * listing.quantity;
                                view! {
                                    <tr>
                                        <td>
                                            <Gil amount=listing.price_per_unit />
                                        </td>
                                        <td>{listing.quantity}</td>
                                        <td>
                                            <Gil amount=total />
                                        </td>
                                        <td>
                                            <A href=format!(
                                                "/retainers/listings/{}",
                                                retainer.id,
                                            )>{retainer.name.clone()}</A>
                                        </td>
                                        <td>
                                            <WorldName id=AnySelector::World(listing.world_id) />
                                        </td>
                                        <td>
                                            <DatacenterName world_id=listing.world_id />
                                        </td>
                                        <td>
                                            <RelativeToNow timestamp=listing.timestamp />
                                        </td>
                                    </tr>
                                }
                            }
                        />
                    }
                }}
                <tr
                    on:click=show_click
                    class:hidden=move || { listing_count() < 10 || show_more() }
                >
                    <td colspan=7>
                        <button on:click=show_click class="btn w-full">
                            "Show More"
                        </button>
                    </td>
                </tr>
            </tbody>
            </table>
        </div>
    }
    .into_any()
}
