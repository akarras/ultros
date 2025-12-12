use serde::{Deserialize, Deserializer, Serialize, de};
use std::collections::HashMap;
use std::fmt;

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

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct CheapestListingMapKey {
    pub item_id: i32,
    pub hq: bool,
}

impl Serialize for CheapestListingMapKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{}_{}", self.item_id, self.hq))
    }
}

impl<'de> Deserialize<'de> for CheapestListingMapKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct KeyVisitor;

        impl<'de> de::Visitor<'de> for KeyVisitor {
            type Value = CheapestListingMapKey;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string in the format 'item_id_hq'")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let parts: Vec<&str> = value.splitn(2, '_').collect();
                if parts.len() != 2 {
                    return Err(E::custom(format!(
                        "Invalid format: expected 'item_id_hq', got '{}'",
                        value
                    )));
                }

                let item_id_str = parts[0];
                let hq_str = parts[1];

                let item_id: i32 = item_id_str.parse::<i32>().map_err(|e| {
                    E::custom(format!(
                        "Failed to parse item_id: '{}', error: {}",
                        item_id_str, e
                    ))
                })?;
                let hq: bool = hq_str.parse::<bool>().map_err(|e| {
                    E::custom(format!("Failed to parse hq: '{}', error: {}", hq_str, e))
                })?;

                Ok(CheapestListingMapKey { item_id, hq })
            }
        }

        deserializer.deserialize_str(KeyVisitor)
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, PartialOrd)]
pub struct CheapestListingData {
    pub price: i32,
    pub world_id: i32,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct CheapestListingsMap {
    pub map: HashMap<CheapestListingMapKey, CheapestListingData>,
}

pub struct PriceSummary {
    pub lq: Option<CheapestListingData>,
    pub hq: Option<CheapestListingData>,
}

impl PriceSummary {
    pub fn lowest_gil(&self) -> Option<i32> {
        Some(match (self.lq, self.hq) {
            (None, None) => return None,
            (None, Some(hq)) => hq.price,
            (Some(lq), None) => lq.price,
            (Some(lq), Some(hq)) => lq.price.min(hq.price),
        })
    }

    pub fn price_preferring_hq(&self) -> Option<i32> {
        match (self.lq, self.hq) {
            (_, Some(hq)) => Some(hq.price),
            (Some(lq), _) => Some(lq.price),
            (_, _) => None,
        }
    }
}

impl CheapestListingsMap {
    pub fn find_matching_listings(&self, item_id: i32) -> PriceSummary {
        let hq = self
            .map
            .get(&CheapestListingMapKey { hq: true, item_id })
            .copied();
        let lq = self
            .map
            .get(&CheapestListingMapKey { hq: false, item_id })
            .copied();
        PriceSummary { lq, hq }
    }
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
