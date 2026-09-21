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
    /// Total gil traded in the window (sum of price × quantity). 0 = unknown.
    #[serde(default)]
    pub gil_volume: u64,
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

/// Struct-of-arrays wire form of [`BulkSaleStats`] — what
/// `/api/v1/sale_stats/{scope}?format=columnar` returns. Row `i` is the
/// `i`th element of every column; the names match [`ItemSaleStats`]
/// field for field. Rows are emitted in `(item_id, hq)` order for
/// compressibility; decoding does not depend on the order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct BulkSaleStatsColumnar {
    pub item_id: Vec<i32>,
    pub hq: Vec<bool>,
    pub min_price: Vec<i32>,
    pub median_price: Vec<i32>,
    pub avg_price: Vec<i32>,
    pub num_sold: Vec<i64>,
    pub last_sold_unix: Vec<i64>,
    pub units_sold: Vec<u64>,
    pub vwap: Vec<i32>,
    pub gil_volume: Vec<u64>,
    pub sales_per_day: Vec<f32>,
    pub confidence: Vec<ConfidenceBand>,
}

impl From<BulkSaleStats> for BulkSaleStatsColumnar {
    fn from(value: BulkSaleStats) -> Self {
        let n = value.stats.len();
        let mut out = Self {
            item_id: Vec::with_capacity(n),
            hq: Vec::with_capacity(n),
            min_price: Vec::with_capacity(n),
            median_price: Vec::with_capacity(n),
            avg_price: Vec::with_capacity(n),
            num_sold: Vec::with_capacity(n),
            last_sold_unix: Vec::with_capacity(n),
            units_sold: Vec::with_capacity(n),
            vwap: Vec::with_capacity(n),
            gil_volume: Vec::with_capacity(n),
            sales_per_day: Vec::with_capacity(n),
            confidence: Vec::with_capacity(n),
        };
        for row in value.stats {
            out.item_id.push(row.item_id);
            out.hq.push(row.hq);
            out.min_price.push(row.min_price);
            out.median_price.push(row.median_price);
            out.avg_price.push(row.avg_price);
            out.num_sold.push(row.num_sold);
            out.last_sold_unix.push(row.last_sold_unix);
            out.units_sold.push(row.units_sold);
            out.vwap.push(row.vwap);
            out.gil_volume.push(row.gil_volume);
            out.sales_per_day.push(row.sales_per_day);
            out.confidence.push(row.confidence);
        }
        out
    }
}

impl From<BulkSaleStatsColumnar> for BulkSaleStats {
    /// Zips the columns. A malformed payload with unequal column lengths
    /// degrades to the rows every column has rather than panicking.
    fn from(value: BulkSaleStatsColumnar) -> Self {
        let n = [
            value.item_id.len(),
            value.hq.len(),
            value.min_price.len(),
            value.median_price.len(),
            value.avg_price.len(),
            value.num_sold.len(),
            value.last_sold_unix.len(),
            value.units_sold.len(),
            value.vwap.len(),
            value.gil_volume.len(),
            value.sales_per_day.len(),
            value.confidence.len(),
        ]
        .into_iter()
        .min()
        .unwrap_or(0);
        let stats = (0..n)
            .map(|i| ItemSaleStats {
                item_id: value.item_id[i],
                hq: value.hq[i],
                min_price: value.min_price[i],
                median_price: value.median_price[i],
                avg_price: value.avg_price[i],
                num_sold: value.num_sold[i],
                last_sold_unix: value.last_sold_unix[i],
                units_sold: value.units_sold[i],
                vwap: value.vwap[i],
                gil_volume: value.gil_volume[i],
                sales_per_day: value.sales_per_day[i],
                confidence: value.confidence[i],
            })
            .collect();
        Self { stats }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(item_id: i32, hq: bool, median_price: i32) -> ItemSaleStats {
        ItemSaleStats {
            item_id,
            hq,
            min_price: median_price - 1,
            median_price,
            avg_price: median_price + 1,
            num_sold: 14,
            last_sold_unix: 1_789_915_494,
            units_sold: 20,
            vwap: median_price + 2,
            gil_volume: 6_738,
            sales_per_day: 2.0,
            confidence: ConfidenceBand::High,
        }
    }

    #[test]
    fn columnar_round_trips_rows_in_order() {
        let rows = BulkSaleStats {
            stats: vec![stat(2, false, 100), stat(2, true, 300), stat(5, false, 7)],
        };
        let columnar = BulkSaleStatsColumnar::from(rows.clone());
        assert_eq!(columnar.item_id, vec![2, 2, 5]);
        assert_eq!(columnar.hq, vec![false, true, false]);
        assert_eq!(columnar.median_price, vec![100, 300, 7]);
        assert_eq!(columnar.confidence, vec![ConfidenceBand::High; 3]);
        assert_eq!(BulkSaleStats::from(columnar), rows);
    }

    #[test]
    fn columnar_serializes_as_twelve_arrays() {
        let columnar = BulkSaleStatsColumnar::from(BulkSaleStats {
            stats: vec![stat(5, true, 7)],
        });
        let json = serde_json::to_string(&columnar).unwrap();
        assert_eq!(
            json,
            r#"{"item_id":[5],"hq":[true],"min_price":[6],"median_price":[7],"avg_price":[8],"num_sold":[14],"last_sold_unix":[1789915494],"units_sold":[20],"vwap":[9],"gil_volume":[6738],"sales_per_day":[2.0],"confidence":["high"]}"#
        );
        let back: BulkSaleStatsColumnar = serde_json::from_str(&json).unwrap();
        assert_eq!(back, columnar);
    }

    #[test]
    fn columnar_empty_round_trips() {
        assert_eq!(
            BulkSaleStats::from(BulkSaleStatsColumnar::default()).stats,
            vec![]
        );
        assert_eq!(
            BulkSaleStatsColumnar::from(BulkSaleStats::default()),
            BulkSaleStatsColumnar::default()
        );
    }

    #[test]
    fn columnar_mismatched_lengths_truncate_to_shortest() {
        let mut columnar = BulkSaleStatsColumnar::from(BulkSaleStats {
            stats: vec![stat(1, false, 10), stat(2, true, 20), stat(3, false, 30)],
        });
        columnar.vwap.pop();
        let rows = BulkSaleStats::from(columnar);
        assert_eq!(rows.stats, vec![stat(1, false, 10), stat(2, true, 20)]);
    }

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
        assert_eq!(row.gil_volume, 0);
        assert_eq!(row.sales_per_day, 0.0);
        assert_eq!(row.confidence, ConfidenceBand::Unknown);
    }
}
