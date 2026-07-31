//! Pure "is this resale row real?" policy.
//!
//! Split out of `analyzer_service` so the thresholds are unit-testable
//! without standing up an `AnalyzerService`. Every signal here is derived
//! from data present on 100% of rows (the 6-sale buffer, listing prices,
//! and xiv-gen vendor prices) — ClickHouse enrichment covers ~7% of traded
//! items and therefore cannot gate default behavior.
//!
//! The problem this solves: FFXIV caps direct trades at 1,000,000 gil, so a
//! player moving currency between characters lists a worthless item at an
//! enormous price and buys it from themselves. Ranking resale rows by
//! absolute profit selects for exactly those trades.

/// Multiple of an item's NPC vendor price above which a claimed sale price
/// is arithmetically impossible rather than merely aggressive. Matches the
/// guard already used by the frontend's `real_price`.
pub(crate) const VENDOR_ANCHOR_MULTIPLE: i64 = 100;

/// Minimum span used as the velocity denominator. Guards the degenerate
/// case of six listings cleared in one action, which would divide by zero.
pub(crate) const MIN_VELOCITY_SPAN_DAYS: f32 = 1.0 / 24.0;

/// Median that picks the **lower** middle on even-length input.
///
/// The upper-middle pick resolves a two-sale laundering pair to the higher
/// of the two, which is the worst possible choice for a valuation. Odd
/// lengths and single elements are unaffected.
pub(crate) fn conservative_median(prices: &mut [i32]) -> i32 {
    let idx = (prices.len() - 1) / 2;
    let (_, &mut value, _) = prices.select_nth_unstable(idx);
    value
}

/// Recent sales per day from the bounded 6-sale buffer.
///
/// Mirrors `analysis::velocity_per_day` on the frontend so the card and the
/// Flip Finder can never disagree about the same item. Because the buffer
/// holds the *most recent* sales, this estimates the current rate rather
/// than a lifetime average; resolution degrades only at the high end, which
/// does not matter for a floor-style filter.
pub(crate) fn velocity_per_day(count: usize, span_days: f32) -> Option<f32> {
    if count == 0 {
        return None;
    }
    Some(count as f32 / span_days.max(MIN_VELOCITY_SPAN_DAYS))
}

/// A pass-1 resale row, with everything the policy needs to judge it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Candidate {
    pub(crate) est_sale_price: i32,
    pub(crate) return_on_investment: f32,
    pub(crate) velocity_per_day: Option<f32>,
    pub(crate) buffer_sale_count: u8,
    /// xiv-gen `price_mid`; 0 when the item is not vendor-sold.
    pub(crate) vendor_price: u32,
}

/// Caller-tunable strictness. `Default` applies only the vendor anchor, so
/// existing callers (the Discord `/analyze` command) are unaffected — that
/// anchor rejects arithmetically impossible valuations, not merely
/// aggressive ones.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EligibilityPolicy {
    pub(crate) min_velocity_per_day: Option<f32>,
    pub(crate) min_buffer_sales: Option<u8>,
    pub(crate) max_roi: Option<f32>,
}

impl EligibilityPolicy {
    pub(crate) fn accepts(&self, row: &Candidate) -> bool {
        if row.vendor_price > 0
            && row.est_sale_price as i64 > row.vendor_price as i64 * VENDOR_ANCHOR_MULTIPLE
        {
            return false;
        }
        if let Some(min) = self.min_velocity_per_day {
            if row.velocity_per_day.map(|v| v < min).unwrap_or(true) {
                return false;
            }
        }
        if let Some(min) = self.min_buffer_sales {
            if row.buffer_sale_count < min {
                return false;
            }
        }
        if let Some(max) = self.max_roi {
            if row.return_on_investment > max {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conservative_median_picks_lower_middle_when_even() {
        assert_eq!(conservative_median(&mut [10, 252_000_000]), 10);
        assert_eq!(conservative_median(&mut [1, 2, 3, 4]), 2);
        assert_eq!(conservative_median(&mut [4, 3, 2, 1]), 2);
    }

    #[test]
    fn conservative_median_unchanged_for_odd_and_single() {
        assert_eq!(conservative_median(&mut [1, 2, 3]), 2);
        assert_eq!(conservative_median(&mut [42]), 42);
        assert_eq!(conservative_median(&mut [5, 1, 3, 2, 4]), 3);
    }

    #[test]
    fn velocity_is_count_over_span() {
        // 6 sales across 30 days = 0.2/day
        let v = velocity_per_day(6, 30.0).expect("velocity");
        assert!((v - 0.2).abs() < 1e-5, "got {v}");
    }

    #[test]
    fn velocity_clamps_zero_span_instead_of_dividing_by_zero() {
        // Six listings cleared in one action: span 0.
        let v = velocity_per_day(6, 0.0).expect("velocity");
        assert!(v.is_finite(), "velocity must stay finite, got {v}");
        assert!((v - 6.0 / MIN_VELOCITY_SPAN_DAYS).abs() < 1e-3, "got {v}");
    }

    #[test]
    fn velocity_is_none_without_sales() {
        assert_eq!(velocity_per_day(0, 30.0), None);
    }

    #[test]
    fn velocity_of_stale_buffer_is_near_zero() {
        // 2 laundering sales two years apart.
        let v = velocity_per_day(2, 730.0).expect("velocity");
        assert!(v < 0.01, "got {v}");
    }

    fn candidate() -> Candidate {
        Candidate {
            est_sale_price: 21_450,
            return_on_investment: 68.0,
            velocity_per_day: Some(0.4),
            buffer_sale_count: 6,
            vendor_price: 0,
        }
    }

    #[test]
    fn vendor_anchor_rejects_impossible_valuation() {
        // Hempen Coif: ~50 gil vendor price, 42M claimed sale.
        let row = Candidate {
            est_sale_price: 42_000_000,
            vendor_price: 50,
            ..candidate()
        };
        assert!(!EligibilityPolicy::default().accepts(&row));
    }

    #[test]
    fn vendor_anchor_allows_up_to_the_multiple() {
        let at_limit = Candidate {
            est_sale_price: 5_000,
            vendor_price: 50,
            ..candidate()
        };
        let over = Candidate {
            est_sale_price: 5_001,
            vendor_price: 50,
            ..candidate()
        };
        assert!(EligibilityPolicy::default().accepts(&at_limit));
        assert!(!EligibilityPolicy::default().accepts(&over));
    }

    #[test]
    fn vendor_anchor_ignores_non_vendor_items() {
        // price_mid == 0 means "not sold by an NPC vendor".
        let row = Candidate {
            est_sale_price: 42_000_000,
            vendor_price: 0,
            ..candidate()
        };
        assert!(EligibilityPolicy::default().accepts(&row));
    }

    #[test]
    fn velocity_floor_rejects_below_threshold() {
        let policy = EligibilityPolicy {
            min_velocity_per_day: Some(0.2),
            ..Default::default()
        };
        assert!(!policy.accepts(&Candidate {
            velocity_per_day: Some(0.19),
            ..candidate()
        }));
        assert!(policy.accepts(&Candidate {
            velocity_per_day: Some(0.2),
            ..candidate()
        }));
    }

    #[test]
    fn velocity_floor_rejects_unknown_velocity() {
        let policy = EligibilityPolicy {
            min_velocity_per_day: Some(0.2),
            ..Default::default()
        };
        assert!(!policy.accepts(&Candidate {
            velocity_per_day: None,
            ..candidate()
        }));
    }

    #[test]
    fn buffer_sale_count_floor_applies() {
        let policy = EligibilityPolicy {
            min_buffer_sales: Some(2),
            ..Default::default()
        };
        assert!(!policy.accepts(&Candidate {
            buffer_sale_count: 1,
            ..candidate()
        }));
        assert!(policy.accepts(&Candidate {
            buffer_sale_count: 2,
            ..candidate()
        }));
    }

    #[test]
    fn roi_ceiling_rejects_above_threshold() {
        let policy = EligibilityPolicy {
            max_roi: Some(5000.0),
            ..Default::default()
        };
        assert!(!policy.accepts(&Candidate {
            return_on_investment: 6_984_380.0,
            ..candidate()
        }));
        assert!(policy.accepts(&Candidate {
            return_on_investment: 5000.0,
            ..candidate()
        }));
        // A legitimate cheap-item flip: 715 -> 10,715 gil is a 1400% return.
        assert!(policy.accepts(&Candidate {
            return_on_investment: 1400.0,
            ..candidate()
        }));
    }

    #[test]
    fn default_policy_only_applies_the_vendor_anchor() {
        // The Discord command passes no gates; it must keep seeing everything
        // except arithmetically impossible rows.
        let policy = EligibilityPolicy::default();
        assert!(policy.accepts(&Candidate {
            velocity_per_day: None,
            buffer_sale_count: 1,
            return_on_investment: 900_000.0,
            ..candidate()
        }));
    }
}
