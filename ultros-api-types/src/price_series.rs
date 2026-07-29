//! Pre-bucketed price/volume series — the chart's data source.
//!
//! Buckets carry `gil` and `units` rather than a precomputed VWAP so a
//! consumer can re-derive VWAP over any subset of buckets with exact
//! integer arithmetic (the timeline slicer needs this).
//!
//! `open`/`high`/`low`/`close` and the quantiles are computed server-side at
//! the requested [`SeriesGroup`]. Quantiles are deliberately *not*
//! re-aggregatable client-side: a datacenter's p50 is not any function of
//! its worlds' p50s, which is why grouping is a request parameter rather
//! than a client-side transform.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::CompactSale;

/// Which level of the world hierarchy the server aggregated at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeriesGroup {
    Region,
    Datacenter,
    World,
}

impl SeriesGroup {
    /// Stable identifier for query strings and cache keys.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Region => "region",
            Self::Datacenter => "datacenter",
            Self::World => "world",
        }
    }
}

/// Quality filter applied server-side, replacing the old client-side
/// `retain(|s| s.hq)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HqFilter {
    #[default]
    Any,
    Hq,
    Nq,
}

impl HqFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Hq => "hq",
            Self::Nq => "nq",
        }
    }
}

/// One time bucket for one series.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceBucket {
    /// Bucket start, naive UTC, aligned to absolute time.
    pub ts: NaiveDateTime,
    /// Price of the earliest sale in the bucket.
    pub open: i32,
    pub high: i32,
    pub low: i32,
    /// Price of the latest sale in the bucket.
    pub close: i32,
    /// Sum of `price_per_item * quantity`.
    pub gil: i64,
    /// Sum of `quantity`.
    pub units: i64,
    /// Number of sale rows, *not* units. Drives sparse-bucket handling.
    pub sales: u32,
    pub p25: i32,
    pub p50: i32,
    pub p75: i32,
}

impl PriceBucket {
    /// Volume-weighted average price. `None` when the bucket moved no units,
    /// which the caller should render as a gap rather than a zero.
    pub fn vwap(&self) -> Option<f64> {
        (self.units > 0).then(|| self.gil as f64 / self.units as f64)
    }
}

/// All buckets for one series, keyed by the selector id at the response's
/// [`SeriesGroup`] (world id, datacenter id, or region id).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceSeriesEntry {
    pub id: i32,
    /// Sorted by `ts` ascending. Buckets with no sales are absent, not zero —
    /// consumers must handle gaps.
    pub buckets: Vec<PriceBucket>,
}

/// Response payload for `/api/v1/price_series/{world}/{itemid}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceSeries {
    /// Bucket width the server actually chose, so the client labels axes
    /// without re-deriving it.
    pub bucket_seconds: i64,
    pub group: SeriesGroup,
    /// Time domain actually covered by the data, not the requested range.
    pub from: NaiveDateTime,
    pub to: NaiveDateTime,
    pub series: Vec<PriceSeriesEntry>,
    /// Raw sales, present only when the window holds few enough of them to
    /// draw individually. See `RAW_SALE_LIMIT` in the web handler.
    pub raw: Option<Vec<CompactSale>>,
}

impl PriceSeries {
    /// True when every series is empty — the "No recent sales" case.
    pub fn is_empty(&self) -> bool {
        self.series.iter().all(|s| s.buckets.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket(gil: i64, units: i64) -> PriceBucket {
        PriceBucket {
            ts: chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc(),
            open: 1,
            high: 1,
            low: 1,
            close: 1,
            gil,
            units,
            sales: 1,
            p25: 1,
            p50: 1,
            p75: 1,
        }
    }

    #[test]
    fn vwap_divides_gil_by_units() {
        assert_eq!(bucket(1000, 4).vwap(), Some(250.0));
    }

    #[test]
    fn vwap_is_none_without_units() {
        assert_eq!(bucket(1000, 0).vwap(), None);
    }

    #[test]
    fn series_group_round_trips_through_json() {
        let json = serde_json::to_string(&SeriesGroup::Datacenter).unwrap();
        assert_eq!(json, "\"datacenter\"");
        assert_eq!(
            serde_json::from_str::<SeriesGroup>(&json).unwrap(),
            SeriesGroup::Datacenter
        );
    }
}
