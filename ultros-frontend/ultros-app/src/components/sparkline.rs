//! Inline SVG sparkline for the Market Movers list and other surfaces that
//! want a 24h price trace next to each row.
//!
//! Design rules (informed by the dashboard mockup):
//!   - Tiny: ~80×24 px so it fits comfortably in a table row.
//!   - Color by net trend, not per-segment. Polyline gradient is overkill
//!     for the visual signal we want; a single green/red trace reads
//!     instantly without distracting from the row's text.
//!   - Skip axes, labels, tooltips. At this size they're glyphs, not
//!     charts — the page already has the price and pct change as text.

use leptos::prelude::*;

/// Render an SVG polyline from the given points.
///
/// `points` is the raw VWAP series (frontend gets `Vec<u32>`). We rescale
/// to fit the viewport, mapping the minimum value to the bottom edge and
/// the maximum to the top edge. Zeros (gaps with no trade) are treated
/// as bridging — we interpolate between the surrounding non-zero values
/// rather than dropping to baseline. That keeps the trend line honest
/// instead of showing a misleading dive every time a quiet hour appears.
///
/// `pct_change` is the up/down indicator that drives the stroke color:
/// positive = emerald, negative = red, zero/none = muted neutral.
#[component]
pub fn Sparkline(
    /// VWAP series, oldest first. Length 8-48 typical; we don't impose a
    /// hard cap. Zeros mean "no trade in this hour" and are interpolated
    /// across.
    points: Vec<u32>,
    /// Drives stroke color. Pass the API's `pct_change_24h`.
    #[prop(default = 0.0)]
    pct_change: f32,
    /// Pixel width of the rendered sparkline. Default 80.
    #[prop(default = 80)]
    width: u32,
    /// Pixel height. Default 24.
    #[prop(default = 24)]
    height: u32,
) -> impl IntoView {
    // Fill in gaps so the polyline doesn't dive to zero on quiet hours.
    let filled = interpolate_gaps(&points);

    // Empty / all-zero series → render nothing rather than a flat line at
    // the bottom. The page typically shows the price as text anyway.
    if filled.iter().all(|&v| v == 0.0) {
        return view! { <span class="inline-block w-20 h-6" /> }.into_any();
    }

    let (min, max) = filled
        .iter()
        .copied()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v), hi.max(v))
        });
    let span = (max - min).max(1.0);

    // 2px inset on top/bottom so the stroke isn't clipped at the edges.
    let inset = 2.0;
    let usable_h = height as f32 - inset * 2.0;
    let n = filled.len();
    let step = if n > 1 {
        width as f32 / (n as f32 - 1.0)
    } else {
        0.0
    };

    let path: String = filled
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = i as f32 * step;
            // Higher value → smaller y (SVG coord origin top-left).
            let y = inset + (1.0 - (v - min) / span) * usable_h;
            format!("{x:.1},{y:.1}")
        })
        .collect::<Vec<_>>()
        .join(" ");

    let stroke = if pct_change > 0.0 {
        // emerald-400
        "#34d399"
    } else if pct_change < 0.0 {
        // red-400
        "#f87171"
    } else {
        // var(--color-text-muted) fallback; SVG can't take CSS var directly
        // in `stroke=`, so we use currentColor and let the parent CSS
        // class set color. Caller-side we don't bother — neutral grey
        // looks fine.
        "#94a3b8"
    };

    view! {
        <svg
            width=width
            height=height
            viewBox=format!("0 0 {width} {height}")
            class="inline-block align-middle"
            aria-hidden="true"
        >
            <polyline
                fill="none"
                stroke=stroke
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
                points=path
            />
        </svg>
    }
    .into_any()
}

/// Replace each zero in the series with a linear interpolation between
/// the surrounding non-zero values. Leading/trailing zeros are clamped to
/// the nearest non-zero. If the whole series is zero, returns zeros.
fn interpolate_gaps(raw: &[u32]) -> Vec<f32> {
    let n = raw.len();
    let mut out: Vec<f32> = raw.iter().map(|&v| v as f32).collect();

    // Pass 1: anchor positions of non-zero values.
    let positions: Vec<usize> = raw
        .iter()
        .enumerate()
        .filter_map(|(i, &v)| if v > 0 { Some(i) } else { None })
        .collect();

    if positions.is_empty() {
        return out;
    }

    // Leading zeros: pin to first non-zero.
    let first = positions[0];
    for o in out.iter_mut().take(first) {
        *o = raw[first] as f32;
    }
    // Trailing zeros: pin to last non-zero.
    let last = *positions.last().unwrap();
    for o in out.iter_mut().take(n).skip(last + 1) {
        *o = raw[last] as f32;
    }

    // Internal gaps: linear interp between consecutive anchors.
    for w in positions.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b - a <= 1 {
            continue;
        }
        let va = raw[a] as f32;
        let vb = raw[b] as f32;
        let gap = (b - a) as f32;
        for k in 1..(b - a) {
            let t = k as f32 / gap;
            out[a + k] = va + (vb - va) * t;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolate_fills_leading_and_trailing_zeros() {
        let out = interpolate_gaps(&[0, 0, 10, 20, 0, 0]);
        assert_eq!(out, vec![10.0, 10.0, 10.0, 20.0, 20.0, 20.0]);
    }

    #[test]
    fn interpolate_bridges_internal_gap_linearly() {
        // 100 at idx 0, 200 at idx 4 → 125, 150, 175 between
        let out = interpolate_gaps(&[100, 0, 0, 0, 200]);
        assert_eq!(out, vec![100.0, 125.0, 150.0, 175.0, 200.0]);
    }

    #[test]
    fn interpolate_all_zeros_stays_zero() {
        let out = interpolate_gaps(&[0, 0, 0]);
        assert_eq!(out, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn interpolate_passthrough_when_no_gaps() {
        let out = interpolate_gaps(&[10, 20, 30]);
        assert_eq!(out, vec![10.0, 20.0, 30.0]);
    }
}
