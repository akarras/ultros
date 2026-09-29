//! What a column's numbers mean, so one filter editor can read `1.2m`,
//! `2d 4h` or `15%` and every chip can print the value back the same way.
//!
//! A bound is stored in the URL as a *token*: a plain decimal in the column's
//! base unit (gil, percentage points, seconds, hours, unix seconds), or, on a
//! timestamp column only, a relative token like `-7d` resolved against "now"
//! when rows are filtered. Tokens stay readable and match what the legacy
//! aliases already wrote (`next-sale=2d` became `172800` seconds).

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Unit {
    #[default]
    Plain,
    Gil,
    /// Percentage points: `15` is 15%.
    Percent,
    /// Per day, fractional.
    Rate,
    /// A duration stored in seconds.
    Seconds,
    /// A duration stored in hours.
    Hours,
    /// Unix seconds; bounds may also be relative to now.
    Timestamp,
}

impl Unit {
    pub fn is_duration(self) -> bool {
        matches!(self, Self::Seconds | Self::Hours)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitError {
    /// Not a value this unit can read.
    Invalid,
}

const MINUTE: f64 = 60.0;
const HOUR: f64 = 3_600.0;
const DAY: f64 = 86_400.0;
const WEEK: f64 = 604_800.0;

/// A typed bound as its URL token. `Ok(None)` is an empty field (an open end).
pub fn parse_input(unit: Unit, input: &str) -> Result<Option<String>, UnitError> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(None);
    }
    let value = match unit {
        Unit::Plain | Unit::Gil => parse_amount(input, true)?,
        Unit::Rate => parse_amount(input, false)?,
        Unit::Percent => parse_amount(input.strip_suffix('%').unwrap_or(input), false)?,
        Unit::Seconds => parse_duration(input)?,
        Unit::Hours => parse_duration(input)? / HOUR,
        Unit::Timestamp => {
            // "7d" and "-7d" both mean "7 days ago"; there is no future.
            let secs = parse_duration(input.strip_prefix('-').unwrap_or(input))?;
            return Ok(Some(format!("-{}", compact_duration(secs))));
        }
    };
    Ok(Some(number_token(value)))
}

/// A stored token as the text an editor field shows, so reopening a filter
/// shows what was typed rather than raw seconds. Absolute timestamps are
/// edited through a date picker and return their token unchanged.
pub fn format_input(unit: Unit, token: &str) -> String {
    let Some(value) = token_number(token) else {
        return relative_secs(token)
            .map(compact_spaced_duration)
            .unwrap_or_else(|| token.to_string());
    };
    match unit {
        Unit::Plain | Unit::Gil => group_thousands(value),
        Unit::Percent | Unit::Rate | Unit::Timestamp => number_token(value),
        Unit::Seconds => compact_spaced_duration(value),
        Unit::Hours => compact_spaced_duration(value * HOUR),
    }
}

/// How a bound reads on a chip. The UI supplies the words around it.
#[derive(Clone, Debug, PartialEq)]
pub enum ChipBound {
    Text(String),
    /// A relative timestamp: "within the last {0}".
    Within(String),
    /// An absolute time, `YYYY-MM-DD HH:MM UTC`. Chips render on the
    /// server too, so they cannot use the viewer's zone without a
    /// hydration mismatch; they say which zone they are in instead.
    At(String),
}

pub fn format_chip(unit: Unit, token: &str) -> ChipBound {
    if let Some(secs) = relative_secs(token) {
        return ChipBound::Within(short_duration(secs));
    }
    let Some(value) = token_number(token) else {
        return ChipBound::Text(token.to_string());
    };
    ChipBound::Text(match unit {
        Unit::Plain => {
            if value.abs() >= 10_000.0 {
                compact_amount(value)
            } else {
                group_thousands(value)
            }
        }
        Unit::Gil => compact_amount(value),
        Unit::Percent => format!("{}%", round_to(value, 1)),
        Unit::Rate => round_to(value, 2),
        Unit::Seconds => short_duration(value),
        Unit::Hours => short_duration(value * HOUR),
        Unit::Timestamp => return ChipBound::At(format!("{} UTC", utc_minute(value))),
    })
}

/// A bound as a number to compare against, resolving relative tokens.
pub fn resolve_token(token: &str, now: f64) -> Option<f64> {
    token_number(token).or_else(|| relative_secs(token).map(|secs| now - secs))
}

pub fn is_relative(token: &str) -> bool {
    relative_secs(token).is_some()
}

fn token_number(token: &str) -> Option<f64> {
    token.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

fn relative_secs(token: &str) -> Option<f64> {
    let rest = token.trim().strip_prefix('-')?;
    // A negative number is a value, not a relative token.
    if rest.parse::<f64>().is_ok() {
        return None;
    }
    parse_duration(rest).ok()
}

/// Digits with `,` `_` or space grouping, and optionally a `k`/`m`/`b`
/// multiplier. Commas always group: the site prints `1,200,000` in every
/// locale, so `1,5` is not one and a half.
fn parse_amount(input: &str, suffixes: bool) -> Result<f64, UnitError> {
    let cleaned: String = input
        .chars()
        .filter(|c| !matches!(c, ',' | '_' | ' ' | '\u{a0}'))
        .collect();
    let lower = cleaned.to_ascii_lowercase();
    let (digits, multiplier) = match lower.chars().last() {
        Some('k') if suffixes => (&lower[..lower.len() - 1], 1e3),
        Some('m') if suffixes => (&lower[..lower.len() - 1], 1e6),
        Some('b') if suffixes => (&lower[..lower.len() - 1], 1e9),
        _ => (lower.as_str(), 1.0),
    };
    // `parse::<f64>` would also take "inf", "nan" and "1e5".
    if digits.is_empty()
        || !digits
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
    {
        return Err(UnitError::Invalid);
    }
    digits
        .parse::<f64>()
        .ok()
        .map(|v| v * multiplier)
        .filter(|v| v.is_finite())
        .ok_or(UnitError::Invalid)
}

/// `2d 4h`, `90m`, `1w`, `1.5h`, `3 days`; a bare number is hours.
fn parse_duration(input: &str) -> Result<f64, UnitError> {
    let input = input.trim().to_ascii_lowercase();
    if let Ok(hours) = input.parse::<f64>() {
        return (hours.is_finite() && hours >= 0.0 && !input.starts_with(['-', '+']))
            .then_some(hours * HOUR)
            .ok_or(UnitError::Invalid);
    }
    let mut total = 0.0;
    let mut chars = input.chars().peekable();
    let mut any = false;
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if chars.peek().is_none() {
            break;
        }
        let mut number = String::new();
        while let Some(c) = chars.next_if(|c| c.is_ascii_digit() || *c == '.') {
            number.push(c);
        }
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let mut unit = String::new();
        while let Some(c) = chars.next_if(|c| c.is_ascii_alphabetic()) {
            unit.push(c);
        }
        let value = number.parse::<f64>().map_err(|_| UnitError::Invalid)?;
        let scale = match unit.as_str() {
            "w" | "wk" | "wks" | "week" | "weeks" => WEEK,
            "d" | "day" | "days" => DAY,
            "h" | "hr" | "hrs" | "hour" | "hours" => HOUR,
            "m" | "min" | "mins" | "minute" | "minutes" => MINUTE,
            "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
            _ => return Err(UnitError::Invalid),
        };
        total += value * scale;
        any = true;
    }
    if any && total.is_finite() {
        Ok(total)
    } else {
        Err(UnitError::Invalid)
    }
}

/// The shortest decimal that reads back as the same number.
fn number_token(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 9e15 {
        format!("{}", value as i64)
    } else {
        let text = format!("{value:.6}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn round_to(value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    }
}

fn group_thousands(value: f64) -> String {
    let text = number_token(value);
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text.as_str()),
    };
    let (int, frac) = rest.split_once('.').unwrap_or((rest, ""));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    if frac.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped}.{frac}")
    }
}

fn compact_amount(value: f64) -> String {
    let abs = value.abs();
    let (scaled, suffix) = if abs >= 1e9 {
        (value / 1e9, "B")
    } else if abs >= 1e6 {
        (value / 1e6, "M")
    } else if abs >= 1e3 {
        (value / 1e3, "k")
    } else {
        return group_thousands(value.round());
    };
    format!("{}{suffix}", round_to(scaled, 1))
}

fn duration_parts(secs: f64) -> Vec<(u64, char)> {
    let mut left = secs.max(0.0).round() as u64;
    let mut parts = Vec::new();
    for (size, label) in [(86_400, 'd'), (3_600, 'h'), (60, 'm'), (1, 's')] {
        if left >= size {
            parts.push((left / size, label));
            left %= size;
        }
    }
    parts
}

/// Exact: every unit, so an editor round-trip never loses time.
fn compact_spaced_duration(secs: f64) -> String {
    let parts = duration_parts(secs);
    if parts.is_empty() {
        return "0m".into();
    }
    parts
        .iter()
        .map(|(n, l)| format!("{n}{l}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn compact_duration(secs: f64) -> String {
    compact_spaced_duration(secs).replace(' ', "")
}

/// At most two units, for chips.
fn short_duration(secs: f64) -> String {
    let parts = duration_parts(secs);
    if parts.is_empty() {
        return "0m".into();
    }
    parts
        .iter()
        .take(2)
        .map(|(n, l)| format!("{n}{l}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `YYYY-MM-DD HH:MM` in UTC, matching the market columns' own display.
pub fn utc_minute(unix: f64) -> String {
    let secs = unix.floor() as i64;
    let days = secs.div_euclid(86_400);
    let of_day = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        of_day / 3_600,
        of_day % 3_600 / 60
    )
}

/// Howard Hinnant's days-to-civil, for dates without a time crate.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(unit: Unit, input: &str) -> Option<String> {
        parse_input(unit, input).unwrap()
    }

    #[test]
    fn amounts_accept_grouping_and_multipliers() {
        for (input, token) in [
            ("1.2m", "1200000"),
            ("1.2M", "1200000"),
            ("150,000", "150000"),
            ("150 000", "150000"),
            ("1_000", "1000"),
            ("25k", "25000"),
            ("2b", "2000000000"),
            ("-500", "-500"),
            ("0.5", "0.5"),
        ] {
            assert_eq!(parsed(Unit::Gil, input).as_deref(), Some(token), "{input}");
        }
        for input in ["abc", "1.2x", "inf", "NaN", "1e5", "k", "--1"] {
            assert_eq!(
                parse_input(Unit::Gil, input),
                Err(UnitError::Invalid),
                "{input}"
            );
        }
        assert_eq!(parsed(Unit::Gil, "   "), None);
    }

    #[test]
    fn percent_and_rate_take_plain_numbers() {
        assert_eq!(parsed(Unit::Percent, "15%").as_deref(), Some("15"));
        assert_eq!(parsed(Unit::Percent, "12.5").as_deref(), Some("12.5"));
        assert_eq!(parsed(Unit::Rate, "0.25").as_deref(), Some("0.25"));
        // Multipliers are for amounts only: "5m" is not a rate.
        assert!(parse_input(Unit::Rate, "5m").is_err());
        assert!(parse_input(Unit::Percent, "5k").is_err());
    }

    #[test]
    fn durations_combine_units_and_bare_numbers_are_hours() {
        for (input, secs) in [
            ("2d 4h", 187_200),
            ("2d4h", 187_200),
            ("90m", 5_400),
            ("1w", 604_800),
            ("1.5h", 5_400),
            ("3 days", 259_200),
            ("12", 43_200),
            ("30s", 30),
        ] {
            assert_eq!(
                parsed(Unit::Seconds, input).as_deref(),
                Some(secs.to_string().as_str()),
                "{input}"
            );
        }
        assert_eq!(parsed(Unit::Hours, "1d 12h").as_deref(), Some("36"));
        assert_eq!(parsed(Unit::Hours, "6").as_deref(), Some("6"));
        for input in ["abc", "2x", "-3h", "h", "2d abc"] {
            assert!(parse_input(Unit::Seconds, input).is_err(), "{input}");
        }
    }

    #[test]
    fn timestamps_store_relative_tokens_and_resolve_against_now() {
        assert_eq!(parsed(Unit::Timestamp, "7d").as_deref(), Some("-7d"));
        assert_eq!(parsed(Unit::Timestamp, "-36h").as_deref(), Some("-1d12h"));
        assert_eq!(parsed(Unit::Timestamp, "24").as_deref(), Some("-1d"));
        let now = 1_000_000.0;
        assert_eq!(resolve_token("-1d", now), Some(now - 86_400.0));
        assert_eq!(resolve_token("1758499200", now), Some(1_758_499_200.0));
        assert_eq!(resolve_token("-500", now), Some(-500.0));
        assert_eq!(resolve_token("soon", now), None);
        assert!(is_relative("-7d") && !is_relative("-7") && !is_relative("7d"));
    }

    #[test]
    fn inputs_round_trip_through_their_editor_text() {
        for (unit, input) in [
            (Unit::Gil, "1.2m"),
            (Unit::Gil, "-1,500"),
            (Unit::Percent, "15"),
            (Unit::Rate, "0.75"),
            (Unit::Seconds, "2d 4h 5m 6s"),
            (Unit::Hours, "1d 12h"),
            (Unit::Timestamp, "7d"),
        ] {
            let token = parsed(unit, input).unwrap();
            let shown = format_input(unit, &token);
            assert_eq!(
                parsed(unit, &shown),
                Some(token.clone()),
                "{input} → {shown}"
            );
        }
        assert_eq!(format_input(Unit::Gil, "1200000"), "1,200,000");
        assert_eq!(format_input(Unit::Seconds, "86400"), "1d");
        assert_eq!(format_input(Unit::Timestamp, "-7d"), "7d");
    }

    #[test]
    fn chips_are_compact_and_unit_aware() {
        let text = |unit, token| match format_chip(unit, token) {
            ChipBound::Text(t) => t,
            other => panic!("{other:?}"),
        };
        assert_eq!(text(Unit::Gil, "1200000"), "1.2M");
        assert_eq!(text(Unit::Gil, "999"), "999");
        assert_eq!(text(Unit::Gil, "25000"), "25k");
        assert_eq!(text(Unit::Plain, "1500"), "1,500");
        assert_eq!(text(Unit::Percent, "12.345"), "12.3%");
        assert_eq!(text(Unit::Rate, "0.126"), "0.13");
        assert_eq!(text(Unit::Seconds, "86400"), "1d");
        assert_eq!(text(Unit::Seconds, "129600"), "1d 12h");
        assert_eq!(text(Unit::Hours, "36"), "1d 12h");
        assert_eq!(
            format_chip(Unit::Timestamp, "-7d"),
            ChipBound::Within("7d".into())
        );
        assert_eq!(
            format_chip(Unit::Timestamp, "1756684800"),
            ChipBound::At("2025-09-01 00:00 UTC".into())
        );
    }

    #[test]
    fn utc_dates_match_known_instants() {
        assert_eq!(utc_minute(0.0), "1970-01-01 00:00");
        assert_eq!(utc_minute(951_782_400.0), "2000-02-29 00:00");
        assert_eq!(utc_minute(1_790_000_000.0), "2026-09-21 14:13");
    }
}
