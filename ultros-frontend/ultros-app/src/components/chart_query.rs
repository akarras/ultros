//! URL query encoding for the item-page price chart.
//!
//! Every function here is pure: no reactive reads, no clock access (`now` is
//! always a parameter). That keeps the whole encoding layer unit-testable,
//! which matters because on a local debug build `query_signal` *writes* are
//! inert while reads still work — these tests are the only place the
//! round-trip behaviour can actually be verified without a release build.

use std::str::FromStr;

/// A quick-range button. Anchored to *now*, not to the newest data point, so
/// a shared `?range=7d` link means the same thing to every viewer.
///
/// Currently unused in this module; first wired into chart components in
/// Tasks 6–9 of the sale-history-chart-filters plan.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangePreset {
    Week,
    Month,
    Year,
}

impl RangePreset {
    /// Display order for the button row.
    ///
    /// Currently unused in this module; first wired into chart components in
    /// Tasks 6–9 of the sale-history-chart-filters plan.
    #[allow(dead_code)]
    pub const ALL: [RangePreset; 3] = [Self::Week, Self::Month, Self::Year];

    /// Window length in seconds. A month is 30 days and a year 365; these
    /// are button labels, not calendar arithmetic.
    ///
    /// Currently unused in this module; first wired into chart components in
    /// Tasks 6–9 of the sale-history-chart-filters plan.
    #[allow(dead_code)]
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
///
/// Currently unused in this module; first wired into chart components in
/// Tasks 6–9 of the sale-history-chart-filters plan.
#[allow(dead_code)]
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
///
/// Currently unused in this module; first wired into chart components in
/// Tasks 6–9 of the sale-history-chart-filters plan.
#[allow(dead_code)]
pub fn preset_has_data(preset: RangePreset, domain_end: i64, now: i64) -> bool {
    domain_end >= now - preset.seconds()
}

// ── `show`: a visibility expression ──────────────────────────────────────
//
//   show := base ("," item)*
//   base := "all" | "none"
//   item := ("+" | "-")? name
//
// Named `show` rather than `hide` because `hide=all` would read as "hide
// everything" while meaning the opposite. Under base `all` a bare or
// `-`-prefixed name excludes; under `none` a bare or `+`-prefixed name
// includes. The base may be omitted, in which case `all` is assumed, so a
// bare list still means "hide these".
//
// The encoder picks whichever base is shorter, which bounds the parameter to
// ceil(n/2) + 1 tokens — on a region page with 70 worlds that is the
// difference between a usable link and an unusable one.

/// Which series are visible before deltas are applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShowBase {
    All,
    None,
}

/// Resolve a `show` expression against the series currently on the chart,
/// returning the names that should be **hidden**.
///
/// Unknown names are ignored rather than rejected: the series set depends on
/// the grouping level, so a perfectly valid link can name series that don't
/// exist at the current level.
///
/// Currently unused in this module; first wired into chart components in
/// Tasks 6–9 of the sale-history-chart-filters plan.
#[allow(dead_code)]
pub fn parse_show(expr: &str, series: &[String]) -> Vec<String> {
    let tokens: Vec<&str> = expr
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect();
    let Some(first) = tokens.first() else {
        return Vec::new();
    };

    let (base, deltas) = match first.to_ascii_lowercase().as_str() {
        "all" => (ShowBase::All, &tokens[1..]),
        "none" => (ShowBase::None, &tokens[1..]),
        // No recognised base: treat the whole list as exclusions.
        _ => (ShowBase::All, &tokens[..]),
    };

    let mut visible = vec![base == ShowBase::All; series.len()];
    // Tracked so a *stale* `none` link can be told apart from a deliberate
    // "hide everything" — see the fail-open rule below.
    let mut includes = 0usize;
    let mut matched_includes = 0usize;

    for token in deltas {
        let (include, name) = match token.strip_prefix('+') {
            Some(name) => (true, name),
            None => match token.strip_prefix('-') {
                Some(name) => (false, name),
                // A bare name takes its polarity from the base.
                None => (base == ShowBase::None, *token),
            },
        };
        let name = name.trim();
        if include {
            includes += 1;
        }

        let mut matched = false;
        for (index, series_name) in series.iter().enumerate() {
            if series_name.eq_ignore_ascii_case(name) {
                visible[index] = include;
                matched = true;
            }
        }
        if include && matched {
            matched_includes += 1;
        }
    }

    // FAIL OPEN. A `none` base whose includes matched nothing is a link
    // authored against a different series set — most often a different
    // grouping level. Honouring it would render a blank chart, which is
    // indistinguishable from a bug. A `none` base with no deltas at all is
    // different: that is an explicit "hide everything" and round-trips
    // honestly.
    if base == ShowBase::None && includes > 0 && matched_includes == 0 {
        return Vec::new();
    }

    series
        .iter()
        .zip(&visible)
        .filter(|(_, visible)| !**visible)
        .map(|(name, _)| name.clone())
        .collect()
}

/// Encode the hidden set as the shortest valid `show` expression, or `None`
/// when nothing is hidden (the param is then omitted from the URL entirely).
///
/// Hidden names outside the current series set are dropped — otherwise the
/// expression would accumulate stale names as the user switches grouping.
///
/// Currently unused in this module; first wired into chart components in
/// Tasks 6–9 of the sale-history-chart-filters plan.
#[allow(dead_code)]
pub fn encode_show(hidden: &[String], series: &[String]) -> Option<String> {
    let is_hidden = |name: &String| hidden.iter().any(|entry| entry.eq_ignore_ascii_case(name));

    let mut hidden_names: Vec<&str> = series
        .iter()
        .filter(|name| is_hidden(name))
        .map(String::as_str)
        .collect();
    if hidden_names.is_empty() {
        return None;
    }
    let mut visible_names: Vec<&str> = series
        .iter()
        .filter(|name| !is_hidden(name))
        .map(String::as_str)
        .collect();

    // Ties favour `all`: unmatched exclusions are inert, so an `all`
    // expression can never fail open the way a stale `none` list can.
    let (base, sign, names) = if hidden_names.len() <= visible_names.len() {
        hidden_names.sort_unstable();
        ("all", '-', hidden_names)
    } else {
        visible_names.sort_unstable();
        ("none", '+', visible_names)
    };

    let mut encoded = String::from(base);
    for name in names {
        encoded.push(',');
        encoded.push(sign);
        encoded.push_str(name);
    }
    Some(encoded)
}

// ── `overlays`: which overlay toggles are on ─────────────────────────────

/// The chart's overlay toggles as one URL param.
///
/// A single comma-separated param rather than five booleans: five params
/// would dominate the query string, and they are read and written together.
///
/// Currently unused in this module; first wired into chart components in
/// Tasks 6–9 of the sale-history-chart-filters plan.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overlays {
    pub market_average: bool,
    pub trend: bool,
    pub quantity: bool,
    pub percent_change: bool,
    pub patches: bool,
}

impl Default for Overlays {
    /// Market average and patch bands on; the rest off. Matches the
    /// component defaults these params replace.
    fn default() -> Self {
        Self {
            market_average: true,
            trend: false,
            quantity: false,
            percent_change: false,
            patches: true,
        }
    }
}

impl std::fmt::Display for Overlays {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut tokens = Vec::with_capacity(5);
        if self.market_average {
            tokens.push("avg");
        }
        if self.trend {
            tokens.push("trend");
        }
        if self.quantity {
            tokens.push("qty");
        }
        if self.percent_change {
            tokens.push("pct");
        }
        if self.patches {
            tokens.push("patches");
        }
        // "Everything off" needs a sentinel: an empty value would parse back
        // as the default set, so it is the one state that could not survive
        // a round trip.
        if tokens.is_empty() {
            f.write_str("none")
        } else {
            f.write_str(&tokens.join(","))
        }
    }
}

impl FromStr for Overlays {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut overlays = Self {
            market_average: false,
            trend: false,
            quantity: false,
            percent_change: false,
            patches: false,
        };
        for token in s.split(',') {
            match token.trim().to_ascii_lowercase().as_str() {
                "avg" => overlays.market_average = true,
                "trend" => overlays.trend = true,
                "qty" => overlays.quantity = true,
                "pct" => overlays.percent_change = true,
                "patches" => overlays.patches = true,
                // "none", empty, and anything unrecognised: ignored, so a
                // link from a build with more overlays still applies the
                // tokens this build understands.
                _ => {}
            }
        }
        Ok(overlays)
    }
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

    fn series() -> Vec<String> {
        ["Gilgamesh", "Sargatanas", "Faerie", "Siren"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn hidden(expr: &str) -> Vec<String> {
        parse_show(expr, &series())
    }

    #[test]
    fn an_all_base_treats_listed_names_as_exclusions() {
        assert_eq!(hidden("all,-Gilgamesh"), vec!["Gilgamesh".to_string()]);
        // The sign is optional under `all`.
        assert_eq!(hidden("all,Gilgamesh"), vec!["Gilgamesh".to_string()]);
    }

    #[test]
    fn a_none_base_treats_listed_names_as_inclusions() {
        assert_eq!(
            hidden("none,+Gilgamesh,+Sargatanas"),
            vec!["Faerie".to_string(), "Siren".to_string()]
        );
        // The sign is optional under `none` too.
        assert_eq!(
            hidden("none,Gilgamesh,Sargatanas"),
            vec!["Faerie".to_string(), "Siren".to_string()]
        );
    }

    // Convenience for hand-authored links: a bare list implies `all`.
    #[test]
    fn an_omitted_base_implies_all() {
        assert_eq!(hidden("Gilgamesh"), vec!["Gilgamesh".to_string()]);
    }

    #[test]
    fn names_match_case_insensitively() {
        assert_eq!(hidden("all,-gilgamesh"), vec!["Gilgamesh".to_string()]);
        assert_eq!(hidden("ALL,-GILGAMESH"), vec!["Gilgamesh".to_string()]);
    }

    #[test]
    fn an_empty_expression_hides_nothing() {
        assert!(hidden("").is_empty());
        assert!(hidden("all").is_empty());
    }

    // Unmatched exclusions under `all` are simply inert — this is why `all`
    // is the safer base and wins ties in encode_show.
    #[test]
    fn unmatched_exclusions_are_inert() {
        assert!(hidden("all,-Nonexistent").is_empty());
    }

    // SAFETY RULE 1. The series set depends on the group level, so a link
    // authored at World grouping carries world names that match nothing at
    // Region grouping. `none` plus zero matches would blank the chart, and a
    // blank chart from a stale link is indistinguishable from a bug.
    #[test]
    fn a_stale_none_expression_fails_open() {
        assert!(hidden("none,+Europe,+Japan").is_empty());
    }

    // ...but a `none` base with NO deltas is an explicit, unambiguous
    // "hide everything", not a stale link. It must round-trip honestly.
    #[test]
    fn an_explicit_none_with_no_deltas_hides_everything() {
        assert_eq!(hidden("none"), series());
    }

    // A partially-stale expression is not stale: one match is enough to
    // prove the link still refers to this series set.
    #[test]
    fn a_partially_matching_none_expression_is_honoured() {
        assert_eq!(
            hidden("none,+Gilgamesh,+Europe"),
            vec![
                "Sargatanas".to_string(),
                "Faerie".to_string(),
                "Siren".to_string()
            ]
        );
    }

    // SAFETY RULE 2. leptos_router unescapes with decodeURIComponent /
    // percent_decode, NOT form-urlencoding, so `+` survives as a literal
    // rather than becoming a space. This test pins that assumption: if the
    // decoder ever changes, every `none,+...` link silently breaks.
    #[test]
    fn a_literal_plus_prefix_parses_as_an_inclusion() {
        assert_eq!(
            hidden("none,+Gilgamesh"),
            vec![
                "Sargatanas".to_string(),
                "Faerie".to_string(),
                "Siren".to_string()
            ]
        );
    }

    #[test]
    fn nothing_hidden_encodes_to_no_param() {
        assert_eq!(encode_show(&[], &series()), None);
    }

    #[test]
    fn a_minority_hidden_encodes_with_an_all_base() {
        assert_eq!(
            encode_show(&["Gilgamesh".to_string()], &series()),
            Some("all,-Gilgamesh".to_string())
        );
    }

    #[test]
    fn a_majority_hidden_encodes_with_a_none_base() {
        let hidden_names = [
            "Gilgamesh".to_string(),
            "Sargatanas".to_string(),
            "Faerie".to_string(),
        ];
        assert_eq!(
            encode_show(&hidden_names, &series()),
            Some("none,+Siren".to_string())
        );
    }

    // The user's requirement: never list more than about half the series.
    #[test]
    fn a_tie_favours_the_all_base() {
        let hidden_names = ["Gilgamesh".to_string(), "Sargatanas".to_string()];
        assert_eq!(
            encode_show(&hidden_names, &series()),
            Some("all,-Gilgamesh,-Sargatanas".to_string())
        );
    }

    #[test]
    fn deltas_are_emitted_alphabetically() {
        let hidden_names = ["Sargatanas".to_string(), "Gilgamesh".to_string()];
        assert_eq!(
            encode_show(&hidden_names, &series()),
            Some("all,-Gilgamesh,-Sargatanas".to_string())
        );
    }

    // Hidden names that aren't in the current series set must not reach the
    // URL, or the expression would grow without bound as the user switches
    // grouping levels.
    #[test]
    fn encoding_ignores_hidden_names_outside_the_series_set() {
        let hidden_names = ["Gilgamesh".to_string(), "Europe".to_string()];
        assert_eq!(
            encode_show(&hidden_names, &series()),
            Some("all,-Gilgamesh".to_string())
        );
    }

    #[test]
    fn show_round_trips_through_encode_and_parse() {
        let cases: Vec<Vec<String>> = vec![
            vec!["Gilgamesh".to_string()],
            vec!["Gilgamesh".to_string(), "Sargatanas".to_string()],
            vec![
                "Gilgamesh".to_string(),
                "Sargatanas".to_string(),
                "Faerie".to_string(),
            ],
        ];
        for hidden_names in cases {
            let encoded = encode_show(&hidden_names, &series()).unwrap();
            let mut round_tripped = parse_show(&encoded, &series());
            let mut expected = hidden_names.clone();
            round_tripped.sort();
            expected.sort();
            assert_eq!(round_tripped, expected, "via {encoded}");
        }
    }

    #[test]
    fn overlay_defaults_are_market_average_and_patches() {
        let overlays = Overlays::default();
        assert!(overlays.market_average);
        assert!(overlays.patches);
        assert!(!overlays.trend);
        assert!(!overlays.quantity);
        assert!(!overlays.percent_change);
    }

    #[test]
    fn overlays_round_trip() {
        let overlays = Overlays {
            market_average: true,
            trend: true,
            quantity: false,
            percent_change: false,
            patches: true,
        };
        assert_eq!(overlays.to_string(), "avg,trend,patches");
        assert_eq!(overlays.to_string().parse::<Overlays>(), Ok(overlays));
    }

    // Without a sentinel, "everything off" would encode to an empty value
    // and parse back as the default set — the one state that cannot survive
    // a round trip.
    #[test]
    fn all_overlays_off_round_trips_via_the_none_sentinel() {
        let overlays = Overlays {
            market_average: false,
            trend: false,
            quantity: false,
            percent_change: false,
            patches: false,
        };
        assert_eq!(overlays.to_string(), "none");
        assert_eq!("none".parse::<Overlays>(), Ok(overlays));
    }

    // Unknown tokens are ignored rather than rejected, so a link written by
    // a newer build that gained an overlay still applies the tokens this
    // build understands instead of falling back to the default set.
    #[test]
    fn unknown_overlay_tokens_are_ignored() {
        let parsed = "avg,newthing".parse::<Overlays>().unwrap();
        assert!(parsed.market_average);
        assert!(!parsed.patches);
    }

    #[test]
    fn overlay_parsing_is_forgiving() {
        let parsed = " AVG , trend ".parse::<Overlays>().unwrap();
        assert!(parsed.market_average);
        assert!(parsed.trend);
    }
}
