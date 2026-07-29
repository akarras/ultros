use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
/// This mostly matches with the ultros-db/entity type, but doesn't include the sea-orm specifics
/// See [ultros-db::active_listing::Entity]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveListing {
    pub id: i32,
    pub world_id: i32,
    pub item_id: i32,
    pub retainer_id: i32,
    pub price_per_unit: i32,
    pub quantity: i32,
    pub hq: bool,
    pub timestamp: NaiveDateTime,
}

impl ActiveListing {
    /// Returns true if this listing belongs to a world that is in the excluded list
    pub fn is_excluded(&self, excluded_worlds: &[i32]) -> bool {
        excluded_worlds.contains(&self.world_id)
    }

    pub fn is_datacenter_excluded(
        &self,
        excluded_datacenters: &std::collections::HashSet<String>,
        world_helper: &crate::world_helper::WorldHelper,
    ) -> bool {
        if excluded_datacenters.is_empty() {
            return false;
        }
        world_helper
            .lookup_selector(crate::world_helper::AnySelector::World(self.world_id))
            .and_then(|r| r.as_world())
            .and_then(|w| {
                world_helper.lookup_selector(crate::world_helper::AnySelector::Datacenter(
                    w.datacenter_id,
                ))
            })
            .and_then(|r| r.as_datacenter())
            .map(|dc| excluded_datacenters.contains(&dc.name))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Datacenter, Region, World, WorldData};
    use crate::world_helper::WorldHelper;
    use std::collections::HashSet;

    fn sample_world_data() -> WorldData {
        // Region 1 "North-America" → DC 10 "Aether" → Worlds 100 "Adamantoise", 101 "Cactuar"
        // Region 1 "North-America" → DC 11 "Primal" → World 110 "Behemoth"
        // Region 2 "Japan" → DC 20 "Elemental" → World 200 "Aegis"
        WorldData {
            regions: vec![
                Region {
                    id: 1,
                    name: "North-America".into(),
                    datacenters: vec![
                        Datacenter {
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
                },
                Region {
                    id: 2,
                    name: "Japan".into(),
                    datacenters: vec![Datacenter {
                        id: 20,
                        name: "Elemental".into(),
                        region_id: 2,
                        worlds: vec![World {
                            id: 200,
                            name: "Aegis".into(),
                            datacenter_id: 20,
                        }],
                    }],
                },
            ],
        }
    }

    #[test]
    fn test_is_excluded_matching() {
        let listing = ActiveListing {
            id: 1,
            world_id: 100,
            item_id: 1,
            retainer_id: 1,
            price_per_unit: 100,
            quantity: 1,
            hq: false,
            timestamp: chrono::Utc::now().naive_utc(),
        };
        let excluded_worlds = vec![100, 101];
        assert!(listing.is_excluded(&excluded_worlds));
    }

    #[test]
    fn test_is_excluded_not_matching() {
        let listing = ActiveListing {
            id: 1,
            world_id: 102,
            item_id: 1,
            retainer_id: 1,
            price_per_unit: 100,
            quantity: 1,
            hq: false,
            timestamp: chrono::Utc::now().naive_utc(),
        };
        let excluded_worlds = vec![100, 101];
        assert!(!listing.is_excluded(&excluded_worlds));
    }

    #[test]
    fn test_is_datacenter_excluded_matching() {
        let helper: WorldHelper = sample_world_data().into();
        let listing = ActiveListing {
            id: 1,
            world_id: 100, // Aether
            item_id: 1,
            retainer_id: 1,
            price_per_unit: 100,
            quantity: 1,
            hq: false,
            timestamp: chrono::Utc::now().naive_utc(),
        };
        let mut excluded = HashSet::new();
        excluded.insert("Aether".to_string());
        assert!(listing.is_datacenter_excluded(&excluded, &helper));
    }

    #[test]
    fn test_is_datacenter_excluded_not_matching() {
        let helper: WorldHelper = sample_world_data().into();
        let listing = ActiveListing {
            id: 1,
            world_id: 110, // Primal
            item_id: 1,
            retainer_id: 1,
            price_per_unit: 100,
            quantity: 1,
            hq: false,
            timestamp: chrono::Utc::now().naive_utc(),
        };
        let mut excluded = HashSet::new();
        excluded.insert("Aether".to_string());
        assert!(!listing.is_datacenter_excluded(&excluded, &helper));
    }

    #[test]
    fn test_is_datacenter_excluded_unknown_world() {
        let helper: WorldHelper = sample_world_data().into();
        let listing = ActiveListing {
            id: 1,
            world_id: 999, // Unknown
            item_id: 1,
            retainer_id: 1,
            price_per_unit: 100,
            quantity: 1,
            hq: false,
            timestamp: chrono::Utc::now().naive_utc(),
        };
        let mut excluded = HashSet::new();
        excluded.insert("Aether".to_string());
        assert!(!listing.is_datacenter_excluded(&excluded, &helper));
    }
}
