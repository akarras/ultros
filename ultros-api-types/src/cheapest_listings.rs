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

/// Struct-of-arrays wire form of [`CheapestListings`] — what
/// `/api/v1/cheapest/{world}?format=columnar` returns. Row `i` is
/// `(item_id[i], hq[i], price[i], world_id[i])`. The server emits rows in
/// `(item_id, hq)` order, which is what makes the columns compress well
/// (~83 KB brotli vs ~150 KB for the row-of-objects shape on a 24k-row
/// region), but decoding does not depend on the order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CheapestListingsColumnar {
    pub item_id: Vec<i32>,
    pub hq: Vec<bool>,
    pub price: Vec<i32>,
    pub world_id: Vec<i32>,
}

impl From<CheapestListings> for CheapestListingsColumnar {
    fn from(value: CheapestListings) -> Self {
        let n = value.cheapest_listings.len();
        let mut out = Self {
            item_id: Vec::with_capacity(n),
            hq: Vec::with_capacity(n),
            price: Vec::with_capacity(n),
            world_id: Vec::with_capacity(n),
        };
        for row in value.cheapest_listings {
            out.item_id.push(row.item_id);
            out.hq.push(row.hq);
            out.price.push(row.cheapest_price);
            out.world_id.push(row.world_id);
        }
        out
    }
}

impl From<CheapestListingsColumnar> for CheapestListings {
    /// Zips the four columns. A malformed payload with unequal column
    /// lengths degrades to the rows every column has rather than panicking.
    fn from(value: CheapestListingsColumnar) -> Self {
        let cheapest_listings = value
            .item_id
            .into_iter()
            .zip(value.hq)
            .zip(value.price)
            .zip(value.world_id)
            .map(
                |(((item_id, hq), cheapest_price), world_id)| CheapestListingItem {
                    item_id,
                    hq,
                    cheapest_price,
                    world_id,
                },
            )
            .collect();
        Self { cheapest_listings }
    }
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

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, PartialOrd)]
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

    /// The listing `lowest_gil` (prefer_hq = false) or `price_preferring_hq`
    /// (prefer_hq = true) would price from, entry and all, so a caller can
    /// keep its world. LQ wins an equal-price tie under lowest; HQ under
    /// prefer.
    pub fn chosen(&self, prefer_hq: bool) -> Option<CheapestListingData> {
        match (self.lq, self.hq) {
            (None, None) => None,
            (None, Some(hq)) => Some(hq),
            (Some(lq), None) => Some(lq),
            (Some(lq), Some(hq)) => Some(if prefer_hq || hq.price < lq.price {
                hq
            } else {
                lq
            }),
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

    /// `chosen` replays `lowest_gil` / `price_preferring_hq` but keeps the
    /// entry: LQ wins an equal-price tie under lowest, HQ under prefer.
    #[test]
    fn chosen_matches_lowest_gil_and_prefer_hq_with_tie_rule() {
        let prices: [Option<i32>; 5] = [None, Some(1), Some(2), Some(3), Some(4)];
        for lq in prices {
            for hq in prices {
                let s = PriceSummary {
                    lq: lq.map(|p| data(p, 11)),
                    hq: hq.map(|p| data(p, 22)),
                };
                assert_eq!(
                    s.chosen(false).map(|c| c.price),
                    s.lowest_gil(),
                    "{lq:?} {hq:?}"
                );
                assert_eq!(
                    s.chosen(true).map(|c| c.price),
                    s.price_preferring_hq(),
                    "{lq:?} {hq:?}"
                );
                if lq.is_some() && lq == hq {
                    assert_eq!(
                        s.chosen(false).unwrap().world_id,
                        11,
                        "lowest: LQ wins a tie"
                    );
                    assert_eq!(
                        s.chosen(true).unwrap().world_id,
                        22,
                        "prefer: HQ wins a tie"
                    );
                }
            }
        }
        let s = PriceSummary { lq: None, hq: None };
        assert!(s.chosen(false).is_none());
        assert!(s.chosen(true).is_none());
    }

    fn item(item_id: i32, hq: bool, cheapest_price: i32, world_id: i32) -> CheapestListingItem {
        CheapestListingItem {
            item_id,
            hq,
            cheapest_price,
            world_id,
        }
    }

    #[test]
    fn columnar_round_trips_rows_in_order() {
        let rows = CheapestListings {
            cheapest_listings: vec![
                item(2, false, 12, 73),
                item(2, true, 300, 74),
                item(5, false, 7, 34),
            ],
        };
        let columnar = CheapestListingsColumnar::from(rows.clone());
        assert_eq!(columnar.item_id, vec![2, 2, 5]);
        assert_eq!(columnar.hq, vec![false, true, false]);
        assert_eq!(columnar.price, vec![12, 300, 7]);
        assert_eq!(columnar.world_id, vec![73, 74, 34]);
        assert_eq!(CheapestListings::from(columnar), rows);
    }

    #[test]
    fn columnar_serializes_as_four_arrays() {
        let columnar = CheapestListingsColumnar::from(CheapestListings {
            cheapest_listings: vec![item(2, true, 300, 74)],
        });
        let json = serde_json::to_string(&columnar).unwrap();
        assert_eq!(
            json,
            r#"{"item_id":[2],"hq":[true],"price":[300],"world_id":[74]}"#
        );
        let back: CheapestListingsColumnar = serde_json::from_str(&json).unwrap();
        assert_eq!(back, columnar);
    }

    #[test]
    fn columnar_empty_round_trips() {
        let empty = CheapestListingsColumnar::default();
        assert_eq!(CheapestListings::from(empty).cheapest_listings, vec![]);
        assert_eq!(
            CheapestListingsColumnar::from(CheapestListings::default()),
            CheapestListingsColumnar::default()
        );
    }

    #[test]
    fn columnar_mismatched_lengths_truncate_to_shortest() {
        let columnar = CheapestListingsColumnar {
            item_id: vec![1, 2, 3],
            hq: vec![false, true],
            price: vec![10, 20, 30],
            world_id: vec![7, 8, 9],
        };
        let rows = CheapestListings::from(columnar);
        assert_eq!(
            rows.cheapest_listings,
            vec![item(1, false, 10, 7), item(2, true, 20, 8)]
        );
    }
}
