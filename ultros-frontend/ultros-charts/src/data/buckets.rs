//! Time-bucketed aggregation: VWAP line vertices and volume bars. Bucket
//! boundaries align to absolute UTC timestamps so day/week buckets land on
//! calendar boundaries — ported from the web UI's quantity histogram.

use std::collections::BTreeMap;

use chrono::NaiveDateTime;

use crate::data::grouping::SalePoint;

const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;

/// Bucket width for VWAP lines / volume bars. `days_range` is the user's
/// selected window (7/30/90); `None` or 0 falls back to the data span.
pub fn bucket_seconds(days_range: Option<i32>, data_span_days: i64) -> i64 {
    let effective_days = match days_range {
        Some(days) if days > 0 => days as i64,
        _ => data_span_days.max(1),
    };
    match effective_days {
        ..=2 => HOUR,
        3..=10 => 6 * HOUR,
        11..=120 => DAY,
        121..=400 => 7 * DAY,
        _ => 30 * DAY,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VwapPoint {
    /// Bucket midpoint (the line vertex sits in the middle of its bucket).
    pub ts: NaiveDateTime,
    pub vwap: f64,
}

/// Volume-weighted average price per time bucket.
pub fn vwap_buckets(points: &[SalePoint], bucket_secs: i64) -> Vec<VwapPoint> {
    if bucket_secs <= 0 {
        return Vec::new();
    }
    let mut sums: BTreeMap<i64, (i64, i64)> = BTreeMap::new();
    for point in points {
        let bucket = point.ts.and_utc().timestamp().div_euclid(bucket_secs) * bucket_secs;
        let entry = sums.entry(bucket).or_default();
        entry.0 += point.price as i64 * point.quantity as i64;
        entry.1 += point.quantity as i64;
    }
    sums.into_iter()
        .filter(|(_, (_, quantity))| *quantity > 0)
        .filter_map(|(bucket, (gil, quantity))| {
            chrono::DateTime::from_timestamp(bucket + bucket_secs / 2, 0).map(|ts| VwapPoint {
                ts: ts.naive_utc(),
                vwap: gil as f64 / quantity as f64,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeBucket {
    /// Bucket start.
    pub ts: NaiveDateTime,
    pub quantity: i64,
}

/// Total quantity per bucket over grouped sale points (the chart feeds the
/// visible series' points here so hidden series don't count).
pub fn volume_buckets_from_points<'a>(
    points: impl Iterator<Item = &'a SalePoint>,
    bucket_secs: i64,
) -> Vec<VolumeBucket> {
    if bucket_secs <= 0 {
        return Vec::new();
    }
    let mut sums: BTreeMap<i64, i64> = BTreeMap::new();
    for point in points {
        let bucket = point.ts.and_utc().timestamp().div_euclid(bucket_secs) * bucket_secs;
        *sums.entry(bucket).or_default() += point.quantity as i64;
    }
    sums.into_iter()
        .filter_map(|(bucket, quantity)| {
            chrono::DateTime::from_timestamp(bucket, 0).map(|ts| VolumeBucket {
                ts: ts.naive_utc(),
                quantity,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::grouping::SalePoint;
    use crate::test_util::ts;

    #[test]
    fn bucket_seconds_scales_with_window() {
        assert_eq!(bucket_seconds(Some(7), 0), 6 * 3_600);
        assert_eq!(bucket_seconds(Some(30), 0), 86_400);
        assert_eq!(bucket_seconds(Some(90), 0), 86_400);
        assert_eq!(bucket_seconds(None, 2), 3_600);
        assert_eq!(bucket_seconds(None, 500), 30 * 86_400);
    }

    #[test]
    fn vwap_buckets_weight_by_quantity() {
        // 100×1 and 200×3 in the same day bucket → VWAP 175, vertex at midday
        let points = vec![
            SalePoint {
                ts: ts(0),
                price: 100,
                quantity: 1,
            },
            SalePoint {
                ts: ts(3_600),
                price: 200,
                quantity: 3,
            },
        ];
        let buckets = vwap_buckets(&points, 86_400);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].vwap, 175.0);
        assert_eq!(buckets[0].ts, ts(43_200));
    }

    #[test]
    fn volume_buckets_sum_quantities() {
        let points = vec![
            SalePoint { ts: ts(0), price: 100, quantity: 2 },
            SalePoint { ts: ts(60), price: 100, quantity: 3 },
            SalePoint { ts: ts(86_400), price: 100, quantity: 5 },
        ];
        let buckets = volume_buckets_from_points(points.iter(), 86_400);
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].quantity, 5);
        assert_eq!(buckets[1].quantity, 5);
    }
}
