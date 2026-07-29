use std::collections::HashSet;

use ultros_api_types::{ActiveListing, world_helper::WorldHelper};

pub(crate) fn listing_in_excluded_datacenter(
    listing: &ActiveListing,
    world_data: &WorldHelper,
    excluded_datacenters: &HashSet<String>,
) -> bool {
    if excluded_datacenters.is_empty() {
        return false;
    }

    listing.is_datacenter_excluded(excluded_datacenters, world_data)
}

pub(crate) fn retain_listing_for_datacenter_exclusions(
    listing: &ActiveListing,
    world_data: Option<&WorldHelper>,
    excluded_datacenters: &HashSet<String>,
) -> bool {
    if excluded_datacenters.is_empty() {
        return true;
    }

    world_data
        .map(|world_data| {
            !listing_in_excluded_datacenter(listing, world_data, excluded_datacenters)
        })
        .unwrap_or(true)
}

pub(crate) fn filter_active_listings(
    mut listings: Vec<ActiveListing>,
    world_data: Option<&WorldHelper>,
    excluded_worlds: &HashSet<i32>,
    excluded_datacenters: &HashSet<String>,
) -> Vec<ActiveListing> {
    if excluded_worlds.is_empty() && excluded_datacenters.is_empty() {
        return listings;
    }

    if !excluded_worlds.is_empty() {
        listings.retain(|listing| !excluded_worlds.contains(&listing.world_id));
    }

    listings.retain(|listing| {
        retain_listing_for_datacenter_exclusions(listing, world_data, excluded_datacenters)
    });
    listings
}

pub(crate) fn filter_listing_rows<T>(
    mut listings: Vec<(ActiveListing, T)>,
    world_data: Option<&WorldHelper>,
    excluded_worlds: &HashSet<i32>,
    excluded_datacenters: &HashSet<String>,
) -> Vec<(ActiveListing, T)> {
    if excluded_worlds.is_empty() && excluded_datacenters.is_empty() {
        return listings;
    }

    if !excluded_worlds.is_empty() {
        listings.retain(|(listing, _)| !excluded_worlds.contains(&listing.world_id));
    }

    listings
        .into_iter()
        .filter(|(listing, _)| {
            retain_listing_for_datacenter_exclusions(listing, world_data, excluded_datacenters)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use ultros_api_types::world::{Datacenter, Region, World, WorldData};

    fn world_data() -> WorldHelper {
        WorldData {
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
        .into()
    }

    fn listing(id: i32, world_id: i32) -> ActiveListing {
        ActiveListing {
            id,
            world_id,
            item_id: 1,
            retainer_id: 1,
            price_per_unit: id,
            quantity: 1,
            hq: false,
            timestamp: NaiveDate::from_ymd_opt(2026, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        }
    }

    #[test]
    fn empty_exclusion_set_preserves_listings() {
        let data = world_data();
        let listings = vec![listing(1, 100), listing(2, 110)];

        let result = filter_active_listings(
            listings.clone(),
            Some(&data),
            &HashSet::new(),
            &HashSet::new(),
        );

        assert_eq!(result, listings);
    }

    #[test]
    fn filters_listings_from_excluded_datacenter() {
        let data = world_data();
        let listings = vec![listing(1, 100), listing(2, 110)];
        let excluded = HashSet::from(["Aether".to_string()]);

        let result = filter_active_listings(listings, Some(&data), &HashSet::new(), &excluded);

        assert_eq!(result, vec![listing(2, 110)]);
    }

    #[test]
    fn filters_listings_from_excluded_world() {
        let data = world_data();
        let listings = vec![listing(1, 100), listing(2, 110)];
        let excluded_worlds = HashSet::from([100]);

        let result =
            filter_active_listings(listings, Some(&data), &excluded_worlds, &HashSet::new());

        assert_eq!(result, vec![listing(2, 110)]);
    }

    #[test]
    fn missing_world_data_preserves_current_behavior() {
        let listings = vec![listing(1, 100)];
        let excluded = HashSet::from(["Aether".to_string()]);

        let result = filter_active_listings(listings.clone(), None, &HashSet::new(), &excluded);

        assert_eq!(result, listings);
    }
}
