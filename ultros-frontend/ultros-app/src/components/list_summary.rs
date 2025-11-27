use std::collections::HashMap;

use leptos::prelude::*;
use thousands::Separable;
use ultros_api_types::{list::ListItem, ActiveListing};
use xiv_gen::ItemId;

use super::{datacenter_name::*, gil::*, tooltip::*, world_name::*};
use ultros_api_types::world_helper::AnySelector;

/// Represents the total price for items from a specific world
#[derive(Clone, Copy, Debug)]
struct WorldPrice {
    world_id: i32,
    total_price: i32,
    item_count: usize,
}

/// Find the cheapest listings for a given item based on quantity and HQ preference
fn get_cheapest_listing(
    mut listings: Vec<ActiveListing>,
    quantity: i32,
    hq: Option<bool>,
) -> Vec<ActiveListing> {
    listings.sort_by_key(|listing| listing.price_per_unit);
    let quantity_needed = quantity;
    let mut current_quantity = 0;
    listings
        .into_iter()
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

/// Calculate the total price and breakdown by world for all items in the list
fn calculate_list_totals(
    items: Vec<(ListItem, Vec<ActiveListing>)>,
) -> (i32, HashMap<i32, WorldPrice>) {
    let mut grand_total = 0;
    let mut world_prices: HashMap<i32, WorldPrice> = HashMap::new();

    for (list_item, listings) in items {
        let quantity = list_item.quantity.unwrap_or(1);
        let hq = list_item.hq;

        let cheapest_listings = get_cheapest_listing(listings, quantity, hq);

        for listing in cheapest_listings {
            let price = listing.price_per_unit * listing.quantity;
            grand_total += price;

            world_prices
                .entry(listing.world_id)
                .and_modify(|wp| {
                    wp.total_price += price;
                    wp.item_count += 1;
                })
                .or_insert(WorldPrice {
                    world_id: listing.world_id,
                    total_price: price,
                    item_count: 1,
                });
        }
    }

    (grand_total, world_prices)
}

#[component]
pub fn ListSummary(items: Vec<(ListItem, Vec<ActiveListing>)>) -> impl IntoView {
    let data = xiv_gen_db::data();
    let game_items = &data.items;

    // Filter out items that are not on the market board
    let marketable_items: Vec<_> = items
        .into_iter()
        .filter(|(item, _)| {
            game_items
                .get(&ItemId(item.item_id))
                .map(|i| i.item_search_category.0 > 1)
                .unwrap_or(false)
        })
        .collect();

    if marketable_items.is_empty() {
        return view! {
            <div class="panel p-4 rounded-xl mt-4">
                <div class="text-center text-[color:var(--color-text-muted)]">
                    "No marketable items in list"
                </div>
            </div>
        }
        .into_any();
    }

    let (grand_total, world_prices) = calculate_list_totals(marketable_items);

    // Sort worlds by total price (descending)
    let mut sorted_worlds: Vec<_> = world_prices.into_iter().collect();
    sorted_worlds.sort_by(|a, b| b.1.total_price.cmp(&a.1.total_price));

    // Clone sorted_worlds for the tooltip closure
    let sorted_worlds_for_tooltip = sorted_worlds.clone();
    let tooltip_content = Signal::derive(move || {
        if sorted_worlds_for_tooltip.is_empty() {
            "No price data available".to_string()
        } else {
            let mut content = "Total by World:\n".to_string();
            for (_, world_price) in sorted_worlds_for_tooltip.iter() {
                content.push_str(&format!(
                    "• World {}: {} gil ({} item{})\n",
                    world_price.world_id,
                    world_price.total_price.separate_with_commas(),
                    world_price.item_count,
                    if world_price.item_count == 1 { "" } else { "s" }
                ));
            }
            content
        }
    });

    view! {
        <div class="panel p-4 rounded-xl mt-4 border-2 border-[color:var(--brand-border)]">
            <Tooltip tooltip_text=tooltip_content>
                <div class="flex flex-row items-center justify-center gap-2">
                    <span class="text-lg font-semibold text-[color:var(--brand-fg)]">
                        "List Total:"
                    </span>
                    <Gil amount=Signal::derive(move || grand_total) />
                    <span class="text-sm text-[color:var(--color-text-muted)] ml-2">
                        "(hover for world breakdown)"
                    </span>
                </div>
            </Tooltip>

            <div class="mt-3 flex flex-col gap-1 text-sm">
                {sorted_worlds
                    .into_iter()
                    .map(|(_, world_price)| {
                        let world_id = world_price.world_id;
                        let total_price = world_price.total_price;
                        let item_count = world_price.item_count;
                        view! {
                            <div class="flex flex-row items-center gap-2 justify-between">
                                <div class="flex flex-row items-center gap-1">
                                    <WorldName id=AnySelector::World(world_id) />
                                    <span class="text-[color:var(--color-text-muted)]">"-"</span>
                                    <DatacenterName world_id=world_id />
                                </div>
                                <div class="flex flex-row items-center gap-2">
                                    <Gil amount=Signal::derive(move || total_price) />
                                    <span class="text-[color:var(--color-text-muted)]">
                                        {format!(
                                            "({} item{})",
                                            item_count,
                                            if item_count == 1 { "" } else { "s" },
                                        )}
                                    </span>
                                </div>
                            </div>
                        }
                    })
                    .collect::<Vec<_>>()}
            </div>
        </div>
    }
    .into_any()
}

