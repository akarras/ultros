//! Hour-of-day strips for the listing-hours and undercut-hours columns: 24
//! cells in the viewer's time zone, shaded against the row's own busiest
//! hour so the strip shows the item's daily rhythm rather than its volume.
//! See docs/superpowers/specs/2026-09-24-listing-hours-design.md.

use leptos::prelude::*;

/// Below this share of pinned new listings a listing-hours strip mostly maps
/// when players look at the board, so it is drawn faded.
pub const PINNED_FLOOR: f64 = 0.5;

/// Shade steps above zero; the busiest hour takes the last.
const LEVELS: u32 = 7;

/// The viewer's offset from UTC, rounded to whole hours (a half-hour zone
/// lands on the nearer hour). Zero outside the browser; the columns only
/// render from a client-side fetch, so SSR never draws a strip.
pub fn local_offset_hours() -> i64 {
    #[cfg(feature = "hydrate")]
    let minutes = -(js_sys::Date::new_0().get_timezone_offset() as i64);
    #[cfg(not(feature = "hydrate"))]
    let minutes = 0_i64;
    (minutes as f64 / 60.0).round() as i64
}

/// UTC hour-of-day counts rotated so index 0 is local midnight.
pub fn to_local(utc: &[u32; 24], offset_hours: i64) -> [u32; 24] {
    let mut local = [0; 24];
    for (hour, n) in utc.iter().enumerate() {
        local[(hour as i64 + offset_hours).rem_euclid(24) as usize] = *n;
    }
    local
}

/// The busiest hour, earliest on a tie. `None` when nothing was counted.
pub fn peak_hour(hours: &[u32; 24]) -> Option<usize> {
    let max = *hours.iter().max()?;
    (max > 0).then(|| hours.iter().position(|n| *n == max).unwrap())
}

/// `0` for an empty hour, else `1..=LEVELS` relative to the busiest hour.
fn level(n: u32, max: u32) -> u32 {
    if n == 0 || max == 0 {
        0
    } else {
        (u64::from(n) * u64::from(LEVELS)).div_ceil(u64::from(max)) as u32
    }
}

pub fn hour_label(hour: usize) -> String {
    format!("{hour:02}:00")
}

#[component]
pub fn HourStrip(
    /// Counts by local hour, index 0 = midnight.
    hours: [u32; 24],
    /// Drawn at reduced strength when the hour placement is unreliable.
    #[prop(optional)]
    faded: bool,
    /// Hover and screen-reader text.
    label: String,
) -> impl IntoView {
    let max = hours.iter().copied().max().unwrap_or(0);
    let cells = hours
        .iter()
        .map(|n| {
            let style = match level(*n, max) {
                0 => "background: var(--color-outline)".to_string(),
                l => format!(
                    "background: color-mix(in srgb, var(--accent) {}%, transparent)",
                    16 + l * 12
                ),
            };
            view! { <span class="block h-3.5 rounded-[1px]" style=style /> }
        })
        .collect_view();
    view! {
        <div
            class="grid gap-px"
            class:opacity-40=faded
            style="grid-template-columns: repeat(24, 5px)"
            role="img"
            aria-label=label.clone()
            title=label
        >
            {cells}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_moves_utc_hours_into_the_viewer_s_day() {
        let mut utc = [0; 24];
        utc[3] = 5;
        // UTC-7: 03:00 UTC is 20:00 local.
        assert_eq!(to_local(&utc, -7)[20], 5);
        // UTC+9: 03:00 UTC is 12:00 local.
        assert_eq!(to_local(&utc, 9)[12], 5);
        assert_eq!(to_local(&utc, 0), utc);
    }

    #[test]
    fn peak_is_the_earliest_busiest_hour_and_none_when_empty() {
        assert_eq!(peak_hour(&[0; 24]), None);
        let mut hours = [1; 24];
        hours[9] = 4;
        hours[21] = 4;
        assert_eq!(peak_hour(&hours), Some(9));
    }

    #[test]
    fn levels_scale_to_the_row_s_busiest_hour() {
        assert_eq!(level(0, 10), 0);
        assert_eq!(level(10, 10), LEVELS);
        assert_eq!(level(1, 1000), 1, "any activity is visible");
        assert_eq!(level(5, 10), 4);
    }
}
