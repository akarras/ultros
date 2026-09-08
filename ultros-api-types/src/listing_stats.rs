//! The alive listing set for `/api/v1/listing_stats/{worldDcOrRegion}`.
//!
//! One row per `(item_id, hq)` with at least one listing on the board,
//! replayed from ClickHouse `listing_events` and aggregated across every
//! world in the selector's scope. Part I1 of #1342: there is no time window
//! yet, so every field describes the board as it is now.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ItemListingStats {
    pub item_id: i32,
    pub hq: bool,
    // Everything below is serde-defaulted so a client built against a later
    // wire shape still reads an older server's payload.
    //
    /// Listings whose last observed event is not a removal.
    #[serde(default)]
    pub alive_count: u32,
    /// Units across those listings (sum of quantities).
    #[serde(default)]
    pub alive_units: u64,
    /// Distinct retainers holding them.
    #[serde(default)]
    pub distinct_retainers: u32,
    /// Unix seconds when the least recently touched alive listing was last
    /// reviewed by its retainer — the honest "listed since". 0 = unknown.
    #[serde(default)]
    pub oldest_reviewed_unix: i64,
    /// Median seconds since an alive listing was last touched.
    #[serde(default)]
    pub median_age_secs: u32,
    /// Lowest per-unit price among alive listings. 0 = unknown.
    #[serde(default)]
    pub floor_alive: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BulkListingStats {
    pub stats: Vec<ItemListingStats>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_wire_shape_still_deserializes() {
        // The minimal shape: only the key. Every stat field must default
        // rather than fail deserialization, so I2's additions (and a client
        // ahead of its server) never break the read.
        let old = r#"{"item_id":1,"hq":true}"#;
        let row: ItemListingStats = serde_json::from_str(old).unwrap();
        assert_eq!(row.item_id, 1);
        assert!(row.hq);
        assert_eq!(row.alive_count, 0);
        assert_eq!(row.alive_units, 0);
        assert_eq!(row.distinct_retainers, 0);
        assert_eq!(row.oldest_reviewed_unix, 0);
        assert_eq!(row.median_age_secs, 0);
        assert_eq!(row.floor_alive, 0);
    }

    #[test]
    fn bulk_payload_round_trips() {
        let bulk = BulkListingStats {
            stats: vec![ItemListingStats {
                item_id: 7,
                hq: false,
                alive_count: 3,
                alive_units: 12,
                distinct_retainers: 2,
                oldest_reviewed_unix: 1_700_000_000,
                median_age_secs: 86_400,
                floor_alive: 950,
            }],
        };
        let json = serde_json::to_string(&bulk).unwrap();
        assert_eq!(
            serde_json::from_str::<BulkListingStats>(&json).unwrap(),
            bulk
        );
    }
}
