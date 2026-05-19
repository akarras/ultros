//! Cross-cutting "is this row safe to show the user?" policy.
//!
//! Pulled out of `analyzer_service::get_best_resale` so the same call —
//! the Trends page, the Flip Finder, the Top Opportunities card, and any
//! future resale surface — produces the same hide/show decision. The
//! threshold and the "show suspicious" escape hatch live here once instead
//! of being recoded per call-site.
//!
//! Inputs:
//! - `confidence_band` (derived from the rollup) — `Unusable` is the
//!   strongest signal we have that a row is currency-transfer launder.
//! - `launder_suspicion_pct` (0.0-1.0) — direct fraction of the 30-day
//!   sample that the noise filter rejected. The 0.7 default threshold
//!   matches the analyzer rollup's own internal cutoff for flagging an
//!   item as launder-polluted.
//!
//! Rows without a [`DeepScan`] (no CH coverage at all) are kept — we don't
//! penalize items that haven't been rolled up yet, on the same Pass-1
//! "show with `ConfidenceBand::Unknown`" principle the rest of the
//! analyzer uses.

use crate::queries::DeepScan;
use ultros_api_types::trends::ConfidenceBand;

/// Filter policy applied to a list of (item, hq, world) rows enriched
/// with [`DeepScan`] data. Each surface (analyzer, trends, top
/// opportunities) constructs one of these and applies it once.
#[derive(Debug, Clone, Copy)]
pub struct ResaleQualityFilter {
    /// When true, no rows are dropped — the FE can render the suspicious
    /// ones with a warning chip. Drives the per-page "Show suspicious"
    /// toggle.
    pub include_suspicious: bool,
    /// Fraction of the cleaned sample flagged as noise above which a row
    /// is considered suspicious. 0.7 matches the rollup's own threshold;
    /// lowering it makes the filter stricter, raising it more permissive.
    pub launder_threshold: f32,
}

impl Default for ResaleQualityFilter {
    fn default() -> Self {
        Self {
            include_suspicious: false,
            launder_threshold: 0.7,
        }
    }
}

impl ResaleQualityFilter {
    /// Permissive variant: never hides anything. Equivalent to the
    /// `?show_suspicious=1` toggle being on.
    pub fn show_all() -> Self {
        Self {
            include_suspicious: true,
            launder_threshold: 0.7,
        }
    }

    /// Returns true if this row should be surfaced. Rows with no
    /// [`DeepScan`] are always kept.
    pub fn keep(&self, scan: Option<&DeepScan>) -> bool {
        if self.include_suspicious {
            return true;
        }
        let Some(scan) = scan else {
            return true;
        };
        !Self::is_suspicious_inner(scan, self.launder_threshold)
    }

    /// Whether this row would be classified as suspicious under the
    /// default threshold. Useful when the FE wants to render a warning
    /// chip on rows that would otherwise have been filtered.
    pub fn is_suspicious(scan: &DeepScan) -> bool {
        Self::is_suspicious_inner(scan, 0.7)
    }

    fn is_suspicious_inner(scan: &DeepScan, launder_threshold: f32) -> bool {
        matches!(scan.confidence_band(), ConfidenceBand::Unusable)
            || scan.launder_suspicion_pct > launder_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(band: &str, launder: f32) -> DeepScan {
        DeepScan {
            item_id: 1,
            hq: 0,
            world_id: 40,
            window_days: 30,
            vwap: 100,
            p50: 100,
            p10: 50,
            p25: 75,
            p75: 125,
            p90: 200,
            median_abs_deviation: 10,
            sample_size: 50,
            cleaned_sample_size: 45,
            excluded_count: 5,
            unit_volume: 100,
            gil_volume: 10_000,
            unique_buyers: 10,
            quality_score: 80,
            confidence_band_raw: band.to_string(),
            launder_suspicion_pct: launder,
        }
    }

    #[test]
    fn keep_when_no_scan_available() {
        let f = ResaleQualityFilter::default();
        assert!(f.keep(None));
    }

    #[test]
    fn keep_high_confidence_clean() {
        let f = ResaleQualityFilter::default();
        assert!(f.keep(Some(&scan("high", 0.0))));
        assert!(f.keep(Some(&scan("medium", 0.1))));
        assert!(f.keep(Some(&scan("low", 0.5))));
    }

    #[test]
    fn drop_unusable_by_default() {
        let f = ResaleQualityFilter::default();
        assert!(!f.keep(Some(&scan("unusable", 0.0))));
    }

    #[test]
    fn drop_high_launder_by_default() {
        let f = ResaleQualityFilter::default();
        assert!(!f.keep(Some(&scan("high", 0.71))));
        assert!(f.keep(Some(&scan("high", 0.70))));
    }

    #[test]
    fn show_suspicious_keeps_everything() {
        let f = ResaleQualityFilter::show_all();
        assert!(f.keep(Some(&scan("unusable", 0.99))));
        assert!(f.keep(Some(&scan("high", 0.99))));
        assert!(f.keep(None));
    }

    #[test]
    fn is_suspicious_flags_unusable_and_high_launder() {
        assert!(ResaleQualityFilter::is_suspicious(&scan("unusable", 0.0)));
        assert!(ResaleQualityFilter::is_suspicious(&scan("high", 0.8)));
        assert!(!ResaleQualityFilter::is_suspicious(&scan("high", 0.0)));
        assert!(!ResaleQualityFilter::is_suspicious(&scan("medium", 0.3)));
    }
}
