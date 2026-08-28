//! Bulk sale-history statistics for `/api/v1/sale_stats/{worldDcOrRegion}`.
//!
//! One row per `(item_id, hq)` with sales inside the requested trailing
//! window, aggregated across every world in the selector's scope. The
//! recipe analyzer uses these as an alternative cost/revenue basis to the
//! single cheapest current listing.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSaleStats {
    pub item_id: i32,
    pub hq: bool,
    /// Lowest per-unit sale price in the window.
    pub min_price: i32,
    /// Exact median per-unit sale price in the window.
    pub median_price: i32,
    /// Arithmetic mean per-unit sale price in the window, rounded.
    pub avg_price: i32,
    /// Number of sales in the window backing the statistics above.
    pub num_sold: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BulkSaleStats {
    pub stats: Vec<ItemSaleStats>,
}
