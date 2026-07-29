/// Replace each zero in the series with a linear interpolation between
/// the surrounding non-zero values. Leading/trailing zeros are clamped to
/// the nearest non-zero. If the whole series is zero, returns zeros.
pub fn interpolate_gaps(raw: &[u32]) -> Vec<f32> {
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
