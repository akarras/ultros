//! Batch resale-quality wire types.
//!
//! Feeds the Flip Finder enrichment pass: the FE sends a list of
//! `(item_id, hq)` tuples for the current world, and gets back a row per
//! tuple with the ClickHouse rollup's confidence band, 30d VWAP, sample
//! size, laundering suspicion, and sales/day. The FE uses these to
//! render the `Quality` column, the VWAP column, and the Sales/day
//! column, and to drive the quality filter.
//!
//! Rows the rollup has no data for are simply absent from the response —
//! the FE renders those as Pass-1 / `Unknown` confidence (no chip).

use serde::{Deserialize, Serialize};

use crate::trends::ConfidenceBand;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResaleQualityRow {
    pub item_id: i32,
    pub hq: bool,
    pub world_id: i32,
    /// Echo of the queried window (typically 30).
    pub window_days: u16,
    pub vwap: i32,
    pub sample_size: u32,
    /// Window sample size / window length. Honest velocity metric for
    /// the Flip Finder Sales/day column.
    pub sales_per_day: f32,
    pub confidence_band: ConfidenceBand,
    /// 0.0-1.0; the FE uses > 0.7 as the "suspicious" cutoff for its
    /// own defense-in-depth drop pass when Show suspicious is off.
    pub launder_suspicion: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResaleQualityResponse {
    pub world_id: i32,
    pub window_days: u16,
    pub rows: Vec<ResaleQualityRow>,
}

/// POST body for `/api/v1/resale_quality/{world}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResaleQualityRequest {
    /// Each tuple is `(item_id, hq)`. World comes from the URL.
    pub items: Vec<(i32, bool)>,
    /// Window in days; clamped to {7, 30, 90} server-side. Default 30.
    #[serde(default)]
    pub window_days: Option<u16>,
}
