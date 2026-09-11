//! Per-row details: the secondary fields that used to sit beside every item
//! (owned quantity, target price), the cheapest listings that match the
//! row's quality, and what the line estimate did with them. Opened from the
//! row's details toggle; Escape closes it and hands focus back.
//!
//! The listings are the ones the page already fetched for the row, so
//! opening a panel fetches nothing — and the UI does not claim otherwise.

use leptos::prelude::*;
use thousands::Separable;
use ultros_api_types::{ActiveListing, list::ListItem, world_helper::AnySelector};

use super::estimate::{LineEstimate, LineStatus, matching_listings};
use super::row::{gil_text, numeric_editor};
use crate::global_state::LocalWorldData;
use crate::i18n::*;

/// How many matching listings the panel shows, cheapest first.
pub const LISTINGS_SHOWN: usize = 5;

#[component]
pub fn CartRowDetails(
    /// The element id the row's toggle names in `aria-controls`.
    id: String,
    item: Signal<ListItem>,
    listings: Signal<Vec<ActiveListing>>,
    line: Signal<Option<LineEstimate>>,
    name: String,
    can_write: Signal<bool>,
    on_edit: Callback<ListItem>,
    /// Escape inside the panel: the row closes it and refocuses the toggle.
    on_close: Callback<()>,
) -> impl IntoView {
    let i18n = use_i18n();
    let worlds = StoredValue::new(use_context::<LocalWorldData>().and_then(|data| data.0.ok()));
    let world_name = move |world_id: i32| {
        worlds.with_value(|worlds| {
            worlds
                .as_ref()
                .and_then(|helper| helper.lookup_selector(AnySelector::World(world_id)))
                .map(|world| world.get_name().to_string())
                .unwrap_or_else(|| world_id.to_string())
        })
    };
    let owned = numeric_editor(
        item,
        name.clone(),
        t_string!(i18n, lists_workspace_owned).to_string(),
        1,
        can_write,
        on_edit,
        "input w-24 text-right tabular-nums",
    );
    let target = numeric_editor(
        item,
        name,
        t_string!(i18n, lists_workspace_target_price).to_string(),
        2,
        can_write,
        on_edit,
        "input w-32 text-right tabular-nums",
    );
    let cheapest = Memo::new(move |_| {
        let item = item.get();
        listings.with(|listings| {
            matching_listings(&item, listings)
                .into_iter()
                .take(LISTINGS_SHOWN)
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let matching_count = Memo::new(move |_| {
        let item = item.get();
        listings.with(|listings| matching_listings(&item, listings).len())
    });
    let pricing = move || {
        let line = line.get()?;
        if line.remaining == 0 {
            return Some(t_string!(i18n, cart_nothing_to_buy).to_string());
        }
        Some(match (line.status, line.unit_price) {
            (LineStatus::NoSupply, _) | (_, None) => {
                t_string!(i18n, cart_no_matching_listings).to_string()
            }
            (_, Some(price)) => t_string!(
                i18n,
                cart_pricing_detail,
                covered = line.priced_units,
                requested = line.remaining,
                listings = matching_count.get(),
                price = gil_text(i18n, i64::from(price))
            )
            .to_string(),
        })
    };
    view! {
        <div id=id class="order-last col-span-full space-y-3 rounded-lg bg-[color:var(--color-background)] px-3 py-2 text-sm" data-testid="cart-row-details"
            on:keydown=move |ev| {
                if ev.key() == "Escape" {
                    ev.stop_propagation();
                    on_close.run(());
                }
            }>
            <div class="flex flex-wrap items-end gap-4">
                <label class="flex flex-col gap-1">
                    <span class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, lists_workspace_owned)}</span>
                    {owned}
                </label>
                <label class="flex flex-col gap-1">
                    <span class="text-xs text-[color:var(--color-text-muted)]">{t!(i18n, lists_workspace_target_price)}</span>
                    {target}
                </label>
                <button type="button" class="btn-ghost ml-auto text-xs" on:click=move |_| on_close.run(())>{t!(i18n, cart_close_details)}</button>
            </div>
            <div class="space-y-1">
                <h4 class="text-xs uppercase tracking-wide text-[color:var(--color-text-muted)]">{t!(i18n, cart_listings_heading)}</h4>
                <p class="text-xs text-[color:var(--color-text-muted)]" data-testid="cart-pricing-detail">{pricing}</p>
                <Show when=move || !cheapest.get().is_empty()>
                    <table class="w-full max-w-md text-xs tabular-nums">
                        <thead>
                            <tr class="text-left text-[color:var(--color-text-muted)]">
                                <th class="py-1 pr-2 font-normal">{t!(i18n, cart_listing_world)}</th>
                                <th class="py-1 pr-2 text-right font-normal">{t!(i18n, cart_listing_unit_price)}</th>
                                <th class="py-1 pr-2 text-right font-normal">{t!(i18n, cart_listing_stack)}</th>
                                <th class="py-1 font-normal">{t!(i18n, lists_workspace_quality)}</th>
                            </tr>
                        </thead>
                        <tbody>
                            <For each=move || cheapest.get() key=|listing| listing.id children=move |listing| {
                                view! {
                                    <tr>
                                        <td class="py-0.5 pr-2">{world_name(listing.world_id)}</td>
                                        <td class="py-0.5 pr-2 text-right">{listing.price_per_unit.separate_with_commas()}</td>
                                        <td class="py-0.5 pr-2 text-right">{listing.quantity}</td>
                                        <td class="py-0.5">{if listing.hq { t_string!(i18n, lists_workspace_hq).to_string() } else { t_string!(i18n, lists_workspace_nq).to_string() }}</td>
                                    </tr>
                                }
                            } />
                        </tbody>
                    </table>
                </Show>
            </div>
        </div>
    }
}
