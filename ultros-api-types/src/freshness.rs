use crate::SaleHistory;
use chrono::Duration;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FreshnessVerdict {
    /// The data is very recent relative to how fast the item sells.
    Fresh,
    /// The data is starting to get old; it might still be accurate but use with care.
    Caution,
    /// The data is old enough that it is likely inaccurate for this item's velocity.
    /// Checking in-game is recommended.
    VerifyInGame,
    /// Not enough information to determine freshness.
    #[default]
    NoData,
}

/// The freshness threshold for an item with no sales (in hours).
/// For an item that never sells, we trust data for up to 24 hours as "Fresh".
const BASE_FRESH_HOURS: f64 = 24.0;

/// The caution threshold for an item with no sales (in hours).
/// For an item that never sells, we trust data for up to 72 hours as "Caution".
/// Beyond this, it becomes "VerifyInGame".
const BASE_CAUTION_HOURS: f64 = 72.0;

/// How much each sale per day reduces the freshness window.
///
/// The threshold is calculated as: `BASE_THRESHOLD / (1.0 + (sales_per_day * VELOCITY_FACTOR))`
///
/// A factor of 1.0 means:
/// - 0 sales/day: 24h Fresh / 72h Caution
/// - 1 sale/day: 12h Fresh / 36h Caution
/// - 10 sales/day: ~2.1h Fresh / ~6.5h Caution
/// - 100 sales/day: ~14m Fresh / ~42m Caution
const VELOCITY_FACTOR: f64 = 1.0;

/// Estimates sales velocity (sales per day) from a window of recent sales.
///
/// Returns `None` whenever a rate cannot honestly be derived, so callers feed
/// [`calculate_freshness_verdict`] a `None` and get a [`FreshnessVerdict::NoData`]
/// instead of a fabricated number:
/// - no sales at all: `None` — an absence of data is *not* a confident zero
///   (a zero would select the most permissive freshness thresholds);
/// - exactly one sale: `None` — a single point has no time window;
/// - all sales share one timestamp: `None` — a zero-length window has no
///   finite rate (previously this produced a magic `100.0`).
///
/// The estimate is `(count - 1) / window_days`, i.e. the number of intervals
/// between the oldest and newest sale in the window. Sale order does not matter.
pub fn sales_per_day(sales: &[SaleHistory]) -> Option<f32> {
    if sales.len() < 2 {
        return None;
    }
    let newest = sales.iter().map(|sale| sale.sold_date).max()?;
    let oldest = sales.iter().map(|sale| sale.sold_date).min()?;
    let seconds = (newest - oldest).num_seconds();
    if seconds <= 0 {
        return None;
    }
    let intervals = (sales.len() - 1) as f32;
    Some(intervals / (seconds as f32 / 86_400.0))
}

/// Calculates a freshness verdict based on the age of a listing and its sales velocity.
///
/// If either `age` or `sales_per_day` is missing, returns [`FreshnessVerdict::NoData`].
pub fn calculate_freshness_verdict(
    age: Option<Duration>,
    sales_per_day: Option<f32>,
) -> FreshnessVerdict {
    let age = match age {
        Some(age) => age,
        None => return FreshnessVerdict::NoData,
    };

    let sales_per_day = match sales_per_day {
        Some(s) if s >= 0.0 => s as f64,
        _ => return FreshnessVerdict::NoData,
    };

    let age_hours = age.num_seconds() as f64 / 3600.0;

    let fresh_threshold = BASE_FRESH_HOURS / (1.0 + sales_per_day * VELOCITY_FACTOR);
    let caution_threshold = BASE_CAUTION_HOURS / (1.0 + sales_per_day * VELOCITY_FACTOR);

    if age_hours <= fresh_threshold {
        FreshnessVerdict::Fresh
    } else if age_hours <= caution_threshold {
        FreshnessVerdict::Caution
    } else {
        FreshnessVerdict::VerifyInGame
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Duration};

    fn sale_at(seconds: i64) -> SaleHistory {
        SaleHistory {
            id: seconds as i32,
            quantity: 1,
            price_per_item: 100,
            buying_character_id: 1,
            hq: false,
            sold_item_id: 1,
            sold_date: DateTime::from_timestamp(seconds, 0).unwrap().naive_utc(),
            world_id: 1,
            buyer_name: None,
        }
    }

    #[test]
    fn test_sales_per_day_empty_is_none() {
        // No sales is unknown velocity, NOT a confident zero: a zero would
        // select the most permissive thresholds and paint no-data items green.
        assert_eq!(sales_per_day(&[]), None);
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(1)), sales_per_day(&[])),
            FreshnessVerdict::NoData
        );
    }

    #[test]
    fn test_sales_per_day_single_sale_is_none() {
        // One sale has no window; consistent with the empty case.
        assert_eq!(sales_per_day(&[sale_at(0)]), None);
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(1)), sales_per_day(&[sale_at(0)])),
            FreshnessVerdict::NoData
        );
    }

    #[test]
    fn test_sales_per_day_zero_window_is_none() {
        // All sales at the same instant: no finite rate can be derived.
        let sales = vec![sale_at(1000), sale_at(1000), sale_at(1000)];
        assert_eq!(sales_per_day(&sales), None);
    }

    #[test]
    fn test_sales_per_day_simple_rates() {
        let day = 86_400;
        // 2 sales one day apart -> 1 sale/day.
        let sales = vec![sale_at(0), sale_at(day)];
        assert_eq!(sales_per_day(&sales), Some(1.0));

        // 5 sales spread over one day -> 4 intervals/day.
        let sales: Vec<_> = (0..5).map(|i| sale_at(i * day / 4)).collect();
        assert_eq!(sales_per_day(&sales), Some(4.0));

        // Order does not matter (min/max scan, not first/last).
        let sales = vec![sale_at(day), sale_at(0)];
        assert_eq!(sales_per_day(&sales), Some(1.0));
    }

    #[test]
    fn test_no_data() {
        assert_eq!(
            calculate_freshness_verdict(None, Some(1.0)),
            FreshnessVerdict::NoData
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(1)), None),
            FreshnessVerdict::NoData
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(1)), Some(-1.0)),
            FreshnessVerdict::NoData
        );
    }

    #[test]
    fn test_slow_mover() {
        let velocity = Some(0.0);
        // 0 sales/day: 24h Fresh / 72h Caution
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(12)), velocity),
            FreshnessVerdict::Fresh
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(24)), velocity),
            FreshnessVerdict::Fresh
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(25)), velocity),
            FreshnessVerdict::Caution
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(72)), velocity),
            FreshnessVerdict::Caution
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(73)), velocity),
            FreshnessVerdict::VerifyInGame
        );
    }

    #[test]
    fn test_steady_mover() {
        let velocity = Some(1.0);
        // 1 sale/day: 12h Fresh / 36h Caution
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(6)), velocity),
            FreshnessVerdict::Fresh
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(12)), velocity),
            FreshnessVerdict::Fresh
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(13)), velocity),
            FreshnessVerdict::Caution
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(36)), velocity),
            FreshnessVerdict::Caution
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(37)), velocity),
            FreshnessVerdict::VerifyInGame
        );
    }

    #[test]
    fn test_fast_mover() {
        let velocity = Some(10.0);
        // 10 sales/day: 24/11h (~2.18h) Fresh / 72/11h (~6.54h) Caution
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(2)), velocity),
            FreshnessVerdict::Fresh
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(3)), velocity),
            FreshnessVerdict::Caution
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(6)), velocity),
            FreshnessVerdict::Caution
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(7)), velocity),
            FreshnessVerdict::VerifyInGame
        );
    }

    #[test]
    fn test_threshold_edges() {
        let velocity = Some(0.0);
        // Exact thresholds
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(24)), velocity),
            FreshnessVerdict::Fresh
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(72)), velocity),
            FreshnessVerdict::Caution
        );

        // Just over
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::seconds(24 * 3600 + 1)), velocity),
            FreshnessVerdict::Caution
        );
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::seconds(72 * 3600 + 1)), velocity),
            FreshnessVerdict::VerifyInGame
        );
    }

    #[test]
    fn test_item_view_regression_focused_coverage() {
        use FreshnessVerdict::*;

        // 1. Reliable/current: Very recent data for a slow mover
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::minutes(5)), Some(0.1)),
            Fresh,
            "5-minute old data for slow mover should be Fresh"
        );

        // 2. Stale/verify-in-game: Old data for a very fast mover
        // 100 sales/day => Fresh threshold ~14m, Caution ~42m.
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(2)), Some(100.0)),
            VerifyInGame,
            "2-hour old data for ultra-fast mover should be VerifyInGame"
        );

        // 3. Missing/unknown timestamp
        assert_eq!(
            calculate_freshness_verdict(None, Some(1.0)),
            NoData,
            "Missing age should result in NoData"
        );

        // 4. Missing velocity (unknown state)
        assert_eq!(
            calculate_freshness_verdict(Some(Duration::hours(1)), None),
            NoData,
            "Missing velocity should result in NoData"
        );
    }
}
