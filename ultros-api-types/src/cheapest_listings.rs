use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// "item_id":6605,"hq":false,"cheapest_price":6999999,"world_id":99
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheapestListingItem {
    pub item_id: i32,
    pub hq: bool,
    pub cheapest_price: i32,
    pub world_id: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CheapestListings {
    pub cheapest_listings: Vec<CheapestListingItem>,
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Clone, Copy)]
pub struct CheapestListingMapKey {
    pub item_id: i32,
    pub hq: bool,
}

#[derive(Deserialize, Serialize, Clone, Copy)]
pub struct CheapestListingData {
    pub price: i32,
    pub world_id: i32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CheapestListingsMap {
    pub map: HashMap<CheapestListingMapKey, CheapestListingData>,
}

impl From<CheapestListings> for CheapestListingsMap {
    fn from(value: CheapestListings) -> Self {
        Self {
            map: value
                .cheapest_listings
                .into_iter()
                .map(
                    |CheapestListingItem {
                         item_id,
                         hq,
                         cheapest_price,
                         world_id,
                     }| {
                        (
                            CheapestListingMapKey { item_id, hq },
                            CheapestListingData {
                                price: cheapest_price,
                                world_id,
                            },
                        )
                    },
                )
                .collect(),
        }
    }
}
