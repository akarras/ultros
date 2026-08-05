//! URL query encoding for the item-page price chart.
//!
//! Every function here is pure: no reactive reads, no clock access (`now` is
//! always a parameter). That keeps the whole encoding layer unit-testable,
//! which matters because on a local debug build `query_signal` *writes* are
//! inert while reads still work — these tests are the only place the
//! round-trip behaviour can actually be verified without a release build.

#![allow(dead_code)]

use std::str::FromStr;

/// A quick-range button. Anchored to *now*, not to the newest data point, so
/// a shared `?range=7d` link means the same thing to every viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangePreset {
    Week,
    Month,
    Year,
}

impl RangePreset {
    /// Display order for the button row.
    pub const ALL: [RangePreset; 3] = [Self::Week, Self::Month, Self::Year];

    /// Window length in seconds. A month is 30 days and a year 365; these
    /// are button labels, not calendar arithmetic.
    pub fn seconds(self) -> i64 {
        const DAY: i64 = 86_400;
        match self {
            Self::Week => 7 * DAY,
            Self::Month => 30 * DAY,
            Self::Year => 365 * DAY,
        }
    }
}

/// Wire format for `?range=`. Stable — part of every shared chart link.
impl std::fmt::Display for RangePreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Week => "7d",
            Self::Month => "1mo",
            Self::Year => "1y",
        })
    }
}

impl FromStr for RangePreset {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "7d" => Ok(Self::Week),
            "1mo" => Ok(Self::Month),
            "1y" => Ok(Self::Year),
            _ => Err(()),
        }
    }
}

/// Resolve the URL's range params into an absolute window, or `None` for
/// full range.
///
/// A preset wins over absolute bounds: `?range=` is what a preset click
/// writes, so its presence means the link is deliberately relative.
/// `normalize_time_range` clamps the result to the available domain later,
/// which is what makes `1y` on a six-month-old item show those six months
/// rather than erroring.
pub fn resolve_range(
    preset: Option<RangePreset>,
    from_to: Option<(i64, i64)>,
    now: i64,
) -> Option<(i64, i64)> {
    match preset {
        Some(preset) => Some((now - preset.seconds(), now)),
        None => from_to,
    }
}

/// Whether a preset's window contains any data at all.
///
/// False means the newest sale predates the whole window, so clicking the
/// button would blank the chart — the button is disabled with a reason
/// instead.
pub fn preset_has_data(preset: RangePreset, domain_end: i64, now: i64) -> bool {
    domain_end >= now - preset.seconds()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    // 2026-07-05 18:00:00 UTC — a fixed "now" so tests never depend on
    // the wall clock.
    const NOW: i64 = 1_783_360_800;

    #[test]
    fn range_preset_wire_format_round_trips() {
        for preset in RangePreset::ALL {
            assert_eq!(preset.to_string().parse::<RangePreset>(), Ok(preset));
        }
        assert_eq!(RangePreset::Week.to_string(), "7d");
        assert_eq!(RangePreset::Month.to_string(), "1mo");
        assert_eq!(RangePreset::Year.to_string(), "1y");
    }

    #[test]
    fn range_preset_parsing_is_forgiving() {
        assert_eq!("7D".parse::<RangePreset>(), Ok(RangePreset::Week));
        assert_eq!(" 1mo ".parse::<RangePreset>(), Ok(RangePreset::Month));
        assert_eq!("nonsense".parse::<RangePreset>(), Err(()));
    }

    #[test]
    fn a_preset_resolves_to_a_window_ending_now() {
        assert_eq!(
            resolve_range(Some(RangePreset::Week), None, NOW),
            Some((NOW - 7 * DAY, NOW))
        );
    }

    // The spec's precedence rule: a link carrying both shapes is a relative
    // link, because `range` is what a preset click writes.
    #[test]
    fn a_preset_wins_over_absolute_bounds() {
        assert_eq!(
            resolve_range(Some(RangePreset::Week), Some((1, 2)), NOW),
            Some((NOW - 7 * DAY, NOW))
        );
    }

    #[test]
    fn absolute_bounds_are_used_when_no_preset_is_set() {
        assert_eq!(resolve_range(None, Some((1, 2)), NOW), Some((1, 2)));
    }

    #[test]
    fn no_params_means_full_range() {
        assert_eq!(resolve_range(None, None, NOW), None);
    }

    // The dead-item case: an item whose newest sale predates the whole
    // window would render blank, so the button is disabled instead.
    #[test]
    fn a_preset_with_no_data_in_window_is_unavailable() {
        assert!(!preset_has_data(RangePreset::Week, NOW - 30 * DAY, NOW));
        assert!(preset_has_data(RangePreset::Month, NOW - 30 * DAY + 1, NOW));
    }

    // A domain ending exactly at the window's start still contains that
    // boundary bucket — off-by-one here silently disables a usable button.
    #[test]
    fn a_domain_ending_exactly_at_the_window_start_is_available() {
        assert!(preset_has_data(RangePreset::Week, NOW - 7 * DAY, NOW));
    }
}
