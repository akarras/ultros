use super::gil::*;
use super::relative_time::*;
use crate::components::{datacenter_name::*, world_name::*};
use crate::i18n::*;
use leptos::prelude::*;
use leptos_router::components::A;
use std::sync::Arc;
use ultros_api_types::{ActiveListing, retainer::Retainer, world_helper::AnySelector};

#[component]
pub fn ListingsTable(
    #[prop(into)] listings: Signal<Vec<(ActiveListing, Arc<Retainer>)>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let (show_more, set_show_more) = signal(false);
    let listing_count = move || listings.with(|l| l.len());
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
            <thead>
                <tr>
                    <th scope="col">{t!(i18n, listings_col_price)}</th>
                    <th scope="col">{t!(i18n, listings_col_qty)}</th>
                    <th scope="col">{t!(i18n, listings_col_total)}</th>
                    <th scope="col">{t!(i18n, listings_col_retainer)}</th>
                    <th scope="col">{t!(i18n, listings_col_world)}</th>
                    <th scope="col">{t!(i18n, listings_col_datacenter)}</th>
                    <th scope="col">{t!(i18n, listings_col_first_seen)}</th>
                </tr>
            </thead>
            <tbody>
                // #6831 — the largest GlitchTip issue by orders of magnitude —
                // is a tachys hydration panic (`hydration.rs`
                // `failed_to_cast_marker_node`: "expected a marker node, found
                // <tr>") firing on essentially every `/item/*` page under
                // production's out-of-order streaming SSR.
                //
                // Root cause: a `<For>` relies on its *following sibling* to
                // supply a marker node so the hydration walk knows where the
                // keyed list ends. A dynamic sibling (`{ move || … }`) emits that
                // opening marker; a plain static element does not. This `<tbody>`
                // placed a static `<tr>` (the "show more" row) directly after the
                // `<For>`, leaving the list's trailing edge unbounded — the walker
                // then reads that `<tr>` where it expected a marker and panics.
                //
                // (PR #933 removed a redundant `{ move || <For/> }` *wrapper* but
                // left this static-`<tr>`-after-`<For>` adjacency intact, so the
                // crash survived on the deployed fix build.)
                //
                // Fix: render `<For>` as a direct child and the "show more" row as
                // a *dynamic* `{ move || … }` block, exactly like the sibling
                // `SaleHistoryTable` — which reads the same resource in the same
                // `<Transition>` on this page and never crashes. The dynamic block
                // supplies the marker node that bounds the `<For>`.
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
                {move || {
                    (!show_more() && listing_count() >= 10)
                        .then(|| {
                            view! {
                                <tr>
                                    <td colspan=7>
                                        <button
                                            class="btn w-full"
                                            on:click=move |_| set_show_more(true)
                                        >
                                            {t!(i18n, listings_show_more)}
                                        </button>
                                    </td>
                                </tr>
                            }
                        })
                }}
            </tbody>
            </table>
        </div>
    }
    .into_any()
}
