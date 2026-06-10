/// Least-squares fit over `(x, y)` points. Returns `(slope, intercept)`,
/// or `None` with fewer than 2 points or zero x-variance.
pub fn least_squares(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    if points.len() < 2 {
        return None;
    }
    let n = points.len() as f64;
    let mean_x = points.iter().map(|(x, _)| x).sum::<f64>() / n;
    let mean_y = points.iter().map(|(_, y)| y).sum::<f64>() / n;
    let mut covariance = 0.0;
    let mut variance_x = 0.0;
    for (x, y) in points {
        let dx = x - mean_x;
        covariance += dx * (y - mean_y);
        variance_x += dx * dx;
    }
    if variance_x == 0.0 {
        return None;
    }
    let slope = covariance / variance_x;
    Some((slope, mean_y - slope * mean_x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_a_perfect_line() {
        let points: Vec<_> = (0..10).map(|i| (i as f64, 3.0 + 2.0 * i as f64)).collect();
        let (slope, intercept) = least_squares(&points).unwrap();
        assert!((slope - 2.0).abs() < 1e-9);
        assert!((intercept - 3.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_degenerate_input() {
        assert!(least_squares(&[]).is_none());
        assert!(least_squares(&[(1.0, 1.0)]).is_none());
        assert!(least_squares(&[(1.0, 1.0), (1.0, 2.0)]).is_none());
    }
}
