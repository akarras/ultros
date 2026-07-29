use std::collections::{HashMap, HashSet};

use crate::components::icon::Icon;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string, use_i18n};
use icondata as i;
use leptos::prelude::*;
use ultros_api_types::{ActiveListing, list::ListItem};
use xiv_gen::ItemId;

use crate::components::gil::*;
use crate::global_state::LocalWorldData;
use ultros_api_types::world_helper::{AnyResult, AnySelector, WorldHelper};

/// Represents the total price for items from a specific world
#[derive(Clone, Debug, PartialEq)]
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
    excluded_worlds: &[i32],
    excluded_datacenters: &HashSet<String>,
    world_helper: Option<&WorldHelper>,
) -> Vec<ActiveListing> {
    // ⚡ Bolt: Filter out unwanted listings before sorting.
    // This reduces the array size N significantly, making the O(N log N) sorting step much faster.
    listings.retain(|listing| {
        if listing.is_excluded(excluded_worlds) {
            return false;
        }
        if let Some(world_helper) = world_helper
            && listing.is_datacenter_excluded(excluded_datacenters, world_helper)
        {
            return false;
        }
        if let Some(hq) = hq {
            listing.hq == hq
        } else {
            true
        }
    });
    listings.sort_unstable_by_key(|listing| listing.price_per_unit);

    let quantity_needed = quantity;
    let mut current_quantity = 0;
    listings
        .into_iter()
        .take_while(|listing| {
            let needed_more = current_quantity < quantity_needed;
            current_quantity += listing.quantity;
            needed_more
        })
        .collect::<Vec<_>>()
}

/// Calculate the total price and breakdown by world for all items in the list
fn calculate_list_totals(
    items: Vec<(ListItem, Vec<ActiveListing>)>,
    world_data: &WorldHelper,
    unknown_label: &str,
    excluded_worlds: &[i32],
    excluded_datacenters: &HashSet<String>,
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

        let cheapest_listings = get_cheapest_listing(
            listings,
            quantity,
            hq,
            excluded_worlds,
            excluded_datacenters,
            Some(world_data),
        );

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
                                .unwrap_or_else(|| unknown_label.to_string());
                            (world.name.clone(), world.datacenter_id, datacenter_name)
                        } else {
                            (unknown_label.to_string(), 0, unknown_label.to_string())
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
pub fn ListSummary(
    items: Vec<(ListItem, Vec<ActiveListing>)>,
    #[prop(default = &[])] excluded_worlds: &'static [i32],
    #[prop(into, default = Signal::derive(HashSet::new))] excluded_datacenters: Signal<
        HashSet<String>,
    >,
) -> impl IntoView {
    let i18n = use_i18n();
    let data = tracked_data();
    let game_items = &data.items;
    let unknown_label = t_string!(i18n, list_summary_unknown).to_string();

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
                .map(|i| i.item_search_category > 1)
                .unwrap_or(false)
        })
        .collect();

    if marketable_items.is_empty() {
        return view! {
            <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] p-4">
                <div class="text-center text-sm text-[color:var(--color-text-muted)]">
                    {t!(i18n, list_summary_no_marketable)}
                </div>
            </div>
        }
        .into_any();
    }

    let list_totals = Memo::new(move |_| {
        excluded_datacenters.with(|excluded_datacenters| {
            calculate_list_totals(
                marketable_items.clone(),
                &world_data,
                &unknown_label,
                excluded_worlds,
                excluded_datacenters,
            )
        })
    });

    let all_acquired = move || {
        let (grand_total, world_prices) = list_totals.get();
        grand_total == 0 && world_prices.is_empty()
    };
    if all_acquired() {
        return view! {
            <div class="rounded-lg border border-[color:var(--brand-ring)]/40 bg-[color:var(--color-background-panel)] p-4">
                <div class="text-center text-lg font-bold text-[color:var(--brand-fg)]">
                    {t!(i18n, list_summary_all_acquired)}
                </div>
            </div>
        }
        .into_any();
    }

    // Group by datacenter and calculate datacenter totals
    let summary_data = Memo::new(move |_| {
        let (_grand_total, world_prices) = list_totals.get();
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
        (sorted_datacenters, datacenter_groups)
    });

    // Create a signal to track which datacenters are expanded
    // Initially expand all if single datacenter, or collapse all if multiple
    let (expanded_datacenters, set_expanded_datacenters) = signal(HashSet::<i32>::new());
    Effect::new(move |prev: Option<bool>| {
        let (sorted_datacenters, _) = summary_data.get();
        let has_multiple = sorted_datacenters.len() > 1;
        if prev.is_none() || prev != Some(has_multiple) {
            if !has_multiple {
                set_expanded_datacenters
                    .set(sorted_datacenters.iter().map(|(id, _)| *id).collect());
            } else {
                set_expanded_datacenters.set(HashSet::new());
            }
        }
        has_multiple
    });

    view! {
        <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)]">
            <div class="flex flex-col gap-2 border-b border-[color:var(--color-outline)] px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
                <span class="text-sm font-semibold uppercase tracking-wide text-[color:var(--color-text-muted)]">
                    {t!(i18n, list_summary_estimated_remaining_cost)}
                </span>
                <div class="text-lg font-bold text-[color:var(--brand-fg)]">
                    <Gil amount=Signal::derive(move || list_totals.get().0) />
                </div>
            </div>

            <div class="flex flex-col gap-2 p-3 text-sm">
                {move || {
                    let (sorted_datacenters, datacenter_groups) = summary_data.get();
                    let has_multiple_datacenters = sorted_datacenters.len() > 1;
                    sorted_datacenters
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
                                        let base = "flex flex-row items-center gap-2 justify-between font-semibold text-[color:var(--brand-fg)] bg-[color:var(--color-background-elevated)] px-3 py-2 rounded-lg border border-[color:var(--color-outline)]";
                                        if has_multiple_datacenters {
                                            format!("{} cursor-pointer hover:border-[color:var(--color-outline-strong)] transition-colors", base)
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
                                                <span class="text-[color:var(--color-text-muted)]">
                                                    <Icon icon=Signal::derive(move || {
                                                        if is_expanded() {
                                                            i::BiChevronDownRegular
                                                        } else {
                                                            i::BiChevronRightRegular
                                                        }
                                                    }) />
                                                </span>
                                            }
                                        })}
                                        <span>{dc_name}</span>
                                    </div>
                                    <div class="flex flex-row items-center gap-2">
                                        <Gil amount=Signal::derive(move || dc_total) />
                                        <span class="text-[color:var(--color-text-muted)] font-normal">
                                            {t!(i18n, list_summary_item_count, count = dc_count)}
                                        </span>
                                    </div>
                                </div>
                                <div
                                    class="flex flex-col gap-1 overflow-hidden pl-4 transition-all duration-200"
                                    class:hidden=move || has_multiple_datacenters && !is_expanded()
                                >
                                    {worlds
                                        .into_iter()
                                        .map(|world_price| {
                                            let total_price = world_price.total_price;
                                            let item_count = world_price.item_count;
                                            let world_name = world_price.world_name;
                                            view! {
                                                <div class="flex flex-row items-center gap-2 justify-between px-3 py-1">
                                                    <span class="text-[color:var(--color-text)]">{world_name}</span>
                                                    <div class="flex flex-row items-center gap-2">
                                                        <Gil amount=Signal::derive(move || total_price) />
                                                        <span class="text-[color:var(--color-text-muted)]">
                                                            {t!(i18n, list_summary_item_count, count = item_count)}
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
                    .collect::<Vec<_>>()
                }}
            </div>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn mock_listing(id: i32, price_per_unit: i32, quantity: i32, hq: bool) -> ActiveListing {
        ActiveListing {
            id,
            world_id: 1,
            item_id: 1,
            retainer_id: 1,
            price_per_unit,
            quantity,
            hq,
            timestamp: NaiveDate::from_ymd_opt(2023, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        }
    }

    #[test]
    fn test_get_cheapest_listing_exact_quantity() {
        let listings = vec![
            mock_listing(1, 100, 5, false),
            mock_listing(2, 200, 5, false),
            mock_listing(3, 300, 5, false),
        ];
        let result = get_cheapest_listing(listings, 10, None, &[], &HashSet::new(), None);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, 1);
        assert_eq!(result[1].id, 2);
    }

    #[test]
    fn test_get_cheapest_listing_exceeds_quantity() {
        let listings = vec![
            mock_listing(1, 100, 5, false),
            mock_listing(2, 200, 10, false),
            mock_listing(3, 300, 5, false),
        ];
        // We need 12. We take the 5 from id=1, and we need 7 more, so we take the 10 from id=2.
        let result = get_cheapest_listing(listings, 12, None, &[], &HashSet::new(), None);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, 1);
        assert_eq!(result[1].id, 2);
    }

    #[test]
    fn test_calculate_list_totals() {
        use ultros_api_types::world::{Datacenter, Region, World, WorldData};
        use ultros_api_types::world_helper::WorldHelper;

        let world_data: WorldHelper = WorldData {
            regions: vec![Region {
                id: 1,
                name: "North-America".into(),
                datacenters: vec![Datacenter {
                    id: 10,
                    name: "Aether".into(),
                    region_id: 1,
                    worlds: vec![
                        World {
                            id: 100,
                            name: "Adamantoise".into(),
                            datacenter_id: 10,
                        },
                        World {
                            id: 101,
                            name: "Cactuar".into(),
                            datacenter_id: 10,
                        },
                    ],
                }],
            }],
        }
        .into();

        let item1 = ListItem {
            id: 1,
            list_id: 1,
            item_id: 1,
            quantity: Some(5),
            acquired: Some(0),
            hq: None,
            target_price: None,
        };
        let listings1 = vec![mock_listing(1, 100, 5, false)]; // world_id=1, total 500

        let item2 = ListItem {
            id: 2,
            list_id: 1,
            item_id: 2,
            quantity: Some(10),
            acquired: Some(2), // 8 needed
            hq: None,
            target_price: None,
        };
        // Needs 8.
        let mut listing2_a = mock_listing(2, 200, 5, false);
        listing2_a.world_id = 100; // Adamantoise
        let mut listing2_b = mock_listing(3, 300, 5, false);
        listing2_b.world_id = 101; // Cactuar

        let (total, world_prices) = calculate_list_totals(
            vec![(item1, listings1), (item2, vec![listing2_a, listing2_b])],
            &world_data,
            "Unknown",
            &[],
            &HashSet::new(),
        );

        // Grand total:
        // Item 1: 5 * 100 = 500
        // Item 2: Needs 8. Gets 5 @ 200 = 1000. Gets 5 @ 300 = 1500.
        // Total = 500 + 2500 = 3000.
        assert_eq!(total, 3000);

        // Verify World Prices
        // world_id=1 is Unknown (not in sample_world_data)
        assert_eq!(world_prices.get(&1).unwrap().total_price, 500);
        assert_eq!(world_prices.get(&1).unwrap().datacenter_name, "Unknown");
        assert_eq!(world_prices.get(&1).unwrap().world_name, "Unknown");
        assert_eq!(world_prices.get(&1).unwrap().datacenter_id, 0);

        // world_id=100 (Adamantoise, Aether)
        assert_eq!(world_prices.get(&100).unwrap().total_price, 1000);
        assert_eq!(world_prices.get(&100).unwrap().datacenter_name, "Aether");
        assert_eq!(world_prices.get(&100).unwrap().world_name, "Adamantoise");
        assert_eq!(world_prices.get(&100).unwrap().datacenter_id, 10);

        // world_id=101 (Cactuar, Aether)
        assert_eq!(world_prices.get(&101).unwrap().total_price, 1500);
        assert_eq!(world_prices.get(&101).unwrap().datacenter_name, "Aether");
        assert_eq!(world_prices.get(&101).unwrap().world_name, "Cactuar");
        assert_eq!(world_prices.get(&101).unwrap().datacenter_id, 10);
    }

    #[test]
    fn test_get_cheapest_listing_with_excluded_world() {
        let mut l1 = mock_listing(1, 100, 5, false);
        l1.world_id = 10;
        let mut l2 = mock_listing(2, 150, 5, false);
        l2.world_id = 20;
        let mut l3 = mock_listing(3, 200, 5, false);
        l3.world_id = 30;

        let listings = vec![l1, l2, l3];

        // Case 1: Exclude the cheapest world (10)
        let result = get_cheapest_listing(listings.clone(), 5, None, &[10], &HashSet::new(), None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, 2); // Should pick the next cheapest

        // Case 2: Exclude all current listings
        let result = get_cheapest_listing(
            listings.clone(),
            5,
            None,
            &[10, 20, 30],
            &HashSet::new(),
            None,
        );
        assert!(result.is_empty());

        // Case 3: Exclude none
        let result = get_cheapest_listing(listings, 5, None, &[], &HashSet::new(), None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, 1);
    }

    #[test]
    fn test_calculate_list_totals_with_excluded_worlds() {
        use ultros_api_types::world::{Datacenter, Region, World, WorldData};
        use ultros_api_types::world_helper::WorldHelper;

        let world_data: WorldHelper = WorldData {
            regions: vec![Region {
                id: 1,
                name: "North-America".into(),
                datacenters: vec![Datacenter {
                    id: 10,
                    name: "Aether".into(),
                    region_id: 1,
                    worlds: vec![
                        World {
                            id: 100,
                            name: "Adamantoise".into(),
                            datacenter_id: 10,
                        },
                        World {
                            id: 101,
                            name: "Cactuar".into(),
                            datacenter_id: 10,
                        },
                    ],
                }],
            }],
        }
        .into();

        let item = ListItem {
            id: 1,
            list_id: 1,
            item_id: 1,
            quantity: Some(10),
            acquired: Some(0),
            hq: None,
            target_price: None,
        };

        let mut l1 = mock_listing(1, 100, 10, false);
        l1.world_id = 100; // Cheapest but we will exclude it
        let mut l2 = mock_listing(2, 200, 10, false);
        l2.world_id = 101; // Next cheapest

        // Exclude world 100
        let (total, world_prices) = calculate_list_totals(
            vec![(item, vec![l1, l2])],
            &world_data,
            "Unknown",
            &[100],
            &HashSet::new(),
        );

        // Total should be 10 * 200 = 2000 (from world 101)
        assert_eq!(total, 2000);
        assert_eq!(world_prices.len(), 1);
        assert!(world_prices.contains_key(&101));
        assert!(!world_prices.contains_key(&100));
    }
}
