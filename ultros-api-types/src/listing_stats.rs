//! The alive listing set for `/api/v1/listing_stats/{worldDcOrRegion}`.
//!
//! One row per `(item_id, hq)` with at least one listing on the board,
//! replayed from ClickHouse `listing_events` and aggregated across every
//! world in the selector's scope.
//! Current fields still describe the board now; explicit window requests add
//! historical observations without changing the current-only response.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
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
    /// Absent on current-only requests and older servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<ListingWindowStats>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
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
        assert!(row.window.is_none());
        assert!(!serde_json::to_string(&row).unwrap().contains("window"));
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
                window: None,
            }],
        };
        let json = serde_json::to_string(&bulk).unwrap();
        assert_eq!(
            serde_json::from_str::<BulkListingStats>(&json).unwrap(),
            bulk
        );
    }

    #[test]
    fn window_without_undercut_fields_still_deserializes() {
        // A generation stored before the deploy, or an older server. Every
        // other field is present because the reducer always writes them.
        let old = r#"{"window_days":7,"from":0,"to":604800,"additions":1,"removals":2,
            "listing_coverage":{"first_observed_unix":null,"last_observed_unix":null,"observed_span_secs":0,"continuity_verified":false},
            "floor_min":null,"floor_max":null,"floor_known_secs":0,"floor_empty_secs":0,"floor_unknown_secs":604800,
            "matches":{"matched":0,"ambiguous":0,"repriced":0,"unmatched":0,"sales_without_receipt":0,
              "receipt_coverage":{"first_observed_unix":null,"last_observed_unix":null,"observed_span_secs":0,"continuity_verified":false},
              "received_sales":0,"settled_through_unix":0,"pending":0,"median_time_to_sell_secs":null,"age_origin":"last_review_time"},
            "stock_status":"unavailable","days_of_stock":null}"#;
        let window: ListingWindowStats = serde_json::from_str(old).unwrap();
        assert_eq!(window.undercuts, 0);
        assert_eq!(window.undercuts_per_day, None);
        assert_eq!(window.undercut_median, None);
    }

    #[test]
    fn undercut_fields_round_trip() {
        let window = ListingWindowStats {
            window_days: 7,
            undercuts: 3,
            undercuts_per_day: Some(0.5),
            undercut_median: Some(0.026),
            ..Default::default()
        };
        let json = serde_json::to_string(&window).unwrap();
        assert!(json.contains("\"undercuts\":3"));
        assert_eq!(
            serde_json::from_str::<ListingWindowStats>(&json).unwrap(),
            window
        );
    }
}

/// Observed bounds are evidence, not a guarantee of uninterrupted ingestion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryCoverage {
    pub first_observed_unix: Option<i64>,
    pub last_observed_unix: Option<i64>,
    /// Seconds between the first and last retained observations inside the
    /// requested window. Quiet intervals and ingestion gaps are indistinguishable.
    pub observed_span_secs: u64,
    pub continuity_verified: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockStatus {
    #[default]
    Unavailable,
    NoSales,
    Estimated,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ListingWindowStats {
    pub window_days: u16,
    pub from: i64,
    pub to: i64,
    pub additions: u64,
    pub removals: u64,
    pub listing_coverage: HistoryCoverage,
    pub floor_min: Option<u32>,
    pub floor_max: Option<u32>,
    pub floor_known_secs: u64,
    pub floor_empty_secs: u64,
    pub floor_unknown_secs: u64,
    pub matches: MatchedSalesStats,
    pub stock_status: StockStatus,
    pub days_of_stock: Option<f64>,
    /// Same-listing price drops observed in the window: an `updated` row
    /// whose price fell, or a removed-then-added pair on one listing id
    /// within 600 s where the add is cheaper. Raises, same-price pairs,
    /// the seed and id-less legacy listings are excluded.
    #[serde(default)]
    pub undercuts: u64,
    /// `undercuts` per day of scope-wide listing coverage (clamped to at
    /// least one day and at most the window). `None` only when the scope
    /// observed no listing events at all in the window.
    #[serde(default)]
    pub undercuts_per_day: Option<f64>,
    /// Median relative drop across those undercuts, in `0..1`. `None` when
    /// `undercuts == 0`.
    #[serde(default)]
    pub undercut_median: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListingAgeOrigin {
    #[default]
    LastReviewTime,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchedSalesStats {
    /// Conservative one-to-one matches, never every removal.
    pub matched: u64,
    pub ambiguous: u64,
    pub repriced: u64,
    pub unmatched: u64,
    /// Raw sales in the game-time window without a durable websocket receipt.
    pub sales_without_receipt: u64,
    pub receipt_coverage: HistoryCoverage,
    /// Distinct durable receipts observed in this window, before matching filters.
    pub received_sales: u64,
    /// Removals after this instant have not had complete +/-300s context.
    pub settled_through_unix: i64,
    pub pending: u64,
    pub median_time_to_sell_secs: Option<u64>,
    pub age_origin: ListingAgeOrigin,
}
