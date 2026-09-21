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

/// Struct-of-arrays wire form of [`BulkListingStats`] — what
/// `/api/v1/listing_stats/{scope}?format=columnar` returns. Row `i` is
/// the `i`th element of every column; the names match
/// [`ItemListingStats`] field for field. The nested per-row
/// [`ListingWindowStats`] is not exploded: on windowed requests it rides
/// as one parallel column of objects, and on current-only requests the
/// key is absent. Rows are emitted in `(item_id, hq)` order for
/// compressibility; decoding does not depend on the order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct BulkListingStatsColumnar {
    pub item_id: Vec<i32>,
    pub hq: Vec<bool>,
    pub alive_count: Vec<u32>,
    pub alive_units: Vec<u64>,
    pub distinct_retainers: Vec<u32>,
    pub oldest_reviewed_unix: Vec<i64>,
    pub median_age_secs: Vec<u32>,
    pub floor_alive: Vec<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<Vec<ListingWindowStats>>,
}

impl From<BulkListingStats> for BulkListingStatsColumnar {
    /// The `window` column is present iff any row carries a window; rows
    /// without one contribute a `Default` placeholder so the column stays
    /// parallel. (The server fills every row on a windowed request.)
    fn from(value: BulkListingStats) -> Self {
        let n = value.stats.len();
        let windowed = value.stats.iter().any(|s| s.window.is_some());
        let mut out = Self {
            item_id: Vec::with_capacity(n),
            hq: Vec::with_capacity(n),
            alive_count: Vec::with_capacity(n),
            alive_units: Vec::with_capacity(n),
            distinct_retainers: Vec::with_capacity(n),
            oldest_reviewed_unix: Vec::with_capacity(n),
            median_age_secs: Vec::with_capacity(n),
            floor_alive: Vec::with_capacity(n),
            window: windowed.then(|| Vec::with_capacity(n)),
        };
        for row in value.stats {
            out.item_id.push(row.item_id);
            out.hq.push(row.hq);
            out.alive_count.push(row.alive_count);
            out.alive_units.push(row.alive_units);
            out.distinct_retainers.push(row.distinct_retainers);
            out.oldest_reviewed_unix.push(row.oldest_reviewed_unix);
            out.median_age_secs.push(row.median_age_secs);
            out.floor_alive.push(row.floor_alive);
            if let Some(window) = out.window.as_mut() {
                window.push(row.window.unwrap_or_default());
            }
        }
        out
    }
}

impl From<BulkListingStatsColumnar> for BulkListingStats {
    /// Zips the flat columns; unequal lengths truncate to the shortest
    /// rather than panicking. A `window` column shorter than the rows
    /// leaves the tail rows' `window` as `None`.
    fn from(value: BulkListingStatsColumnar) -> Self {
        let n = [
            value.item_id.len(),
            value.hq.len(),
            value.alive_count.len(),
            value.alive_units.len(),
            value.distinct_retainers.len(),
            value.oldest_reviewed_unix.len(),
            value.median_age_secs.len(),
            value.floor_alive.len(),
        ]
        .into_iter()
        .min()
        .unwrap_or(0);
        let mut windows = value.window.unwrap_or_default().into_iter();
        let stats = (0..n)
            .map(|i| ItemListingStats {
                item_id: value.item_id[i],
                hq: value.hq[i],
                alive_count: value.alive_count[i],
                alive_units: value.alive_units[i],
                distinct_retainers: value.distinct_retainers[i],
                oldest_reviewed_unix: value.oldest_reviewed_unix[i],
                median_age_secs: value.median_age_secs[i],
                floor_alive: value.floor_alive[i],
                window: windows.next(),
            })
            .collect();
        Self { stats }
    }
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

    fn listing(item_id: i32, hq: bool, floor_alive: i32) -> ItemListingStats {
        ItemListingStats {
            item_id,
            hq,
            alive_count: 3,
            alive_units: 12,
            distinct_retainers: 2,
            oldest_reviewed_unix: 1_700_000_000,
            median_age_secs: 86_400,
            floor_alive,
            window: None,
        }
    }

    fn window(days: u16) -> ListingWindowStats {
        ListingWindowStats {
            window_days: days,
            from: 1_700_000_000 - i64::from(days) * 86_400,
            to: 1_700_000_000,
            additions: 4,
            removals: 1,
            ..Default::default()
        }
    }

    #[test]
    fn columnar_round_trips_rows_in_order() {
        let rows = BulkListingStats {
            stats: vec![
                listing(2, false, 100),
                listing(2, true, 300),
                listing(5, false, 7),
            ],
        };
        let columnar = BulkListingStatsColumnar::from(rows.clone());
        assert_eq!(columnar.item_id, vec![2, 2, 5]);
        assert_eq!(columnar.hq, vec![false, true, false]);
        assert_eq!(columnar.floor_alive, vec![100, 300, 7]);
        assert!(columnar.window.is_none());
        assert_eq!(BulkListingStats::from(columnar), rows);
    }

    #[test]
    fn columnar_current_only_serializes_as_eight_arrays_without_window() {
        let columnar = BulkListingStatsColumnar::from(BulkListingStats {
            stats: vec![listing(5, true, 7)],
        });
        let json = serde_json::to_string(&columnar).unwrap();
        assert_eq!(
            json,
            r#"{"item_id":[5],"hq":[true],"alive_count":[3],"alive_units":[12],"distinct_retainers":[2],"oldest_reviewed_unix":[1700000000],"median_age_secs":[86400],"floor_alive":[7]}"#
        );
        let back: BulkListingStatsColumnar = serde_json::from_str(&json).unwrap();
        assert_eq!(back, columnar);
    }

    #[test]
    fn columnar_windowed_round_trips_window_column() {
        let mut a = listing(2, false, 100);
        a.window = Some(window(7));
        let mut b = listing(5, false, 7);
        b.window = Some(window(30));
        let rows = BulkListingStats { stats: vec![a, b] };
        let columnar = BulkListingStatsColumnar::from(rows.clone());
        assert_eq!(columnar.window.as_ref().map(|w| w.len()), Some(2));
        let json = serde_json::to_string(&columnar).unwrap();
        assert!(json.contains(r#""window":[{"#));
        let back: BulkListingStatsColumnar = serde_json::from_str(&json).unwrap();
        assert_eq!(BulkListingStats::from(back), rows);
    }

    #[test]
    fn columnar_short_window_column_leaves_tail_rows_without_window() {
        let mut columnar = BulkListingStatsColumnar::from(BulkListingStats {
            stats: vec![listing(1, false, 10), listing(2, false, 20)],
        });
        columnar.window = Some(vec![window(7)]);
        let rows = BulkListingStats::from(columnar);
        assert_eq!(rows.stats[0].window, Some(window(7)));
        assert_eq!(rows.stats[1].window, None);
    }

    #[test]
    fn columnar_empty_round_trips() {
        assert_eq!(
            BulkListingStats::from(BulkListingStatsColumnar::default()).stats,
            vec![]
        );
        assert_eq!(
            BulkListingStatsColumnar::from(BulkListingStats::default()),
            BulkListingStatsColumnar::default()
        );
    }

    #[test]
    fn columnar_mismatched_lengths_truncate_to_shortest() {
        let mut columnar = BulkListingStatsColumnar::from(BulkListingStats {
            stats: vec![
                listing(1, false, 10),
                listing(2, true, 20),
                listing(3, false, 30),
            ],
        });
        columnar.median_age_secs.pop();
        let rows = BulkListingStats::from(columnar);
        assert_eq!(
            rows.stats,
            vec![listing(1, false, 10), listing(2, true, 20)]
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
