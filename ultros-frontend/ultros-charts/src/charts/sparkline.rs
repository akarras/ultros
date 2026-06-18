//! Geometry for the tiny inline sparklines (Market Movers, Continue
//! Tracking, Trends, Analyzer). Same visual rules as the old hand-rolled
//! component: 2px vertical inset, min→bottom / max→top scaling, gap
//! interpolation across quiet hours, single trend color.

use crate::data::interpolate::interpolate_gaps;
use crate::scene::Color;

#[derive(Clone, Debug, PartialEq)]
pub struct SparklineModel {
    pub width: f32,
    pub height: f32,
    /// Scaled polyline vertices; empty for an empty/all-zero series.
    pub points: Vec<(f32, f32)>,
    /// Interpolated values aligned with `points` (tooltip content).
    pub values: Vec<f32>,
    /// Trend color: emerald up, red down, slate flat.
    pub color: Color,
}

impl SparklineModel {
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Nearest sample index for pixel `x` (viewBox units).
    pub fn nearest_index(&self, x: f32) -> Option<usize> {
        if self.points.is_empty() {
            return None;
        }
        if self.points.len() == 1 {
            return Some(0);
        }
        let step = self.width / (self.points.len() as f32 - 1.0);
        let index = (x / step).round() as i64;
        Some(index.clamp(0, self.points.len() as i64 - 1) as usize)
    }
}

fn trend_color(pct_change: f32) -> Color {
    if pct_change > 0.0 {
        Color::hex("#34d399")
    } else if pct_change < 0.0 {
        Color::hex("#f87171")
    } else {
        Color::hex("#94a3b8")
    }
}

pub fn build_sparkline(raw: &[u32], pct_change: f32, width: f32, height: f32) -> SparklineModel {
    let filled = interpolate_gaps(raw);
    let color = trend_color(pct_change);
    if filled.iter().all(|&v| v == 0.0) {
        return SparklineModel {
            width,
            height,
            points: Vec::new(),
            values: Vec::new(),
            color,
        };
    }
    let (min, max) = filled
        .iter()
        .copied()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v), hi.max(v))
        });
    let span = (max - min).max(1.0);
    let inset = 2.0;
    let usable_h = height - inset * 2.0;
    let n = filled.len();
    let step = if n > 1 { width / (n as f32 - 1.0) } else { 0.0 };
    let points = filled
        .iter()
        .enumerate()
        .map(|(i, &v)| (i as f32 * step, inset + (1.0 - (v - min) / span) * usable_h))
        .collect();
    SparklineModel {
        width,
        height,
        points,
        values: filled,
        color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_min_to_bottom_and_max_to_top_inset() {
        let m = build_sparkline(&[100, 200], 1.0, 80.0, 24.0);
        assert_eq!(m.points.len(), 2);
        assert_eq!(m.points[0], (0.0, 22.0)); // min → height - inset
        assert_eq!(m.points[1], (80.0, 2.0)); // max → inset
        assert_eq!(m.values, vec![100.0, 200.0]);
    }

    #[test]
    fn all_zero_series_is_empty() {
        let m = build_sparkline(&[0, 0, 0], 0.0, 80.0, 24.0);
        assert!(m.is_empty());
        let m = build_sparkline(&[], 0.0, 80.0, 24.0);
        assert!(m.is_empty());
    }

    #[test]
    fn trend_color_follows_pct_change() {
        use crate::scene::Color;
        assert_eq!(
            build_sparkline(&[1, 2], 5.0, 80.0, 24.0).color,
            Color::hex("#34d399")
        );
        assert_eq!(
            build_sparkline(&[1, 2], -5.0, 80.0, 24.0).color,
            Color::hex("#f87171")
        );
        assert_eq!(
            build_sparkline(&[1, 2], 0.0, 80.0, 24.0).color,
            Color::hex("#94a3b8")
        );
    }

    #[test]
    fn nearest_index_snaps_and_clamps() {
        let m = build_sparkline(&[10, 20, 30], 0.0, 80.0, 24.0); // step = 40
        assert_eq!(m.nearest_index(-10.0), Some(0));
        assert_eq!(m.nearest_index(15.0), Some(0));
        assert_eq!(m.nearest_index(25.0), Some(1));
        assert_eq!(m.nearest_index(999.0), Some(2));
        assert_eq!(
            build_sparkline(&[], 0.0, 80.0, 24.0).nearest_index(10.0),
            None
        );
    }
}
