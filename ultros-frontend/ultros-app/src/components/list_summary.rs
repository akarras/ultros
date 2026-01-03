use std::collections::HashMap;

use leptos::prelude::*;
use ultros_api_types::{ActiveListing, list::ListItem};
use xiv_gen::ItemId;

use super::gil::*;
use crate::global_state::LocalWorldData;
use ultros_api_types::world_helper::{AnyResult, AnySelector};

/// Represents the total price for items from a specific world
#[derive(Clone, Debug)]
struct WorldPrice {
    world_name: String,
    datacenter_id: i32,
    datacenter_name: String,
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
    world_data: &ultros_api_types::world_helper::WorldHelper,
) -> (i32, HashMap<i32, WorldPrice>) {
    let mut grand_total = 0;
    let mut world_prices: HashMap<i32, WorldPrice> = HashMap::new();

    for (list_item, listings) in items {
        let quantity = list_item.quantity.unwrap_or(1);
        let acquired = list_item.acquired.unwrap_or(0);
        let quantity = quantity.saturating_sub(acquired);
        if quantity <= 0 {
            continue;
        }
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
                .or_insert_with(|| {
                    // Look up world and datacenter information
                    let world_result =
                        world_data.lookup_selector(AnySelector::World(listing.world_id));
                    let (world_name, datacenter_id, datacenter_name) =
                        if let Some(AnyResult::World(world)) = world_result {
                            let dc_result = world_data
                                .lookup_selector(AnySelector::Datacenter(world.datacenter_id));
                            let datacenter_name = dc_result
                                .and_then(|dc| dc.as_datacenter())
                                .map(|dc| dc.name.clone())
                                .unwrap_or_else(|| "Unknown".to_string());
                            (world.name.clone(), world.datacenter_id, datacenter_name)
                        } else {
                            ("Unknown".to_string(), 0, "Unknown".to_string())
                        };

                    WorldPrice {
                        world_name,
                        datacenter_id,
                        datacenter_name,
                        total_price: price,
                        item_count: 1,
                    }
                });
        }
    }

    (grand_total, world_prices)
}

#[component]
pub fn ListSummary(items: Vec<(ListItem, Vec<ActiveListing>)>) -> impl IntoView {
    let data = xiv_gen_db::data();
    let game_items = &data.items;

    // Get world data from context
    let world_data = use_context::<LocalWorldData>()
        .expect("LocalWorldData should be available")
        .0
        .expect("LocalWorldData should be loaded");

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

    let (grand_total, world_prices) = calculate_list_totals(marketable_items, &world_data);

    if grand_total == 0 && world_prices.is_empty() {
        return view! {
            <div class="panel p-4 rounded-xl mt-4 border-2 border-[color:var(--brand-border)]">
                <div class="text-center font-bold text-lg text-[color:var(--brand-fg)]">
                    "All items acquired! 🎉"
                </div>
            </div>
        }
        .into_any();
    }

    // Group by datacenter and calculate datacenter totals
    let mut datacenter_groups: HashMap<i32, Vec<WorldPrice>> = HashMap::new();
    let mut datacenter_totals: HashMap<i32, (String, i32, usize)> = HashMap::new();

    for (_, world_price) in world_prices {
        datacenter_groups
            .entry(world_price.datacenter_id)
            .or_default()
            .push(world_price.clone());

        datacenter_totals
            .entry(world_price.datacenter_id)
            .and_modify(|(_, price, count)| {
                *price += world_price.total_price;
                *count += world_price.item_count;
            })
            .or_insert((
                world_price.datacenter_name.clone(),
                world_price.total_price,
                world_price.item_count,
            ));
    }

    // Sort datacenters by total item count (descending)
    let mut sorted_datacenters: Vec<_> = datacenter_totals.into_iter().collect();
    sorted_datacenters.sort_by(|(_, (_, _, a_item_count)), (_, (_, _, b_item_count))| {
        b_item_count.cmp(a_item_count)
    });

    // Sort worlds within each datacenter: by item count (descending), then alphabetically
    for worlds in datacenter_groups.values_mut() {
        worlds.sort_by(|a, b| match b.item_count.cmp(&a.item_count) {
            std::cmp::Ordering::Equal => a.world_name.cmp(&b.world_name),
            other => other,
        });
    }

    // Track if we have multiple datacenters (enables collapsible behavior)
    let has_multiple_datacenters = sorted_datacenters.len() > 1;

    // Create a signal to track which datacenters are expanded
    // Initially expand all if single datacenter, or collapse all if multiple
    let (expanded_datacenters, set_expanded_datacenters) = signal(if has_multiple_datacenters {
        std::collections::HashSet::<i32>::new()
    } else {
        sorted_datacenters.iter().map(|(dc_id, _)| *dc_id).collect()
    });

    view! {
        <div class="panel p-4 rounded-xl mt-4 border-2 border-[color:var(--brand-border)]">
            <div class="flex flex-row items-center justify-center gap-2">
                <span class="text-lg font-semibold text-[color:var(--brand-fg)]">
                    "Estimated Remaining Cost:"
                </span>
                <Gil amount=Signal::derive(move || grand_total) />
            </div>

            <div class="mt-3 flex flex-col gap-3 text-sm">
                {sorted_datacenters
                    .into_iter()
                    .map(|(dc_id, (dc_name, dc_total, dc_count))| {
                        let worlds = datacenter_groups.get(&dc_id).cloned().unwrap_or_default();
                        let is_expanded = Signal::derive(move || {
                            expanded_datacenters.with(|set| set.contains(&dc_id))
                        });

                        view! {
                            <div class="flex flex-col gap-1">
                                <div
                                    class=move || {
                                        let base = "flex flex-row items-center gap-2 justify-between font-semibold text-brand-300 bg-brand-900/20 px-2 py-1 rounded";
                                        if has_multiple_datacenters {
                                            format!("{} cursor-pointer hover:bg-brand-900/30 transition-colors", base)
                                        } else {
                                            base.to_string()
                                        }
                                    }
                                    on:click=move |_| {
                                        if has_multiple_datacenters {
                                            set_expanded_datacenters.update(|set| {
                                                if set.contains(&dc_id) {
                                                    set.remove(&dc_id);
                                                } else {
                                                    set.insert(dc_id);
                                                }
                                            });
                                        }
                                    }
                                >
                                    <div class="flex items-center gap-2">
                                        {move || has_multiple_datacenters.then(|| {
                                            view! {
                                                <span class="text-brand-400">
                                                    {if is_expanded() { "▼" } else { "▶" }}
                                                </span>
                                            }
                                        })}
                                        <span>{dc_name}</span>
                                    </div>
                                    <div class="flex flex-row items-center gap-2">
                                        <Gil amount=Signal::derive(move || dc_total) />
                                        <span class="text-[color:var(--color-text-muted)] font-normal">
                                            {format!(
                                                "({} item{})",
                                                dc_count,
                                                if dc_count == 1 { "" } else { "s" },
                                            )}
                                        </span>
                                    </div>
                                </div>
                                <div
                                    class="flex flex-col gap-1 pl-4 overflow-hidden transition-all duration-200"
                                    class:hidden=move || has_multiple_datacenters && !is_expanded()
                                >
                                    {worlds
                                        .into_iter()
                                        .map(|world_price| {
                                            let total_price = world_price.total_price;
                                            let item_count = world_price.item_count;
                                            let world_name = world_price.world_name;
                                            view! {
                                                <div class="flex flex-row items-center gap-2 justify-between">
                                                    <span class="text-[color:var(--color-text)]">{world_name}</span>
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
                    })
                    .collect::<Vec<_>>()}
            </div>
        </div>
    }
    .into_any()
}
