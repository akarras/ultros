use crate::components::clipboard::Clipboard;
use crate::components::gil::Gil;
use crate::components::item_icon::ItemIcon;
use crate::components::world_name::WorldName;
use crate::error::AppError;
use leptos::prelude::*;
use std::collections::HashMap;
use ultros_api_types::{
    icon_size::IconSize, list::ListItem, listings::ActiveListing,
    user::UserRetainers, world_helper::AnySelector,
};
use xiv_gen::{WorldDcGroupTypeId, ItemId, WorldId};

#[derive(Clone, Debug, PartialEq, Eq)]
struct DatacenterView {
    pub name: String,
    pub worlds: Vec<(i32, Vec<(ListItem, ActiveListing)>)>,
}

#[component]
pub fn ListBuyingView(
    items: StoredValue<Vec<(ListItem, Vec<ActiveListing>)>>,
    edit_item: Action<ListItem, Result<(), AppError>>,
    retainers: Resource<(), Result<UserRetainers, AppError>>,
) -> impl IntoView {
    let datacenters = Signal::derive(move || {
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
        let mut worlds_by_dc: HashMap<WorldDcGroupTypeId, Vec<(i32, Vec<(ListItem, ActiveListing)>)>> =
            HashMap::new();

        for (world_id, listings) in worlds {
            if let Some(world) = data.worlds.get(&WorldId(world_id)) {
                let dc_worlds = worlds_by_dc.entry(world.data_center).or_default();
                dc_worlds.push((world_id, listings));
            }
        }

        let mut datacenters: Vec<DatacenterView> = worlds_by_dc
            .into_iter()
            .map(
                |(dc_id, mut worlds): (
                    WorldDcGroupTypeId,
                    Vec<(i32, Vec<(ListItem, ActiveListing)>)>,
                )| {
                    let dc_name = data
                        .world_dc_group_types
                        .get(&dc_id)
                        .map(|dc| dc.name.clone())
                        .unwrap_or_default();
                    worlds.sort_by_key(|(world_id, _)| *world_id);
                    for (_, listings) in &mut worlds {
                        listings.sort_by_key(|(item, _): &(ListItem, ActiveListing)| {
                            game_items
                                .get(&ItemId(item.item_id))
                                .map(|i| i.name.clone())
                                .unwrap_or_default()
                        });
                    }
                    DatacenterView {
                        name: dc_name,
                        worlds,
                    }
                },
            )
            .collect();
        datacenters.sort_by(|a, b| a.name.cmp(&b.name));
        datacenters
    });

    let data = xiv_gen_db::data();
    let game_items = &data.items;
    let retainer_map = Memo::new(move |_| {
        match retainers.get() {
            Some(Ok(retainers_data)) => {
                retainers_data.retainers
                    .into_iter()
                    .flat_map(|(_, r)| r.into_iter().map(|(r, _)| (r.id, r)))
                    .collect::<HashMap<_, _>>()
            }
            _ => HashMap::new(),
        }
    });

    view! {
        <div class="panel p-4 rounded-xl">
            {move || {
                datacenters()
                    .into_iter()
                    .map(|dc| {
                        view! {
                            <div class="mb-4">
                                <h2 class="text-2xl font-bold">{dc.name}</h2>
                                {dc.worlds
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
                                                                let retainer_name = retainer_map()
                                                                    .get(&listing.retainer_id)
                                                                    .map(|r| r.name.clone())
                                                                    .unwrap_or_default();
                                                                view! {
                                                                    <tr>
                                                                        <td class="w-12"><ItemIcon item_id=item.item_id icon_size=IconSize::Small /></td>
                                                                        <td>
                                                                            <div class="flex flex-row items-center gap-2">
                                                                                <span>{format!("{} {}", quantity_needed, item_name)}</span>
                                                                                <Clipboard clipboard_text=item_name.clone() />
                                                                            </div>
                                                                        </td>
                                                                        <td><Gil amount=listing.price_per_unit /></td>
                                                                        <td>{retainer_name}</td>
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
                                    .collect::<Vec<_>>()}
                            </div>
                        }
                    })
                    .collect::<Vec<_>>()
            }}
        </div>
    }
}
