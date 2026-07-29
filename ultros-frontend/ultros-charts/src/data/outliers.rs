//! IQR-based outlier filtering — the same rule the old plotters chart and
//! the web UI used, consolidated here.

use std::borrow::Cow;

use itertools::Itertools;
use ultros_api_types::SaleHistory;

/// Outlier bounds: `(Q1 - 2.5*IQR, Q3 + 2.5*IQR)`. Returns `None` for
/// samples smaller than 10 — too little data to call anything an outlier.
pub fn iqr_bounds(sales: &[SaleHistory]) -> Option<(i32, i32)> {
    if sales.len() < 10 {
        return None;
    }
    let prices = sales
        .iter()
        .map(|s| s.price_per_item)
        .sorted()
        .collect::<Vec<_>>();
    let q1_index = prices.len() / 4;
    let q3_index = prices.len() - q1_index;
    let q1 = *prices.get(q1_index)?;
    let q3 = *prices.get(q3_index)?;
    let widened = ((q3 - q1) as f32 * 2.5) as i32;

    // Sentry: If IQR is 0, we shouldn't consider minor variations as outliers.
    // If all (or most) prices are exactly the same, no data should be filtered out.
    if widened == 0 {
        return None;
    }

    Some((q1 - widened, q3 + widened))
}

pub fn filter_outliers(sales: &[SaleHistory]) -> Cow<'_, [SaleHistory]> {
    match iqr_bounds(sales) {
        Some((min, max)) => Cow::Owned(
            sales
                .iter()
                .filter(|s| (min..=max).contains(&s.price_per_item))
                .cloned()
                .collect(),
        ),
        None => Cow::Borrowed(sales),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{sale, ts};

    #[test]
    fn small_samples_are_not_filtered() {
        let sales: Vec<_> = (0..5).map(|i| sale(100 + i, 1, 1, ts(0))).collect();
        assert!(iqr_bounds(&sales).is_none());
        assert_eq!(filter_outliers(&sales).len(), 5);
    }

    #[test]
    fn extreme_prices_are_filtered() {
        let mut sales: Vec<_> = (0..20)
            .map(|i| sale(1000 + i, 1, 1, ts(i as i64)))
            .collect();
        sales.push(sale(1_000_000, 1, 1, ts(21)));
        let filtered = filter_outliers(&sales);
        assert_eq!(filtered.len(), 20);
        assert!(filtered.iter().all(|s| s.price_per_item < 10_000));
    }

    #[test]
    fn zero_iqr_retains_minor_variations() {
        let mut sales: Vec<_> = (0..20).map(|i| sale(100, 1, 1, ts(i as i64))).collect();
        sales.push(sale(101, 1, 1, ts(21)));
        sales.push(sale(99, 1, 1, ts(22)));
        let filtered = filter_outliers(&sales);
        // If IQR is 0, nothing should be an outlier. It should keep all 22 sales.
        assert_eq!(filtered.len(), 22);
    }
}
