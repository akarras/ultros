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

#[cfg(test)]
mod tests {
    use super::*;

    fn data(price: i32, world_id: i32) -> CheapestListingData {
        CheapestListingData { price, world_id }
    }

    #[test]
    fn map_key_serializes_to_id_hq_string() {
        let key = CheapestListingMapKey {
            item_id: 42,
            hq: true,
        };
        let s = serde_json::to_string(&key).unwrap();
        assert_eq!(s, "\"42_true\"");
    }

    #[test]
    fn map_key_deserializes_from_id_hq_string() {
        let key: CheapestListingMapKey = serde_json::from_str("\"123_false\"").unwrap();
        assert_eq!(
            key,
            CheapestListingMapKey {
                item_id: 123,
                hq: false
            }
        );
    }

    #[test]
    fn map_key_roundtrip_through_json() {
        for (item_id, hq) in [(1, true), (-7, false), (i32::MAX, true), (0, false)] {
            let key = CheapestListingMapKey { item_id, hq };
            let s = serde_json::to_string(&key).unwrap();
            let back: CheapestListingMapKey = serde_json::from_str(&s).unwrap();
            assert_eq!(key, back);
        }
    }

    #[test]
    fn map_key_rejects_missing_separator() {
        let r: Result<CheapestListingMapKey, _> = serde_json::from_str("\"123true\"");
        assert!(r.is_err(), "should reject when separator absent");
    }

    #[test]
    fn map_key_rejects_invalid_item_id() {
        let r: Result<CheapestListingMapKey, _> = serde_json::from_str("\"abc_true\"");
        assert!(r.is_err());
    }

    #[test]
    fn map_key_rejects_invalid_hq() {
        let r: Result<CheapestListingMapKey, _> = serde_json::from_str("\"123_maybe\"");
        assert!(r.is_err());
    }

    #[test]
    fn price_summary_lowest_gil_none_when_both_missing() {
        let summary = PriceSummary { lq: None, hq: None };
        assert_eq!(summary.lowest_gil(), None);
    }

    #[test]
    fn price_summary_lowest_gil_picks_minimum_when_both_present() {
        let summary = PriceSummary {
            lq: Some(data(100, 1)),
            hq: Some(data(80, 1)),
        };
        assert_eq!(summary.lowest_gil(), Some(80));

        let summary = PriceSummary {
            lq: Some(data(100, 1)),
            hq: Some(data(120, 1)),
        };
        assert_eq!(summary.lowest_gil(), Some(100));
    }

    #[test]
    fn price_summary_lowest_gil_uses_only_present_side() {
        let summary = PriceSummary {
            lq: Some(data(50, 1)),
            hq: None,
        };
        assert_eq!(summary.lowest_gil(), Some(50));

        let summary = PriceSummary {
            lq: None,
            hq: Some(data(75, 1)),
        };
        assert_eq!(summary.lowest_gil(), Some(75));
    }

    #[test]
    fn price_summary_preferring_hq_prefers_hq_even_when_more_expensive() {
        let summary = PriceSummary {
            lq: Some(data(50, 1)),
            hq: Some(data(200, 1)),
        };
        assert_eq!(summary.price_preferring_hq(), Some(200));
    }

    #[test]
    fn price_summary_preferring_hq_falls_back_to_lq_when_no_hq() {
        let summary = PriceSummary {
            lq: Some(data(50, 1)),
            hq: None,
        };
        assert_eq!(summary.price_preferring_hq(), Some(50));
    }

    #[test]
    fn price_summary_preferring_hq_none_when_both_missing() {
        let summary = PriceSummary { lq: None, hq: None };
        assert_eq!(summary.price_preferring_hq(), None);
    }

    #[test]
    fn from_cheapest_listings_builds_map_indexed_by_item_id_and_hq() {
        let listings = CheapestListings {
            cheapest_listings: vec![
                CheapestListingItem {
                    item_id: 1,
                    hq: false,
                    cheapest_price: 100,
                    world_id: 7,
                },
                CheapestListingItem {
                    item_id: 1,
                    hq: true,
                    cheapest_price: 250,
                    world_id: 9,
                },
                CheapestListingItem {
                    item_id: 2,
                    hq: false,
                    cheapest_price: 1,
                    world_id: 3,
                },
            ],
        };
        let map: CheapestListingsMap = listings.into();
        assert_eq!(map.map.len(), 3);
        let lq = map
            .map
            .get(&CheapestListingMapKey {
                item_id: 1,
                hq: false,
            })
            .unwrap();
        assert_eq!(lq.price, 100);
        assert_eq!(lq.world_id, 7);
        let hq = map
            .map
            .get(&CheapestListingMapKey {
                item_id: 1,
                hq: true,
            })
            .unwrap();
        assert_eq!(hq.price, 250);
        assert_eq!(hq.world_id, 9);
    }

    #[test]
    fn find_matching_listings_returns_lq_and_hq() {
        let listings = CheapestListings {
            cheapest_listings: vec![
                CheapestListingItem {
                    item_id: 5,
                    hq: false,
                    cheapest_price: 1000,
                    world_id: 1,
                },
                CheapestListingItem {
                    item_id: 5,
                    hq: true,
                    cheapest_price: 2000,
                    world_id: 1,
                },
            ],
        };
        let map: CheapestListingsMap = listings.into();
        let summary = map.find_matching_listings(5);
        assert_eq!(summary.lq.map(|d| d.price), Some(1000));
        assert_eq!(summary.hq.map(|d| d.price), Some(2000));
    }

    #[test]
    fn find_matching_listings_returns_none_when_item_missing() {
        let map: CheapestListingsMap = CheapestListings {
            cheapest_listings: vec![],
        }
        .into();
        let summary = map.find_matching_listings(999);
        assert!(summary.lq.is_none() && summary.hq.is_none());
    }
}
