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
