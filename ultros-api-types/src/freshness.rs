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
    use chrono::Duration;

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
}
