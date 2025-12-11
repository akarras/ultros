use crate::components::world_name::WorldName;
use leptos::prelude::*;
use std::collections::HashMap;
use ultros_api_types::{list::ListItem, listings::ActiveListing, world_helper::AnySelector};
use xiv_gen::ItemId;
use crate::error::AppError;

#[component]
pub fn ListBuyingView(
    items: StoredValue<Vec<(ListItem, Vec<ActiveListing>)>>,
    edit_item: Action<ListItem, Result<(), AppError>>,
) -> impl IntoView {
    let worlds = Signal::derive(move || {
        let mut worlds: HashMap<i32, Vec<(ListItem, ActiveListing)>> = HashMap::new();
        for (item, listings) in items.get_value() {
            // Skip items that have been acquired
            if item.acquired.unwrap_or(0) >= item.quantity.unwrap_or(1) {
                continue;
            }

            let filtered_listings = listings.into_iter().filter(|l: &ActiveListing| {
                if let Some(hq) = item.hq {
                    l.hq == hq
                } else {
                    true
                }
            });

            let mut listings_by_world: HashMap<i32, Vec<ActiveListing>> = HashMap::new();
            for listing in filtered_listings {
                listings_by_world
                    .entry(listing.world_id)
                    .or_default()
                    .push(listing);
            }

            for (world_id, mut world_listings) in listings_by_world {
                world_listings.sort_by_key(|l: &ActiveListing| l.price_per_unit);
                if let Some(cheapest) = world_listings.first() {
                    let world_entry = worlds.entry(world_id).or_default();
                    world_entry.push((item.clone(), cheapest.clone()));
                }
            }
        }

        let data = xiv_gen_db::data();
        let game_items = &data.items;
        let mut sorted_worlds: Vec<(i32, Vec<(ListItem, ActiveListing)>)> =
            worlds.into_iter().collect();
        sorted_worlds.sort_by_key(|(world_id, _)| *world_id);

        for (_, listings) in &mut sorted_worlds {
            listings.sort_by_key(|(item, _): &(ListItem, ActiveListing)| {
                game_items
                    .get(&ItemId(item.item_id))
                    .map(|i| i.name.clone())
                    .unwrap_or_default()
            });
        }

        sorted_worlds
    });

    let data = xiv_gen_db::data();
    let game_items = &data.items;

    view! {
        <div class="panel p-4 rounded-xl">
            {move || {
                worlds()
                    .into_iter()
                    .map(|(world_id, listings): (i32, Vec<(ListItem, ActiveListing)>)| {
                        view! {
                            <div class="mb-4">
                                <h3 class="text-xl font-bold"><WorldName id=AnySelector::World(world_id) /></h3>
                                <table class="w-full">
                                    <tbody>
                                        {listings
                                            .into_iter()
                                            .map(|(item, listing)| {
                                                let item_name = game_items
                                                    .get(&ItemId(item.item_id))
                                                    .map(|i| i.name.to_string())
                                                    .unwrap_or_default();
                                                let quantity_needed = item.quantity.unwrap_or(1);
                                                view! {
                                                    <tr>
                                                        <td>
                                                            {format!(
                                                                "{} {} @ {} gil each",
                                                                quantity_needed,
                                                                item_name,
                                                                listing.price_per_unit,
                                                            )}
                                                        </td>
                                                        <td>
                                                            <button
                                                                class="btn-primary"
                                                                on:click=move |_| {
                                                                    let mut new_item = item.clone();
                                                                    new_item.acquired = Some(
                                                                        new_item.acquired.unwrap_or(0) +
                                                                            listing.quantity,
                                                                    );
                                                                    edit_item.dispatch(new_item);
                                                                }
                                                            >
                                                                "Purchase"
                                                            </button>
                                                        </td>
                                                    </tr>
                                                }
                                            })
                                            .collect::<Vec<_>>()}
                                    </tbody>
                                </table>
                            </div>
                        }
                    })
                    .collect::<Vec<_>>()
            }}
        </div>
    }
}
