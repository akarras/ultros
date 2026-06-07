use crate::math::filter_outliers_iqr_in_place;
use chrono::{Duration, Utc};
use ultros_api_types::recent_sales::SaleData;

#[derive(Clone, Copy, Debug)]
pub struct SalesStats {
    pub daily_sales: f32,
    pub avg_price: i32,
    pub total_sales: usize,
}

/// Summary stats for a single (item_id, hq) bucket of recent sales. Shared by the analyzer
/// and vendor-resale tables.
#[derive(Hash, Clone, Debug, PartialEq)]
pub struct SaleSummary {
    pub item_id: i32,
    pub hq: bool,
    /// Number of sales considered; bounded by the API's recent-sales window.
    pub num_sold: usize,
    /// Average time between sales across `num_sold`. None if no sales.
    pub avg_sale_duration: Option<Duration>,
    /// Time since the most-recent sale. None if no sales.
    pub days_since_last_sale: Option<Duration>,
    pub max_price: i32,
    pub avg_price: i32,
    /// Robust midpoint of the clamped sales, used as the realistic seller estimate.
    pub median_price: i32,
    pub min_price: i32,
}

/// Renders a duration as a compact "Xd Yh" / "Xh Ym" / "Xm Ys" string (up to two units).
/// Used by analyzer tables for the avg-sale-duration column.
pub fn format_duration_short(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    let seconds = secs % 60;
    let mut parts: Vec<String> = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 && parts.len() < 2 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 && parts.len() < 2 {
        parts.push(format!("{}s", seconds));
    }
    if parts.is_empty() {
        "0s".to_string()
    } else {
        parts[..parts.len().min(2)].join(" ")
    }
}

/// Tailwind class string for the ROI badge in analyzer tables. Tints the badge with the
/// brand-ring color, proportional to ROI %.
pub fn roi_badge_class(roi: i32) -> String {
    let tint = if roi >= 500 {
        "24%"
    } else if roi >= 200 {
        "20%"
    } else if roi >= 100 {
        "16%"
    } else if roi >= 50 {
        "12%"
    } else {
        "10%"
    };
    format!(
        "inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_{tint},transparent)]"
    )
}

pub fn analyze_sales(sales_data: &[&SaleData], filter_outliers: bool) -> SalesStats {
    let now = Utc::now().naive_utc();
    let mut total_sales = 0;
    let mut total_price: i64 = 0;
    let mut oldest_date = now;

    let mut prices = Vec::new();

    for data in sales_data {
        for sale in &data.sales {
            total_sales += 1;
            total_price += sale.price_per_unit as i64;
            if sale.sale_date < oldest_date {
                oldest_date = sale.sale_date;
            }
            if filter_outliers {
                prices.push(sale.price_per_unit);
            }
        }
    }

    if total_sales == 0 {
        return SalesStats {
            daily_sales: 0.0,
            avg_price: 0,
            total_sales: 0,
        };
    }

    let avg_price = if filter_outliers {
        let filtered = filter_outliers_iqr_in_place(&mut prices);
        if filtered.is_empty() {
            0
        } else {
            (filtered.iter().map(|&p| p as i64).sum::<i64>() / filtered.len() as i64) as i32
        }
    } else {
        (total_price / total_sales as i64) as i32
    };

    let duration_millis = (now - oldest_date).num_milliseconds().abs();
    // Clamp to at least 1 hour to prevent huge numbers for very recent single sales
    let duration_hours = (duration_millis as f64 / 1000.0 / 3600.0).max(1.0);
    let days_in_sample = duration_hours / 24.0;

    // If we only have 1 sale, and it was recent, daily_sales might be huge if we strictly divide by duration.
    // But logically, if it sold once in the last hour, that is a rate of 24/day *observed*.
    // We will present it as is, but maybe the UI can clarify "based on 1 sale".
    let daily_sales = total_sales as f32 / days_in_sample as f32;

    SalesStats {
        daily_sales,
        avg_price,
        total_sales,
    }
}

/// One quality's robust price estimate plus the sample accounting behind it.
// Consumed by the item-view Real Price card in a later commit; allow dead_code until then.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealPriceEstimate {
    /// The launder-resistant price.
    pub value: i32,
    /// Number of sales the value was computed from.
    pub used: usize,
    /// Total sales for this quality before any filtering.
    pub total: usize,
    /// `total - used`: sales dropped by the vendor guard and/or IQR filter.
    pub excluded: usize,
}

/// NQ and HQ estimates, computed independently (never blended).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RealPriceBreakdown {
    pub nq: Option<RealPriceEstimate>,
    pub hq: Option<RealPriceEstimate>,
}

#[allow(dead_code)]
impl RealPriceBreakdown {
    /// Headline quality = whichever has more sales; NQ wins an exact tie.
    pub fn primary(&self) -> Option<(bool, RealPriceEstimate)> {
        match (self.nq, self.hq) {
            (Some(nq), Some(hq)) => {
                if hq.total > nq.total {
                    Some((true, hq))
                } else {
                    Some((false, nq))
                }
            }
            (Some(nq), None) => Some((false, nq)),
            (None, Some(hq)) => Some((true, hq)),
            (None, None) => None,
        }
    }

    /// The non-headline quality, shown only when it has >= 4 sales.
    pub fn secondary(&self) -> Option<(bool, RealPriceEstimate)> {
        let primary_is_hq = self.primary()?.0;
        let (is_hq, candidate) = if primary_is_hq {
            (false, self.nq)
        } else {
            (true, self.hq)
        };
        candidate.filter(|e| e.total >= 4).map(|e| (is_hq, e))
    }
}

/// Median of a slice, sorting it in place. Uses the upper-middle element for even
/// lengths, matching the upper-middle pick used by `item_view` / `sale_history_table`.
/// Caller guarantees non-empty.
#[allow(dead_code)]
fn median_in_place(prices: &mut [i32]) -> i32 {
    prices.sort_unstable();
    prices[prices.len() / 2]
}

/// Robust price for a single quality from `(price, qty)` samples.
/// Vendor guard (drop qty==1 sales priced > 100x vendor), then IQR-filtered mean,
/// with a median fallback for fewer than 4 surviving samples.
#[allow(dead_code)]
fn estimate_quality(
    samples: &[(i32, i32)],
    vendor_price: Option<i32>,
) -> Option<RealPriceEstimate> {
    let total = samples.len();
    if total == 0 {
        return None;
    }

    let vendor_cap = vendor_price.filter(|v| *v > 0).map(|v| v as i64 * 100);
    let mut prices: Vec<i32> = samples
        .iter()
        .filter(|&&(price, qty)| match vendor_cap {
            Some(cap) => !(qty == 1 && price as i64 > cap),
            None => true,
        })
        .map(|&(price, _)| price)
        .collect();

    // If the guard removed everything, fall back to the median of all raw prices so we
    // still show something rather than "No data".
    if prices.is_empty() {
        let mut all: Vec<i32> = samples.iter().map(|&(p, _)| p).collect();
        let used = all.len();
        let value = median_in_place(&mut all);
        return Some(RealPriceEstimate {
            value,
            used,
            total,
            excluded: total - used,
        });
    }

    let (value, used) = if prices.len() < 4 {
        let used = prices.len();
        (median_in_place(&mut prices), used)
    } else {
        let filtered = filter_outliers_iqr_in_place(&mut prices);
        let used = filtered.len();
        let mean = (filtered.iter().map(|&p| p as i64).sum::<i64>() / used as i64) as i32;
        (mean, used)
    };

    Some(RealPriceEstimate {
        value,
        used,
        total,
        excluded: total - used,
    })
}

/// Compute the launder-resistant Real Price from the item page's recent sales.
///
/// `samples`: `(price_per_item, quantity, hq)` for each recent sale.
/// `vendor_price`: the item's NPC vendor unit price (xiv-gen `price_mid`) if it is
/// vendor-sold, else `None` — used as an absolute anchor against laundering.
#[allow(dead_code)]
pub fn real_price(samples: &[(i32, i32, bool)], vendor_price: Option<i32>) -> RealPriceBreakdown {
    let nq: Vec<(i32, i32)> = samples
        .iter()
        .filter(|&&(_, _, hq)| !hq)
        .map(|&(p, q, _)| (p, q))
        .collect();
    let hq: Vec<(i32, i32)> = samples
        .iter()
        .filter(|&&(_, _, hq)| hq)
        .map(|&(p, q, _)| (p, q))
        .collect();
    RealPriceBreakdown {
        nq: estimate_quality(&nq, vendor_price),
        hq: estimate_quality(&hq, vendor_price),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use ultros_api_types::recent_sales::{SaleData, Sales};

    #[test]
    fn test_format_duration_short() {
        assert_eq!(format_duration_short(0), "0s");
        assert_eq!(format_duration_short(45), "45s");
        assert_eq!(format_duration_short(60), "1m");
        assert_eq!(format_duration_short(65), "1m 5s");
        assert_eq!(format_duration_short(3600), "1h");
        assert_eq!(format_duration_short(3665), "1h 1m");
        assert_eq!(format_duration_short(86400), "1d");
        assert_eq!(format_duration_short(90000), "1d 1h");
        // drops minutes because we only keep 2 units
        assert_eq!(format_duration_short(90060), "1d 1h");
    }

    #[test]
    fn test_roi_badge_class() {
        assert!(roi_badge_class(49).contains("10%"));
        assert!(roi_badge_class(50).contains("12%"));
        assert!(roi_badge_class(100).contains("16%"));
        assert!(roi_badge_class(200).contains("20%"));
        assert!(roi_badge_class(500).contains("24%"));
    }

    #[test]
    fn test_analyze_sales_empty() {
        let stats = analyze_sales(&[], false);
        assert_eq!(stats.total_sales, 0);
        assert_eq!(stats.avg_price, 0);
        assert_eq!(stats.daily_sales, 0.0);
    }

    #[test]
    fn test_analyze_sales_logic() {
        let now = Utc::now().naive_utc();
        let sale1 = Sales {
            price_per_unit: 100,
            sale_date: now - Duration::days(1), // ~1 day ago
        };
        let sale2 = Sales {
            price_per_unit: 200,
            sale_date: now - Duration::days(2), // ~2 days ago
        };
        let sale3 = Sales {
            price_per_unit: 10000,
            sale_date: now - Duration::days(3), // ~3 days ago (outlier)
        };

        let data = SaleData {
            item_id: 1,
            hq: false,
            sales: vec![sale1.clone(), sale2.clone(), sale3.clone()],
        };

        // Without outliers filtering
        let stats = analyze_sales(&[&data], false);
        assert_eq!(stats.total_sales, 3);
        assert_eq!(stats.avg_price, (100 + 200 + 10000) / 3);

        // Oldest date is ~3 days ago. total_sales = 3.
        // Daily sales should be very close to 1.0 (3 sales / 3 days)
        // We use an epsilon since there is a tiny delay between `now` and `Utc::now()` inside `analyze_sales`.
        assert!(
            (stats.daily_sales - 1.0).abs() < 0.01,
            "Expected ~1.0 daily sales, got {}",
            stats.daily_sales
        );

        // With outliers filtering (less than 4 items -> fallback to no filtering)
        let stats_few_items = analyze_sales(&[&data], true);
        assert_eq!(stats_few_items.avg_price, (100 + 200 + 10000) / 3);

        // Let's add more sales to trigger IQR outlier filtering (requires >= 4 items).
        let sale4 = Sales {
            price_per_unit: 150,
            sale_date: now - Duration::days(1) - Duration::hours(12),
        };
        let sale5 = Sales {
            price_per_unit: 180,
            sale_date: now - Duration::days(2) - Duration::hours(12),
        };
        let sale6 = Sales {
            price_per_unit: 120,
            sale_date: now - Duration::hours(12),
        };

        let data2 = SaleData {
            item_id: 1,
            hq: false,
            sales: vec![sale1, sale2, sale3, sale4, sale5, sale6],
        };

        let stats_filtered = analyze_sales(&[&data2], true);
        assert_eq!(stats_filtered.total_sales, 6);

        // The prices are: 100, 120, 150, 180, 200, 10000.
        // Q1 index = 1, Q3 index = 4 (for N=6).
        // q1 = 120, q3 = 200. IQR = 80.
        // Lower bound = 120 - 1.5 * 80 = 0.
        // Upper bound = 200 + 1.5 * 80 = 320.
        // 10000 is correctly identified as an outlier and filtered out.
        // The remaining valid prices: 100, 120, 150, 180, 200.
        // Sum = 750. Average = 750 / 5 = 150.
        assert_eq!(stats_filtered.avg_price, 150);

        // Oldest date is ~3 days ago. total_sales = 6.
        assert!(
            (stats_filtered.daily_sales - 2.0).abs() < 0.01,
            "Expected ~2.0 daily sales, got {}",
            stats_filtered.daily_sales
        );
    }

    #[test]
    fn test_roi_badge_class_edge_cases() {
        assert!(roi_badge_class(0).contains("10%"));
        assert!(roi_badge_class(-50).contains("10%"));

        // Just under boundaries
        assert!(roi_badge_class(49).contains("10%"));
        assert!(roi_badge_class(99).contains("12%"));
        assert!(roi_badge_class(199).contains("16%"));
        assert!(roi_badge_class(499).contains("20%"));

        // Exactly on boundaries
        assert!(roi_badge_class(50).contains("12%"));
        assert!(roi_badge_class(100).contains("16%"));
        assert!(roi_badge_class(200).contains("20%"));
        assert!(roi_badge_class(500).contains("24%"));

        // High numbers
        assert!(roi_badge_class(1000).contains("24%"));
        assert!(roi_badge_class(10000).contains("24%"));
    }

    #[test]
    fn test_format_duration_short_edge_cases() {
        assert_eq!(format_duration_short(1), "1s");
        assert_eq!(format_duration_short(59), "59s");
        assert_eq!(format_duration_short(3599), "59m 59s");
        assert_eq!(format_duration_short(3601), "1h 1s");
        assert_eq!(format_duration_short(86399), "23h 59m");
        assert_eq!(format_duration_short(86401), "1d 1s");

        // large number of days
        assert_eq!(format_duration_short(86400 * 365 + 3600), "365d 1h");
    }
}

#[cfg(test)]
mod real_price_tests {
    use super::*;

    /// Build NQ-only samples from (price, qty) pairs.
    fn nq(pairs: &[(i32, i32)]) -> Vec<(i32, i32, bool)> {
        pairs.iter().map(|&(p, q)| (p, q, false)).collect()
    }

    #[test]
    fn headline_case_one_huge_outlier() {
        // 199 sales @ 16_000 + one 75M launder sale (qty 1), non-vendor item.
        let mut s = vec![(16_000i32, 1i32, false); 199];
        s.push((75_000_000, 1, false));
        let r = real_price(&s, None);
        let (is_hq, est) = r.primary().expect("primary present");
        assert!(!is_hq);
        assert_eq!(est.value, 16_000);
        assert_eq!(est.total, 200);
        assert_eq!(est.used, 199);
        assert_eq!(est.excluded, 1);
    }

    #[test]
    fn vendor_guard_catches_majority_launder() {
        // vendor price 100 -> cap 10_000. Three qty-1 launder sales dominate, so the
        // quartiles shift and IQR alone would NOT remove them; the vendor anchor does.
        let s = vec![
            (49_000, 1, false),
            (50_000, 1, false),
            (51_000, 1, false),
            (100, 1, false),
            (110, 1, false),
        ];
        let r = real_price(&s, Some(100));
        let (_, est) = r.primary().expect("primary present");
        assert_eq!(est.total, 5);
        assert_eq!(est.used, 2); // only the two legit sales remain
        assert_eq!(est.excluded, 3);
        assert_eq!(est.value, 110); // median of [100, 110]
    }

    #[test]
    fn vendor_guard_ignores_non_qty1() {
        // Same overpriced price but qty 2 -> NOT removed by the guard (guard is qty==1 only).
        let s = vec![
            (100, 1, false),
            (105, 1, false),
            (110, 1, false),
            (120, 1, false),
            (50_000, 2, false),
        ];
        let r = real_price(&s, Some(100));
        let (_, est) = r.primary().expect("primary present");
        assert_eq!(est.total, 5);
        assert_eq!(est.used, 4);
        assert!(est.value >= 100 && est.value <= 120);
    }

    #[test]
    fn small_sample_uses_median_not_mean() {
        // n=3 (<4): median resists the launder; the mean would be ~25M.
        let s = nq(&[(16_000, 1), (16_000, 1), (75_000_000, 1)]);
        let (_, est) = real_price(&s, None).primary().expect("primary present");
        assert_eq!(est.value, 16_000);
        assert_eq!(est.used, 3);
        assert_eq!(est.total, 3);
        assert_eq!(est.excluded, 0);
    }

    #[test]
    fn all_equal_excludes_nothing() {
        let s = nq(&[(16_000, 1); 10]);
        let (_, est) = real_price(&s, None).primary().expect("primary present");
        assert_eq!(est.value, 16_000);
        assert_eq!(est.used, 10);
        assert_eq!(est.excluded, 0);
    }

    #[test]
    fn hq_and_nq_computed_independently() {
        // NQ ~16k with more sales (primary), HQ ~50k (secondary). Never averaged.
        let mut s = vec![(16_000i32, 1i32, false); 6];
        s.extend(vec![(50_000, 1, true); 5]);
        let r = real_price(&s, None);
        let (p_is_hq, p) = r.primary().expect("primary present");
        assert!(!p_is_hq);
        assert_eq!(p.value, 16_000);
        let (s_is_hq, sec) = r.secondary().expect("secondary present");
        assert!(s_is_hq);
        assert_eq!(sec.value, 50_000);
        assert_ne!(p.value, 33_000); // not a blended NQ+HQ mean
    }

    #[test]
    fn secondary_below_threshold_is_hidden() {
        // HQ has only 3 sales (<4) -> omitted from secondary(), but still in the breakdown.
        let mut s = vec![(16_000i32, 1i32, false); 6];
        s.extend(vec![(50_000, 1, true); 3]);
        let r = real_price(&s, None);
        assert!(r.secondary().is_none());
        assert!(r.hq.is_some());
    }

    #[test]
    fn empty_is_none() {
        let r = real_price(&[], None);
        assert!(r.primary().is_none());
        assert!(r.nq.is_none());
        assert!(r.hq.is_none());
    }
}
