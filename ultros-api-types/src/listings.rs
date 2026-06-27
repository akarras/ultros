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
