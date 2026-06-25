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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pct_delta_increase() {
        assert_eq!(pct_delta(150, 100), Some(50.0));
    }

    #[test]
    fn test_pct_delta_decrease() {
        assert_eq!(pct_delta(50, 100), Some(-50.0));
    }

    #[test]
    fn test_pct_delta_zero_yesterday() {
        assert_eq!(pct_delta(100, 0), None);
    }

    #[test]
    fn test_pct_delta_zero_today() {
        assert_eq!(pct_delta(0, 100), Some(-100.0));
    }

    #[test]
    fn test_pct_delta_no_change() {
        assert_eq!(pct_delta(100, 100), Some(0.0));
    }

    #[test]
    fn test_market_pulse_dto_deltas() {
        let dto = MarketPulseDto {
            world_id: 1,
            sales_today: 120,
            sales_yesterday: 100,
            gil_volume_today: 90,
            gil_volume_yesterday: 100,
            unit_volume_today: 0,
            unit_volume_yesterday: 0,
            active_listings: 100,
        };

        assert_eq!(dto.sales_delta_pct(), Some(20.0));
        assert_eq!(dto.gil_volume_delta_pct(), Some(-10.0));
        assert_eq!(dto.unit_volume_delta_pct(), None);
    }
}
