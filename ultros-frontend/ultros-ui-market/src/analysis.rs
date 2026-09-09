//! Presentation helpers for analyzer results. Calculations live in `ultros-calc`.

pub use ultros_calc::analysis::*;

/// Renders a duration as a compact "Xd Yh" / "Xh Ym" / "Xm Ys" string (up to two units).
/// Used by analyzer tables for the avg-sale-duration column.
pub fn format_duration_short(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    let seconds = secs % 60;
    let mut parts: Vec<String> = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 && parts.len() < 2 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 && parts.len() < 2 {
        parts.push(format!("{}s", seconds));
    }
    if parts.is_empty() {
        "0s".to_string()
    } else {
        parts[..parts.len().min(2)].join(" ")
    }
}

/// Tailwind class string for the ROI badge in analyzer tables. Tints the badge with the
/// brand-ring color, proportional to ROI %.
pub fn roi_badge_class(roi: i32) -> &'static str {
    if roi >= 500 {
        "inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_24%,transparent)]"
    } else if roi >= 200 {
        "inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_20%,transparent)]"
    } else if roi >= 100 {
        "inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_16%,transparent)]"
    } else if roi >= 50 {
        "inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)]"
    } else {
        "inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]"
    }
}

/// The noise floor a signed percent must clear before it is coloured.
/// Origin: the flip finder's Drift cell, where ±1% inside a six-sale window
/// is noise wearing a percentage sign. Reused by the recipe analyzer's
/// Drift column and its Price "vs median" tell, which read the same kind of
/// small, sample-limited percentage.
pub const DELTA_DEAD_BAND_PCT: f32 = 1.0;

/// The colour class for a signed percentage: green above `+dead_band`, red
/// below `-dead_band`, muted inside the band and when there is no figure.
/// `dead_band` is the caller's noise floor (0.0 colours every non-zero
/// sign). A NaN falls through both comparisons and reads neutral.
pub fn signed_delta_class(pct: Option<f32>, dead_band: f32) -> &'static str {
    match pct {
        Some(p) if p > dead_band => "text-emerald-300",
        Some(p) if p < -dead_band => "text-red-300",
        _ => "text-[color:var(--color-text-muted)]",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_short() {
        assert_eq!(format_duration_short(0), "0s");
        assert_eq!(format_duration_short(45), "45s");
        assert_eq!(format_duration_short(60), "1m");
        assert_eq!(format_duration_short(65), "1m 5s");
        assert_eq!(format_duration_short(3600), "1h");
        assert_eq!(format_duration_short(3665), "1h 1m");
        assert_eq!(format_duration_short(86400), "1d");
        assert_eq!(format_duration_short(90000), "1d 1h");
        // drops minutes because we only keep 2 units
        assert_eq!(format_duration_short(90060), "1d 1h");
    }

    #[test]
    fn test_roi_badge_class() {
        assert!(roi_badge_class(49).contains("10%"));
        assert!(roi_badge_class(50).contains("12%"));
        assert!(roi_badge_class(100).contains("16%"));
        assert!(roi_badge_class(200).contains("20%"));
        assert!(roi_badge_class(500).contains("24%"));
    }

    #[test]
    fn test_roi_badge_class_edge_cases() {
        assert!(roi_badge_class(0).contains("10%"));
        assert!(roi_badge_class(-50).contains("10%"));

        // Just under boundaries
        assert!(roi_badge_class(49).contains("10%"));
        assert!(roi_badge_class(99).contains("12%"));
        assert!(roi_badge_class(199).contains("16%"));
        assert!(roi_badge_class(499).contains("20%"));

        // Exactly on boundaries
        assert!(roi_badge_class(50).contains("12%"));
        assert!(roi_badge_class(100).contains("16%"));
        assert!(roi_badge_class(200).contains("20%"));
        assert!(roi_badge_class(500).contains("24%"));

        // High numbers
        assert!(roi_badge_class(1000).contains("24%"));
        assert!(roi_badge_class(10000).contains("24%"));
    }

    #[test]
    fn test_format_duration_short_edge_cases() {
        assert_eq!(format_duration_short(1), "1s");
        assert_eq!(format_duration_short(59), "59s");
        assert_eq!(format_duration_short(3599), "59m 59s");
        assert_eq!(format_duration_short(3601), "1h 1s");
        assert_eq!(format_duration_short(86399), "23h 59m");
        assert_eq!(format_duration_short(86401), "1d 1s");

        // large number of days
        assert_eq!(format_duration_short(86400 * 365 + 3600), "365d 1h");
    }

    #[test]
    fn signed_delta_class_has_a_dead_band() {
        assert_eq!(signed_delta_class(Some(4.0), 1.0), "text-emerald-300");
        assert_eq!(signed_delta_class(Some(-4.0), 1.0), "text-red-300");
        // Inside the band, and exactly on it, read neutral.
        let muted = "text-[color:var(--color-text-muted)]";
        assert_eq!(signed_delta_class(Some(0.4), 1.0), muted);
        assert_eq!(signed_delta_class(Some(1.0), 1.0), muted);
        assert_eq!(signed_delta_class(Some(-1.0), 1.0), muted);
        assert_eq!(signed_delta_class(None, 1.0), muted);
        // A zero dead band colours any non-zero sign (the movers' rule).
        assert_eq!(signed_delta_class(Some(0.2), 0.0), "text-emerald-300");
        // NaN is neither above nor below: neutral, never a panic.
        assert_eq!(signed_delta_class(Some(f32::NAN), 1.0), muted);
    }

    /// `analyzer.rs`'s three Drift arms cut at ±1.0 with `text-emerald-300`
    /// / `text-red-300` / muted; the new const and fn must reproduce exactly
    /// those thresholds (`signed_delta_class_has_a_dead_band` passes `1.0`
    /// by hand and so cannot pin the const). The cell's *text* is unchanged
    /// by construction — the fold touches only the class, and `+{d:.0}%`
    /// and `{d:.0}%` are `{d:+.0}%` over the ranges the old arms guarded, a
    /// property of the `+` flag rather than of this code — so the byte
    /// identity of `/flip-finder` rides on `routes::analyzer`'s 69 existing
    /// tests plus manual check 9 in the PR body.
    #[test]
    fn signed_delta_class_reproduces_the_flip_finders_drift_arms() {
        for d in [1.4f32, 4.6, 12.5, 99.5, 100.4] {
            assert_eq!(
                signed_delta_class(Some(d), DELTA_DEAD_BAND_PCT),
                "text-emerald-300"
            );
        }
        for d in [-1.4f32, -3.6, -50.0] {
            assert_eq!(
                signed_delta_class(Some(d), DELTA_DEAD_BAND_PCT),
                "text-red-300"
            );
        }
        for d in [0.0f32, 0.9, -0.9] {
            assert_eq!(
                signed_delta_class(Some(d), DELTA_DEAD_BAND_PCT),
                "text-[color:var(--color-text-muted)]"
            );
        }
    }
}
