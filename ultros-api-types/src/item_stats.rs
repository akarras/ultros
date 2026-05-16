//! Per-item analyzer stats for the item view's confidence chip.
//!
//! The server fills this from a ClickHouse `deep_scan_batch` query; see
//! [`ultros_clickhouse::queries::DeepScan`] for the source-side type.
//! We keep a separate wire type here so api-types doesn't depend on the
//! ClickHouse driver.

use serde::{Deserialize, Serialize};

use crate::trends::ConfidenceBand;

/// One variant (HQ or NQ) of an item's 30-day rolled-up stats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemStatsVariant {
    pub hq: bool,
    pub sample_size_30d: u32,
    pub cleaned_sample_size_30d: u32,
    pub vwap_30d: u32,
    pub p50_30d: u32,
    pub confidence_band: ConfidenceBand,
    /// 0.0-1.0 — fraction of recent sales flagged by the noise filter.
    pub launder_suspicion: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemStatsResponse {
    pub world_id: i32,
    pub item_id: i32,
    pub variants: Vec<ItemStatsVariant>,
}

impl ItemStatsResponse {
    /// Pick the variant matching a `hq` flag. Falls back to NQ if the HQ
    /// variant doesn't exist (some items have no HQ form), or returns
    /// `None` if neither variant is present in the rollup yet.
    pub fn variant_for(&self, hq: bool) -> Option<&ItemStatsVariant> {
        self.variants
            .iter()
            .find(|v| v.hq == hq)
            .or_else(|| self.variants.first())
    }
}
