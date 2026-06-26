use leptos::either::Either;
use leptos::prelude::*;

use super::{datacenter_name::*, gil::*, world_name::*};
use crate::global_state::LocalWorldData;
use crate::i18n::*;
use ultros_api_types::world_helper::{AnyResult, AnySelector, WorldHelper};
use ultros_api_types::ActiveListing;

fn get_cheapest_listing(
    mut listings: Vec<ActiveListing>,
    quantity: i32,
    hq: Option<bool>,
    excluded_worlds: &[i32],
    excluded_datacenters: &[&str],
    world_helper: Option<&WorldHelper>,
) -> Vec<ActiveListing> {
    // Optimization: Filter out unwanted listings *before* sorting.
    // This significantly reduces the N in O(N log N) sorting time.
    listings.retain(|listing| {
        if listing.is_excluded(excluded_worlds) {
            return false;
        }
        if !excluded_datacenters.is_empty() {
            if let Some(world_helper) = world_helper {
                if let Some(AnyResult::World(world)) =
                    world_helper.lookup_selector(AnySelector::World(listing.world_id))
                {
                    if let Some(AnyResult::Datacenter(dc)) =
                        world_helper.lookup_selector(AnySelector::Datacenter(world.datacenter_id))
                    {
                        if excluded_datacenters.iter().any(|&name| name == dc.name) {
                            return false;
                        }
                    }
                }
            }
        }
        if let Some(hq) = hq {
            listing.hq == hq
        } else {
            true
        }
    });
    listings.sort_by_key(|listing| listing.price_per_unit);

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

#[component]
pub fn PriceViewer(
    quantity: i32,
    hq: Option<bool>,
    listings: Vec<ActiveListing>,
    #[prop(default = &[])] excluded_worlds: &'static [i32],
    #[prop(default = &[])] excluded_datacenters: &'static [&'static str],
) -> impl IntoView {
    let i18n = use_i18n();
    let world_data = use_context::<LocalWorldData>();
    let world_helper = world_data.as_ref().and_then(|d| d.0.as_ref().ok());

    let cheapest_listings = get_cheapest_listing(
        listings,
        quantity,
        hq,
        excluded_worlds,
        excluded_datacenters,
        world_helper.map(|h| h.as_ref()),
    );
    view! {
        <div class="flex flex-col gap-1">
            {if cheapest_listings.is_empty() {
                Either::Left(
                    view! {
                        <span class="text-[color:var(--color-text-muted)]">{t!(i18n, price_viewer_no_listing_data)}</span>
                    },
                )
            } else {
                Either::Right(
                    cheapest_listings
                        .iter()
                        .map(|listing| {
                            view! {
                                <div class="flex flex-wrap items-center gap-x-1 gap-y-0 text-sm">
                                    <span>{listing.quantity} "x"</span>
                                    <Gil amount=listing.price_per_unit />
                                    <span>{t!(i18n, price_viewer_on)}</span>
                                    <WorldName id=AnySelector::World(listing.world_id) />
                                    <span class="text-[color:var(--color-text-muted)]">"-"</span>
                                    <DatacenterName world_id=listing.world_id />
                                </div>
                            }
                        })
                        .collect::<Vec<_>>(),
                )
            }}
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn mock_listing(id: i32, price_per_unit: i32, quantity: i32, hq: bool) -> ActiveListing {
        ActiveListing {
            id,
            world_id: 1,
            item_id: 1,
            retainer_id: 1,
            price_per_unit,
            quantity,
            hq,
            timestamp: Utc::now().naive_utc(),
        }
    }

    #[test]
    fn test_get_cheapest_listing_exact_quantity() {
        let listings = vec![
            mock_listing(1, 100, 5, false),
            mock_listing(2, 200, 5, false),
            mock_listing(3, 300, 5, false),
        ];
        let result = get_cheapest_listing(listings, 10, None, &[], &[], None);
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
        let result = get_cheapest_listing(listings, 12, None, &[], &[], None);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, 1);
        assert_eq!(result[1].id, 2);
    }

    #[test]
    fn test_get_cheapest_listing_hq_filter() {
        let listings = vec![
            mock_listing(1, 100, 5, false), // NQ, skipped
            mock_listing(2, 200, 5, true),  // HQ, taken
            mock_listing(3, 300, 5, true),  // HQ, taken
            mock_listing(4, 400, 5, false), // NQ, skipped
        ];
        let result = get_cheapest_listing(listings, 10, Some(true), &[], &[], None);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, 2);
        assert_eq!(result[1].id, 3);
    }

    #[test]
    fn test_get_cheapest_listing_nq_filter() {
        let listings = vec![
            mock_listing(1, 100, 5, true),  // HQ, skipped
            mock_listing(2, 200, 5, false), // NQ, taken
            mock_listing(3, 300, 5, false), // NQ, taken
            mock_listing(4, 400, 5, true),  // HQ, skipped
        ];
        let result = get_cheapest_listing(listings, 10, Some(false), &[], &[], None);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, 2);
        assert_eq!(result[1].id, 3);
    }

    #[test]
    fn test_get_cheapest_listing_no_hq_filter() {
        let listings = vec![
            mock_listing(1, 200, 5, false), // NQ
            mock_listing(2, 100, 5, true),  // HQ
            mock_listing(3, 300, 5, false), // NQ
            mock_listing(4, 400, 5, true),  // HQ
        ];
        // Should pick id=2 then id=1
        let result = get_cheapest_listing(listings, 10, None, &[], &[], None);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, 2);
        assert_eq!(result[1].id, 1);
    }

    #[test]
    fn test_get_cheapest_listing_empty() {
        let listings = vec![];
        let result = get_cheapest_listing(listings, 10, None, &[], &[], None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_cheapest_listing_insufficient_quantity() {
        let listings = vec![
            mock_listing(1, 100, 5, false),
            mock_listing(2, 200, 2, false),
        ];
        // We ask for 10, but only 7 are available. It should return all of them.
        let result = get_cheapest_listing(listings, 10, None, &[], &[], None);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, 1);
        assert_eq!(result[1].id, 2);
    }

    #[test]
    fn test_get_cheapest_listing_excluded_datacenter() {
        use ultros_api_types::world::{Datacenter, Region, World, WorldData};

        let world_data: WorldHelper = WorldData {
            regions: vec![Region {
                id: 1,
                name: "North-America".into(),
                datacenters: vec![
                    Datacenter {
                        id: 10,
                        name: "Aether".into(),
                        region_id: 1,
                        worlds: vec![World {
                            id: 100,
                            name: "Adamantoise".into(),
                            datacenter_id: 10,
                        }],
                    },
                    Datacenter {
                        id: 11,
                        name: "Primal".into(),
                        region_id: 1,
                        worlds: vec![World {
                            id: 110,
                            name: "Behemoth".into(),
                            datacenter_id: 11,
                        }],
                    },
                ],
            }],
        }
        .into();

        let mut l1 = mock_listing(1, 100, 5, false);
        l1.world_id = 100; // Aether
        let mut l2 = mock_listing(2, 200, 5, false);
        l2.world_id = 110; // Primal

        let listings = vec![l1, l2];

        // Exclude Aether
        let result = get_cheapest_listing(
            listings,
            10,
            None,
            &[],
            &["Aether"],
            Some(&world_data),
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, 2);
        assert_eq!(result[0].world_id, 110);
    }
}
