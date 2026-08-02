use ultros_api_types::price_series::PriceBucket;

/// How far past the interquartile range the y-axis will still stretch to
/// follow real data, in multiples of the IQR.
///
/// Tukey's ordinary outlier fence is 1.5; gil prices are right-skewed enough
/// that 1.5 clips genuine rallies, so this uses his wider "far out" fence.
/// A laundered sale — the case that motivated this, see #1068 — sits orders
/// of magnitude above the market and is excluded by either.
const OUTLIER_FENCE_IQR: f64 = 3.0;

/// Fraction of the domain added as headroom above and below.
const DOMAIN_PAD: f64 = 0.05;

/// Volume-weighted average price; `None` on empty input or zero total quantity.
pub fn vwap(prices_and_quantities: &[(i32, i32)]) -> Option<i32> {
    let (num, den) =
        prices_and_quantities
            .iter()
            .fold((0i64, 0i64), |(n, d), (price, quantity)| {
                (
                    n + (*price as i64) * (*quantity as i64),
                    d + (*quantity as i64),
                )
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
    let n = sorted.len();
    if n % 2 == 1 {
        let (_, &mut val, _) = sorted.select_nth_unstable(n / 2);
        Some(val)
    } else {
        let (left, &mut right, _) = sorted.select_nth_unstable(n / 2);
        let left_max = *left.iter().max().unwrap();
        Some(((left_max as i64 + right as i64) / 2) as i32)
    }
}

/// Linearly interpolated quantile of an unsorted sample; `q` is clamped to
/// `0.0..=1.0`. `None` on empty input.
pub fn quantile(values: &[f64], q: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let position = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    Some(sorted[lower] + (sorted[upper] - sorted[lower]) * (position - lower as f64))
}

/// Padded y-axis domain for a price lane, chosen so a handful of extreme
/// buckets can't flatten the rest of the chart against the floor.
///
/// The naive domain is `min(low)..max(high)` over every drawn bucket. On a
/// market with one laundered sale that spans several orders of magnitude and
/// the entire real price history renders as a line along the bottom edge —
/// the second bullet of #1068.
///
/// Instead, take the interquartile range of the values the chart actually
/// *draws* (each bucket's p25/p50/p75 and its VWAP, which is what the Price
/// mode line plots) and let the axis follow the true `low`/`high` only as far
/// as [`OUTLIER_FENCE_IQR`] past that range. Pooling four values per bucket
/// keeps a single wild bucket a small minority of the sample, so it moves the
/// quantiles barely at all while its own extremes fall outside the fence.
///
/// Because the result is intersected with the real extent, a series with no
/// outliers gets exactly the domain it got before: the fence only ever pulls
/// the bounds inward. Callers must clip or drop marks that fall outside —
/// [`crate::scale::LinearScale`] extrapolates rather than clamping.
///
/// Known limitation: the fence needs the drawn values to have *some* spread
/// to measure against. A market whose ordinary buckets are all at one exact
/// price gives a zero IQR, and there the plain extent is kept — an outlier
/// alongside literally flat data still stretches the axis. Real gil prices
/// always wobble, so this has not been worth a second, scale-free heuristic.
///
/// `None` when the iterator yields no buckets.
pub fn robust_price_domain<'a>(
    buckets: impl IntoIterator<Item = &'a PriceBucket>,
) -> Option<(f64, f64)> {
    let mut drawn: Vec<f64> = Vec::new();
    let mut min_price = f64::INFINITY;
    let mut max_price = f64::NEG_INFINITY;
    for bucket in buckets {
        min_price = min_price.min(bucket.low as f64);
        max_price = max_price.max(bucket.high as f64);
        // A zeroed quantile means the payload predates them (or the bucket
        // recorded no sale prices); letting it vote would drag the fence to
        // the origin and defeat the whole exercise.
        if bucket.p50 > 0 {
            drawn.extend([bucket.p25 as f64, bucket.p50 as f64, bucket.p75 as f64]);
        }
        if let Some(vwap) = bucket.vwap() {
            drawn.push(vwap);
        }
    }
    if !min_price.is_finite() || !max_price.is_finite() {
        return None;
    }

    let (mut low, mut high) = (min_price, max_price);
    if let (Some(q1), Some(q3)) = (quantile(&drawn, 0.25), quantile(&drawn, 0.75))
        // A zero IQR means the sample carries no scale to measure an outlier
        // against — flat prices, a single bucket, or a payload with no
        // quantiles at all. Leave the plain extent alone rather than
        // collapsing the axis onto a point.
        && q3 > q1
    {
        let fence = (q3 - q1) * OUTLIER_FENCE_IQR;
        low = low.max(q1 - fence);
        high = high.min(q3 + fence);
        // Quantiles of prices always lie within low..high, so this can only
        // fire on a malformed payload. Fall back to the plain extent rather
        // than inverting the axis.
        if high < low {
            (low, high) = (min_price, max_price);
        }
    }

    let pad = ((high - low) * DOMAIN_PAD).max(1.0);
    Some(((low - pad).max(0.0), high + pad))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket(low: i32, high: i32, p25: i32, p50: i32, p75: i32) -> PriceBucket {
        PriceBucket {
            ts: chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc(),
            open: p50,
            high,
            low,
            close: p50,
            gil: i64::from(p50),
            units: 1,
            sales: 4,
            p25,
            p50,
            p75,
        }
    }

    /// A bucket whose every price is the same — the shape a single laundered
    /// sale produces.
    fn flat_bucket(price: i32) -> PriceBucket {
        bucket(price, price, price, price, price)
    }

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

    #[test]
    fn quantile_interpolates_between_neighbours() {
        let values = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(quantile(&values, 0.0), Some(1.0));
        assert_eq!(quantile(&values, 1.0), Some(4.0));
        assert_eq!(quantile(&values, 0.5), Some(2.5));
        // 0.25 * 3 = 0.75 of the way from 1.0 to 2.0.
        assert_eq!(quantile(&values, 0.25), Some(1.75));
    }

    #[test]
    fn quantile_sorts_its_input_and_clamps_q() {
        assert_eq!(quantile(&[9.0, 1.0, 5.0], 0.5), Some(5.0));
        assert_eq!(quantile(&[9.0, 1.0, 5.0], -2.0), Some(1.0));
        assert_eq!(quantile(&[9.0, 1.0, 5.0], 4.0), Some(9.0));
        assert_eq!(quantile(&[], 0.5), None);
    }

    #[test]
    fn robust_domain_matches_the_plain_extent_without_outliers() {
        // Prices walking 1000..1044 with a 20-wide bucket spread: nothing is
        // anywhere near three IQRs out, so the fence must not bite and the
        // domain must equal the old min(low)..max(high) plus 5% padding.
        let buckets: Vec<PriceBucket> = (0..20)
            .map(|i| {
                let mid = 1000 + i * 2;
                bucket(mid - 10, mid + 10, mid - 5, mid, mid + 5)
            })
            .collect();
        let (low, high) = robust_price_domain(&buckets).expect("non-empty");
        let pad = ((1048.0 - 990.0) * 0.05f64).max(1.0);
        assert!((low - (990.0 - pad)).abs() < 1e-6, "low was {low}");
        assert!((high - (1048.0 + pad)).abs() < 1e-6, "high was {high}");
    }

    #[test]
    fn robust_domain_excludes_a_laundered_sale() {
        // The #1068 case: 20 ordinary buckets plus one whose every price is
        // a thousand times the market.
        let mut buckets: Vec<PriceBucket> = (0..20)
            .map(|i| {
                let mid = 1000 + i * 2;
                bucket(mid - 10, mid + 10, mid - 5, mid, mid + 5)
            })
            .collect();
        buckets.push(flat_bucket(1_000_000));

        let (low, high) = robust_price_domain(&buckets).expect("non-empty");
        assert!(
            high < 2_000.0,
            "the laundered sale must not stretch the axis, high was {high}"
        );
        // …and the ordinary market must still fit comfortably inside it.
        assert!(low <= 990.0, "low was {low}");
        assert!(high >= 1_048.0, "high was {high}");
    }

    #[test]
    fn robust_domain_excludes_a_dumped_sale() {
        // Symmetric case: someone lists at 1 gil in an otherwise stable market.
        let mut buckets: Vec<PriceBucket> = (0..20)
            .map(|i| {
                let mid = 50_000 + i * 100;
                bucket(mid - 500, mid + 500, mid - 250, mid, mid + 250)
            })
            .collect();
        buckets.push(flat_bucket(1));

        let (low, high) = robust_price_domain(&buckets).expect("non-empty");
        assert!(low > 40_000.0, "the 1 gil sale dragged the floor to {low}");
        assert!(high >= 51_900.0, "high was {high}");
    }

    #[test]
    fn robust_domain_keeps_flat_prices_visible() {
        // Every price identical: the IQR is zero, so the fence collapses onto
        // the value and only the 1 gil padding floor keeps the axis open.
        let buckets = vec![flat_bucket(100); 5];
        assert_eq!(robust_price_domain(&buckets), Some((99.0, 101.0)));
    }

    #[test]
    fn robust_domain_falls_back_when_quantiles_are_missing() {
        // Zeroed quantiles (a pre-quantile payload) must not pin the axis to
        // the origin; the plain extent takes over.
        let buckets = vec![bucket(900, 1_100, 0, 0, 0)];
        let (low, high) = robust_price_domain(&buckets).expect("non-empty");
        assert!(low < 900.0 && high > 1_100.0, "got {low}..{high}");
    }

    #[test]
    fn robust_domain_is_none_without_buckets() {
        assert_eq!(robust_price_domain(&[]), None);
    }
}
