use crate::components::gil::*;
use crate::components::icon::Icon;
use crate::components::item_icon::*;
use crate::global_state::LocalWorldData;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string, use_i18n};
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use std::collections::HashMap;
use ultros_api_types::{
    ActiveListing,
    list::ListItem,
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
    let i18n = use_i18n();
    let world_data = use_context::<LocalWorldData>()
        .expect("LocalWorldData should be available")
        .0
        .expect("LocalWorldData should be loaded");
    let data = tracked_data();
    let game_items = &data.items;
    let unknown_item_label = t_string!(i18n, unknown_item).to_string();

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
                .unwrap_or_else(|| unknown_item_label.clone());

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
        <div class="flex flex-col gap-4">
            {if sorted_dcs.is_empty() {
                Either::Left(
                    view! {
                        <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] p-8 text-center text-[color:var(--color-text-muted)]">
                            {t!(i18n, list_buying_view_empty_state)}
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
                                    <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] overflow-hidden">
                                        <div class="flex items-center justify-between border-b border-[color:var(--color-outline)] px-4 py-3">
                                            <div class="text-lg font-bold text-[color:var(--brand-fg)]">
                                                {dc_name}
                                            </div>
                                        </div>
                                        <div class="divide-y divide-[color:var(--color-outline)]">
                                            <For
                                                each=move || worlds.clone()
                                                key=|(world_id, _, _)| *world_id
                                                children=move |(_world_id, world_name, listings)| {
                                                    view! {
                                                        <div class="p-4">
                                                            <div class="mb-3 flex items-center gap-2 text-sm font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]">
                                                                <Icon icon=i::BiMapRegular />
                                                                {world_name}
                                                            </div>
                                                            <div class="grid gap-2">
                                                                <For
                                                                    each=move || listings.clone()
                                                                    key=|listing| listing.listing_id
                                                                    children=move |listing| {
                                                                        let edit_item = edit_item;
                                                                        let mut list_item = listing.list_item.clone();
                                                                        let quantity_to_add = listing.quantity;
                                                                        view! {
                                                                            <div class="group flex flex-col gap-3 rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-elevated)] px-3 py-3 transition-colors hover:border-[color:var(--color-outline-strong)] sm:flex-row sm:items-center">
                                                                                <div class="flex min-w-0 flex-1 items-center gap-3">
                                                                                    <ItemIcon
                                                                                        item_id=listing.item_id
                                                                                        icon_size=IconSize::Medium
                                                                                    />
                                                                                    <div class="min-w-0 flex-1">
                                                                                        <div class="flex min-w-0 flex-wrap items-center gap-2">
                                                                                            <span class="rounded-md bg-[color:var(--brand-bg)]/30 px-2 py-0.5 text-sm font-bold text-[color:var(--brand-fg)]">
                                                                                                {listing.quantity}
                                                                                            </span>
                                                                                            <span class="min-w-0 truncate font-semibold text-[color:var(--color-text)]">
                                                                                                {listing.item_name}
                                                                                            </span>
                                                                                            {listing
                                                                                                .hq
                                                                                                .then(|| {
                                                                                                    view! {
                                                                                                        <span class="rounded-md border border-[color:var(--brand-ring)]/40 px-2 py-0.5 text-xs font-bold text-[color:var(--brand-fg)]">{t!(i18n, list_view_hq)}</span>
                                                                                                    }
                                                                                                })}
                                                                                        </div>
                                                                                        <div class="mt-1 flex items-center gap-1 text-sm text-[color:var(--color-text-muted)]">
                                                                                            "@"
                                                                                            <Gil amount=Signal::derive(move || listing.price) />
                                                                                            {t!(i18n, list_buying_view_each)}
                                                                                        </div>
                                                                                    </div>
                                                                                </div>
                                                                                <button
                                                                                    class="btn-primary shrink-0"
                                                                                    on:click=move |_| {
                                                                                        list_item.acquired = Some(
                                                                                            list_item.acquired.unwrap_or(0) + quantity_to_add,
                                                                                        );
                                                                                        let _ = edit_item.dispatch(list_item.clone());
                                                                                    }
                                                                                >
                                                                                    <Icon icon=i::BiCheckRegular />
                                                                                    <span>{t!(i18n, list_buying_view_mark_purchased)}</span>
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
