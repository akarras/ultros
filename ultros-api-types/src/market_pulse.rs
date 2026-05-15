//! Rolled-up KPIs for the home-page Market Pulse strip.
//!
//! The server fills this from a ClickHouse query against `world_kpi_5min`;
//! see [`ultros_clickhouse::queries::market_pulse`] for the source-side type.
//! We keep a separate wire type here so the api-types crate doesn't depend
//! on the ClickHouse driver.

use serde::{Deserialize, Serialize};

/// Sales / volume KPIs for the trailing 24h plus the matching window
/// 24-48h ago, so the frontend can render delta-vs-yesterday on each card.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketPulseDto {
    pub world_id: i32,
    pub sales_today: u64,
    pub sales_yesterday: u64,
    pub gil_volume_today: u64,
    pub gil_volume_yesterday: u64,
    pub unit_volume_today: u64,
    pub unit_volume_yesterday: u64,
    /// Snapshot count of active marketboard listings on this world.
    /// Filled from Postgres at request time — separate from the
    /// time-series fields above which come from ClickHouse rollups.
    pub active_listings: u64,
}

impl MarketPulseDto {
    /// % change today vs yesterday for sale_count. Returns `None` when
    /// yesterday was zero so the UI can render "—" instead of a fake delta.
    pub fn sales_delta_pct(&self) -> Option<f32> {
        pct_delta(self.sales_today, self.sales_yesterday)
    }
    pub fn gil_volume_delta_pct(&self) -> Option<f32> {
        pct_delta(self.gil_volume_today, self.gil_volume_yesterday)
    }
    pub fn unit_volume_delta_pct(&self) -> Option<f32> {
        pct_delta(self.unit_volume_today, self.unit_volume_yesterday)
    }
}

fn pct_delta(today: u64, yesterday: u64) -> Option<f32> {
    if yesterday == 0 {
        None
    } else {
        Some(((today as f64 - yesterday as f64) / yesterday as f64 * 100.0) as f32)
    }
}
