/// Volume-weighted average price; `None` on empty input or zero total quantity.
pub fn vwap(prices_and_quantities: &[(i32, i32)]) -> Option<i32> {
    let (num, den) = prices_and_quantities
        .iter()
        .fold((0i64, 0i64), |(n, d), (price, quantity)| {
            (n + (*price as i64) * (*quantity as i64), d + (*quantity as i64))
        });
    if den == 0 {
        return None;
    }
    Some((num / den) as i32)
}

/// Median price; integer mean of the middle two for even counts.
pub fn median(prices: &[i32]) -> Option<i32> {
    if prices.is_empty() {
        return None;
    }
    let mut sorted: Vec<i32> = prices.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    if n % 2 == 1 {
        Some(sorted[n / 2])
    } else {
        Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vwap_weights_by_quantity() {
        assert_eq!(vwap(&[(100, 1), (200, 3)]), Some(175));
        assert_eq!(vwap(&[]), None);
        assert_eq!(vwap(&[(100, 0)]), None);
    }

    #[test]
    fn median_handles_even_and_odd() {
        assert_eq!(median(&[3, 1, 2]), Some(2));
        assert_eq!(median(&[4, 1, 2, 3]), Some(2));
        assert_eq!(median(&[]), None);
    }
}
