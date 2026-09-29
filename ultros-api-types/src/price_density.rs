//! Time × price sale-count grid — the density chart's data source.
//!
//! Unlike `price_series`, cells are *sparse*: only populated
//! `(bucket, bin)` pairs are shipped. Payload is bounded by
//! `buckets × price_bins` regardless of sale volume.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// One populated cell of the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DensityCell {
    /// Bucket start, naive UTC, aligned like `PriceBucket::ts`.
    pub ts: NaiveDateTime,
    /// Price-bin index in `0..price_bins`.
    pub bin: u16,
    /// Sale-row count in the cell.
    pub n: u32,
}

/// Response payload for `/api/v1/price_density/{world}/{itemid}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceDensity {
    /// Bucket width the server actually chose (same ladder as
    /// `PriceSeries::bucket_seconds`).
    pub bucket_seconds: i64,
    /// Requested window (density has no per-bucket domain to narrow to).
    pub from: NaiveDateTime,
    pub to: NaiveDateTime,
    /// Price of bin 0's lower edge.
    pub price_lo: i32,
    /// Uniform bin height in gil; always >= 1.
    pub bin_width: f64,
    pub price_bins: u16,
    pub cells: Vec<DensityCell>,
}

impl PriceDensity {
    /// `[lower, upper)` gil bounds of a bin, for axis labels and tooltips.
    pub fn bin_bounds(&self, bin: u16) -> (f64, f64) {
        let lower = self.price_lo as f64 + bin as f64 * self.bin_width;
        (lower, lower + self.bin_width)
    }

    /// Largest cell count — the top of the opacity ramp.
    pub fn max_count(&self) -> u32 {
        self.cells.iter().map(|c| c.n).max().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn density(cells: Vec<DensityCell>) -> PriceDensity {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc();
        PriceDensity {
            bucket_seconds: 86_400,
            from: epoch,
            to: epoch,
            price_lo: 100,
            bin_width: 25.0,
            price_bins: 4,
            cells,
        }
    }

    #[test]
    fn bin_bounds_step_by_bin_width_from_lo() {
        let d = density(Vec::new());
        assert_eq!(d.bin_bounds(0), (100.0, 125.0));
        assert_eq!(d.bin_bounds(3), (175.0, 200.0));
    }

    #[test]
    fn max_count_and_empty() {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc();
        let d = density(vec![
            DensityCell {
                ts: epoch,
                bin: 0,
                n: 3,
            },
            DensityCell {
                ts: epoch,
                bin: 1,
                n: 9,
            },
        ]);
        assert_eq!(d.max_count(), 9);
        assert!(!d.is_empty());
        assert!(density(Vec::new()).is_empty());
        assert_eq!(density(Vec::new()).max_count(), 0);
    }

    #[test]
    fn round_trips_through_json() {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc();
        let d = density(vec![DensityCell {
            ts: epoch,
            bin: 2,
            n: 7,
        }]);
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<PriceDensity>(&json).unwrap(), d);
    }
}
