use serde::{Deserialize, Serialize};

/// "item_id":6605,"hq":false,"cheapest_price":6999999,"world_id":99
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheapestListingItem {
    pub item_id: i32,
    pub hq: bool,
    pub cheapest_price: i32,
    pub world_id: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheapestListings {
    pub cheapest_listings: Vec<CheapestListingItem>,
}
