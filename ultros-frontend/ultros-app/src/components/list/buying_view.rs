use crate::components::gil::*;
use crate::components::icon::Icon;
use crate::components::item_icon::*;
use crate::global_state::LocalWorldData;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use std::collections::HashMap;
use ultros_api_types::{
    ActiveListing, list::ListItem,
    world_helper::{AnyResult, AnySelector},
};
use xiv_gen::ItemId;

#[derive(Clone, Debug, PartialEq, Eq)]
struct GroupedListing {
    item_id: i32,
    item_name: String,
    price: i32,
    quantity: i32,
    list_item: ListItem,
    hq: bool,
    listing_id: i32,
}

#[component]
pub fn BuyingView(
    items: Vec<(ListItem, Vec<ActiveListing>)>,
    edit_item: Action<ListItem, Result<(), crate::error::AppError>>,
) -> impl IntoView {
    let world_data = use_context::<LocalWorldData>()
        .expect("LocalWorldData should be available")
        .0
        .expect("LocalWorldData should be loaded");
    let data = xiv_gen_db::data();
    let game_items = &data.items;

    let mut selected_listings: Vec<(i32, GroupedListing)> = Vec::new();

    for (list_item, mut listings) in items {
        let quantity = list_item.quantity.unwrap_or(1);
        let acquired = list_item.acquired.unwrap_or(0);
        let needed = quantity.saturating_sub(acquired);
        if needed <= 0 {
            continue;
        }

        listings.sort_by_key(|l| l.price_per_unit);
        let mut remaining = needed;
        for listing in listings {
            if remaining <= 0 {
                break;
            }
            if matches!(list_item.hq, Some(hq) if listing.hq != hq) {
                continue;
            }
            let buy_quantity = remaining.min(listing.quantity);
            let item_name = game_items
                .get(&ItemId(list_item.item_id))
                .map(|i| i.name.to_string())
                .unwrap_or_else(|| "Unknown Item".to_string());

            selected_listings.push((
                listing.world_id,
                GroupedListing {
                    item_id: list_item.item_id,
                    item_name,
                    price: listing.price_per_unit,
                    quantity: buy_quantity,
                    list_item: list_item.clone(),
                    hq: listing.hq,
                    listing_id: listing.id,
                },
            ));
            remaining -= buy_quantity;
        }
    }

    // Group by Datacenter -> World -> Listing
    type WorldMap = HashMap<i32, (String, Vec<GroupedListing>)>;
    let mut dc_groups: HashMap<i32, (String, WorldMap)> = HashMap::new();

    for (world_id, listing) in selected_listings {
        let world_res = world_data.lookup_selector(AnySelector::World(world_id));
        if let Some(AnyResult::World(world)) = world_res {
            let dc_res = world_data.lookup_selector(AnySelector::Datacenter(world.datacenter_id));
            if let Some(AnyResult::Datacenter(dc)) = dc_res {
                let dc_entry = dc_groups
                    .entry(dc.id)
                    .or_insert_with(|| (dc.name.clone(), HashMap::new()));
                let world_entry = dc_entry
                    .1
                    .entry(world.id)
                    .or_insert_with(|| (world.name.clone(), Vec::new()));
                world_entry.1.push(listing);
            }
        }
    }

    // Convert to sorted vectors for display
    type WorldList = Vec<(i32, String, Vec<GroupedListing>)>;
    let mut sorted_dcs: Vec<(i32, String, WorldList)> = dc_groups
        .into_iter()
        .map(|(dc_id, (dc_name, worlds))| {
            let mut sorted_worlds: WorldList = worlds
                .into_iter()
                .map(|(world_id, (world_name, listings))| (world_id, world_name, listings))
                .collect();
            sorted_worlds.sort_by(|a, b| a.1.cmp(&b.1));
            (dc_id, dc_name, sorted_worlds)
        })
        .collect();
    sorted_dcs.sort_by(|a, b| a.1.cmp(&b.1));

    view! {
        <div class="flex flex-col gap-6">
            {if sorted_dcs.is_empty() {
                Either::Left(
                    view! {
                        <div class="text-center py-8 text-[color:var(--color-text-muted)] italic">
                            "No items left to buy! 🎉"
                        </div>
                    },
                )
            } else {
                Either::Right(
                    view! {
                        <For
                            each=move || sorted_dcs.clone()
                            key=|(dc_id, _, _)| *dc_id
                            children=move |(_dc_id, dc_name, worlds)| {
                                view! {
                                    <div class="flex flex-col gap-4">
                                        <div class="text-2xl font-bold text-brand-400 border-b-2 border-brand-900/50 pb-1">
                                            {dc_name}
                                        </div>
                                        <div class="flex flex-col gap-6 pl-2">
                                            <For
                                                each=move || worlds.clone()
                                                key=|(world_id, _, _)| *world_id
                                                children=move |(_world_id, world_name, listings)| {
                                                    view! {
                                                        <div class="flex flex-col gap-2">
                                                            <div class="text-xl font-semibold text-brand-200 flex items-center gap-2">
                                                                <Icon icon=i::BiMapRegular />
                                                                {world_name}
                                                            </div>
                                                            <div class="flex flex-col gap-1 pl-6">
                                                                <For
                                                                    each=move || listings.clone()
                                                                    key=|listing| listing.listing_id
                                                                    children=move |listing| {
                                                                        let edit_item = edit_item;
                                                                        let mut list_item = listing.list_item.clone();
                                                                        let quantity_to_add = listing.quantity;
                                                                        view! {
                                                                            <div class="flex flex-row items-center gap-3 py-2 hover:bg-brand-900/20 rounded-lg px-3 group transition-colors">
                                                                                <ItemIcon
                                                                                    item_id=listing.item_id
                                                                                    icon_size=IconSize::Medium
                                                                                />
                                                                                <div class="flex-1 flex flex-col">
                                                                                    <div class="flex items-center gap-2">
                                                                                        <span class="font-bold text-lg">
                                                                                            {listing.quantity}
                                                                                        </span>
                                                                                        <span class="text-[color:var(--color-text)]">
                                                                                            {listing.item_name}
                                                                                        </span>
                                                                                        {listing
                                                                                            .hq
                                                                                            .then(|| {
                                                                                                view! {
                                                                                                    <span class="text-brand-400">"HQ"</span>
                                                                                                }
                                                                                            })}
                                                                                    </div>
                                                                                    <div class="flex items-center gap-1 text-sm text-[color:var(--color-text-muted)]">
                                                                                        "@"
                                                                                        <Gil amount=Signal::derive(move || listing.price) />
                                                                                        "each"
                                                                                    </div>
                                                                                </div>
                                                                                <button
                                                                                    class="btn btn-primary opacity-0 group-hover:opacity-100 transition-opacity flex items-center gap-2"
                                                                                    on:click=move |_| {
                                                                                        list_item.acquired = Some(
                                                                                            list_item.acquired.unwrap_or(0) + quantity_to_add,
                                                                                        );
                                                                                        let _ = edit_item.dispatch(list_item.clone());
                                                                                    }
                                                                                >
                                                                                    <Icon icon=i::BiCheckRegular />
                                                                                    <span>"Mark Purchased"</span>
                                                                                </button>
                                                                            </div>
                                                                        }
                                                                    }
                                                                />
                                                            </div>
                                                        </div>
                                                    }
                                                }
                                            />
                                        </div>
                                    </div>
                                }
                            }
                        />
                    },
                )
            }}
        </div>
    }
}
