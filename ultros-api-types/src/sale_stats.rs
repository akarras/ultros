//! Bulk sale-history statistics for `/api/v1/sale_stats/{worldDcOrRegion}`.
//!
//! One row per `(item_id, hq)` with sales inside the requested trailing
//! window, aggregated across every world in the selector's scope. The
//! recipe analyzer uses these as an alternative cost/revenue basis to the
//! single cheapest current listing.

use serde::{Deserialize, Serialize};

use crate::trends::ConfidenceBand;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct ItemSaleStats {
    pub item_id: i32,
    pub hq: bool,
    /// Lowest per-unit sale price in the window.
    pub min_price: i32,
    /// Approximate median per-unit sale price in the window, merged from
    /// per-world t-digest states.
    pub median_price: i32,
    /// Arithmetic mean per-unit sale price in the window, rounded.
    pub avg_price: i32,
    /// Number of sales in the window backing the statistics above.
    pub num_sold: i64,
    // Everything below is serde-defaulted: added for the recipe analyzer's
    // stats-backed columns, and absent from older servers' payloads.
    //
    /// Unix seconds of the newest sale in the window. 0 = unknown (old
    /// server).
    #[serde(default)]
    pub last_sold_unix: i64,
    /// Units traded in the window (sum of quantities).
    #[serde(default)]
    pub units_sold: u64,
    /// Volume-weighted average per-unit price over the window, rounded.
    /// 0 = unknown.
    #[serde(default)]
    pub vwap: i32,
    /// `num_sold / window_days`, precomputed server-side.
    #[serde(default)]
    pub sales_per_day: f32,
    /// Per-world confidence band; `Unknown` for multi-world scopes or old
    /// servers (the band is a stored per-world judgement and doesn't
    /// compose across worlds).
    #[serde(default)]
    pub confidence: ConfidenceBand,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct BulkSaleStats {
    pub stats: Vec<ItemSaleStats>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_wire_shape_still_deserializes() {
        // Payload shape served before the stats-column widening — every
        // added field must default rather than fail deserialization.
        let old = r#"{"item_id":1,"hq":false,"min_price":10,"median_price":20,"avg_price":21,"num_sold":5}"#;
        let row: ItemSaleStats = serde_json::from_str(old).unwrap();
        assert_eq!(row.num_sold, 5);
        assert_eq!(row.last_sold_unix, 0);
        assert_eq!(row.units_sold, 0);
        assert_eq!(row.vwap, 0);
        assert_eq!(row.sales_per_day, 0.0);
        assert_eq!(row.confidence, ConfidenceBand::Unknown);
    }
}
