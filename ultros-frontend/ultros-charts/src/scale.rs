//! Numeric and time axes: domain→pixel mapping, "nice" tick generation,
//! and the K/mil number formatting shared with the web UI.

use chrono::{NaiveDateTime, TimeDelta};

/// Format an integer gil value as `1.50K` / `2.30mil`, matching the web UI.
pub fn short_number(value: i32) -> String {
    match value {
        1_000_000.. => format!("{:.2}mil", value as f32 / 1_000_000.0),
        1_000..=999_999 => format!("{:.2}K", value as f32 / 1_000.0),
        _ => value.to_string(),
    }
}

/// Maps a numeric domain onto a pixel range. The range may be inverted
/// (`range.0 > range.1`) — SVG y grows downward, so price scales pass
/// `(bottom, top)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearScale {
    domain: (f64, f64),
    range: (f32, f32),
}

impl LinearScale {
    pub fn new(domain: (f64, f64), range: (f32, f32)) -> Self {
        // Degenerate domains (single price) get widened so scale() stays finite.
        let domain = if domain.0 == domain.1 {
            (domain.0 - 1.0, domain.1 + 1.0)
        } else {
            domain
        };
        Self { domain, range }
    }

    pub fn scale(&self, v: f64) -> f32 {
        let t = (v - self.domain.0) / (self.domain.1 - self.domain.0);
        self.range.0 + t as f32 * (self.range.1 - self.range.0)
    }

    /// Tick values at "nice" 1/2/5×10ⁿ steps, clamped inside the domain.
    pub fn ticks(&self, target: usize) -> Vec<f64> {
        let span = self.domain.1 - self.domain.0;
        if span <= 0.0 || target == 0 {
            return Vec::new();
        }
        let raw_step = span / target as f64;
        let magnitude = 10f64.powf(raw_step.log10().floor());
        let normalized = raw_step / magnitude;
        let step = magnitude
            * if normalized <= 1.0 {
                1.0
            } else if normalized <= 2.0 {
                2.0
            } else if normalized <= 5.0 {
                5.0
            } else {
                10.0
            };
        let mut v = (self.domain.0 / step).ceil() * step;
        let mut out = Vec::new();
        while v <= self.domain.1 + step * 1e-9 {
            out.push(v);
            v += step;
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimeTick {
    pub ts: NaiveDateTime,
    pub label: String,
}

/// Maps naive-UTC timestamps onto a pixel range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeScale {
    start: i64,
    end: i64,
    range: (f32, f32),
}

const MINUTE: i64 = 60;
const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;

/// Candidate tick steps, smallest first.
const TIME_STEPS: [i64; 16] = [
    MINUTE,
    5 * MINUTE,
    15 * MINUTE,
    30 * MINUTE,
    HOUR,
    3 * HOUR,
    6 * HOUR,
    12 * HOUR,
    DAY,
    2 * DAY,
    3 * DAY,
    7 * DAY,
    14 * DAY,
    30 * DAY,
    90 * DAY,
    365 * DAY,
];

impl TimeScale {
    pub fn new(start: NaiveDateTime, end: NaiveDateTime, range: (f32, f32)) -> Self {
        let (mut start, mut end) = (start.and_utc().timestamp(), end.and_utc().timestamp());
        if start == end {
            // Single-instant data: widen ±30 min, same as the old plotters chart.
            start -= 30 * MINUTE;
            end += 30 * MINUTE;
        }
        Self { start, end, range }
    }

    pub fn scale(&self, ts: NaiveDateTime) -> f32 {
        let t = (ts.and_utc().timestamp() - self.start) as f64 / (self.end - self.start) as f64;
        self.range.0 + t as f32 * (self.range.1 - self.range.0)
    }

    /// At most `target` ticks aligned to step boundaries. `label_offset_minutes`
    /// shifts the LABEL text only (viewer-local display); tick positions stay
    /// UTC-aligned so SSR and client geometry agree.
    pub fn ticks(&self, target: usize, label_offset_minutes: i32) -> Vec<TimeTick> {
        let span = self.end - self.start;
        let step = TIME_STEPS
            .iter()
            .copied()
            .find(|step| span / step <= target as i64)
            .unwrap_or(365 * DAY);
        let format = if step < HOUR {
            "%H:%M"
        } else if step < DAY {
            "%m-%d %H:%M"
        } else if step < 30 * DAY {
            "%m-%d"
        } else {
            "%Y-%m"
        };
        let mut tick = self.start.div_euclid(step) * step;
        if tick < self.start {
            tick += step;
        }
        let mut out = Vec::new();
        while tick <= self.end {
            if let Some(ts) = chrono::DateTime::from_timestamp(tick, 0) {
                let ts = ts.naive_utc();
                let display = ts + TimeDelta::minutes(label_offset_minutes as i64);
                out.push(TimeTick {
                    ts,
                    label: display.format(format).to_string(),
                });
            }
            tick += step;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn ts(secs: i64) -> NaiveDateTime {
        chrono::DateTime::from_timestamp(secs, 0)
            .unwrap()
            .naive_utc()
    }

    #[test]
    fn short_number_formats_like_the_web_ui() {
        assert_eq!(short_number(0), "0");
        assert_eq!(short_number(999), "999");
        assert_eq!(short_number(1000), "1.00K");
        assert_eq!(short_number(1500), "1.50K");
        assert_eq!(short_number(999999), "1000.00K");
        assert_eq!(short_number(1000000), "1.00mil");
        assert_eq!(short_number(1500000), "1.50mil");
    }

    #[test]
    fn linear_scale_maps_and_inverts_range() {
        let s = LinearScale::new((0.0, 100.0), (200.0, 0.0));
        assert_eq!(s.scale(0.0), 200.0);
        assert_eq!(s.scale(100.0), 0.0);
        assert_eq!(s.scale(50.0), 100.0);
    }

    #[test]
    fn linear_ticks_are_nice() {
        let s = LinearScale::new((0.0, 1000.0), (0.0, 1.0));
        assert_eq!(s.ticks(5), vec![0.0, 200.0, 400.0, 600.0, 800.0, 1000.0]);
    }

    #[test]
    fn degenerate_domain_widens() {
        let s = LinearScale::new((5.0, 5.0), (0.0, 10.0));
        assert_eq!(s.scale(5.0), 5.0);
    }

    #[test]
    fn time_ticks_pick_sensible_steps() {
        let scale = TimeScale::new(
            ts(1_700_000_000),
            ts(1_700_000_000 + 2 * 3600),
            (0.0, 100.0),
        );
        let ticks = scale.ticks(6, 0);
        assert!(!ticks.is_empty() && ticks.len() <= 6);
        // Sub-day spans label as %H:%M
        assert!(ticks[0].label.contains(':'));
    }

    #[test]
    fn equal_timestamps_widen_30_minutes() {
        let t = ts(1_700_000_000);
        let scale = TimeScale::new(t, t, (0.0, 100.0));
        assert_eq!(scale.scale(t), 50.0);
    }

    #[test]
    fn linear_ticks_align_for_offset_domains() {
        let s = LinearScale::new((150.0, 950.0), (0.0, 1.0));
        assert_eq!(s.ticks(5), vec![200.0, 400.0, 600.0, 800.0]);
    }

    #[test]
    fn time_ticks_use_day_steps_for_month_spans() {
        let start = ts(1_700_000_000);
        let end = ts(1_700_000_000 + 30 * 86_400);
        let ticks = TimeScale::new(start, end, (0.0, 100.0)).ticks(6, 0);
        assert!(!ticks.is_empty() && ticks.len() <= 7);
        // Day-scale steps label as %m-%d: no time-of-day component
        assert!(ticks.iter().all(|t| !t.label.contains(':')));
    }

    #[test]
    fn tick_labels_shift_with_offset_but_positions_do_not() {
        let scale = TimeScale::new(
            ts(1_700_000_000),
            ts(1_700_000_000 + 2 * 3600),
            (0.0, 100.0),
        );
        let utc = scale.ticks(6, 0);
        let shifted = scale.ticks(6, 60);
        assert_eq!(utc.len(), shifted.len());
        assert_eq!(utc[0].ts, shifted[0].ts);
        assert_ne!(utc[0].label, shifted[0].label);
    }
}
