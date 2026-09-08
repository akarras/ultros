use crate::math::filter_outliers_iqr_in_place;
use chrono::{Duration, Utc};
use ultros_api_types::recent_sales::SaleData;

#[derive(Clone, Copy, Debug)]
pub struct SalesStats {
    pub daily_sales: f32,
    pub avg_price: i32,
    pub total_sales: usize,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SalesCadence {
    Fast,
    Steady,
    Slow,
    NotEnoughData,
}

#[allow(dead_code)]
/// Converts sales/day plus sample count into a movement verdict.
///
/// Thresholds:
/// - NotEnoughData: < 3 sales or <= 0 sales/day
/// - Fast: >= 5 sales/day
/// - Steady: >= 1 sale/day
/// - Slow: > 0 sales/day
pub fn get_sales_cadence(sales_per_day: f32, sample_count: usize) -> SalesCadence {
    if sample_count < 3 || sales_per_day <= 0.0 {
        SalesCadence::NotEnoughData
    } else if sales_per_day >= 5.0 {
        SalesCadence::Fast
    } else if sales_per_day >= 1.0 {
        SalesCadence::Steady
    } else {
        SalesCadence::Slow
    }
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
pub fn roi_badge_class(roi: i32) -> &'static str {
    if roi >= 500 {
        "inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_24%,transparent)]"
    } else if roi >= 200 {
        "inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_20%,transparent)]"
    } else if roi >= 100 {
        "inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_16%,transparent)]"
    } else if roi >= 50 {
        "inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)]"
    } else {
        "inline-flex items-center justify-end px-2 py-1 rounded-full text-xs font-semibold border text-[color:var(--color-text)] border-[color:var(--color-outline)] bg-[color:color-mix(in_srgb,var(--brand-ring)_10%,transparent)]"
    }
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RealPriceBreakdown {
    pub nq: Option<RealPriceEstimate>,
    pub hq: Option<RealPriceEstimate>,
}

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
fn median_in_place(prices: &mut [i32]) -> i32 {
    let (_, &mut val, _) = prices.select_nth_unstable(prices.len() / 2);
    val
}

/// Robust price for a single quality from `(price, qty)` samples.
/// Vendor guard (drop qty==1 sales priced > 100x vendor), then IQR-filtered mean,
/// with a median fallback for fewer than 4 surviving samples.
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

/// Minimum span used as the velocity denominator. Guards the degenerate
/// case observed in prod of six sales sharing one timestamp (one buyer
/// clearing six listings at once), which would otherwise divide by zero.
pub const MIN_VELOCITY_SPAN_DAYS: f32 = 1.0 / 24.0;

/// Display ceiling for ROI. Beyond this the exact figure carries no
/// decision value, and the previous `as i32` cast saturated at `i32::MAX`
/// for tiny buy prices (a 2-gil buy against a laundered sale price).
pub const ROI_DISPLAY_CEILING: i32 = 100_000;

/// Display ceiling for the Price column's "vs median" tell, in percent.
/// Same rationale as [`ROI_DISPLAY_CEILING`]: past this the exact figure
/// carries no decision value — "+1,000%" and "+4,000%" both say "this price
/// is nothing like what the item trades for" — and the digit string crowds a
/// 10px sub-line (prod rendered "+399900%" before the tell was fixed to
/// compare like qualities).
///
/// One-sided by construction: `delta_pct` divides a positive price by a
/// positive median, so the tell can never fall below -100% and needs no
/// floor. It sits below `TROLL_MULTIPLE`'s +4,900%, so the clamp is what the
/// reader sees for prices between roughly 11x and 50x the median — past that
/// the troll tell takes over from the percentage entirely.
pub const VS_MEDIAN_DISPLAY_CEILING_PCT: f32 = 999.0;

/// Recent sales per day, derived from the bounded `RecentSales` buffer.
///
/// `avg_sale_duration` is `(now - oldest_sale) / num_sold`, so the total
/// span is `avg * num_sold` and velocity is `num_sold / span`. Because the
/// buffer holds the *most recent* sales, this estimates the current rate
/// rather than a lifetime average; resolution degrades only at the high
/// end, which does not matter for a floor-style filter.
pub fn velocity_per_day(summary: &SaleSummary) -> Option<f32> {
    if summary.num_sold == 0 {
        return None;
    }
    let avg = summary.avg_sale_duration?;
    let span_days = (avg.num_seconds() as f32 * summary.num_sold as f32) / 86_400.0;
    Some(summary.num_sold as f32 / span_days.max(MIN_VELOCITY_SPAN_DAYS))
}

/// Expected gil per day from repeating one trade: per-trade profit times a
/// sales-per-day rate. Truncates (a float→int cast, which saturates rather
/// than wrapping). The rate's provenance is the caller's: the flip finder
/// passes [`velocity_per_day`] off its six-sale buffer, the recipe analyzer
/// passes the 7-day rollup's `num_sold / 7`.
pub fn profit_per_day_from_rate(profit: i32, rate: f32) -> i32 {
    (profit as f64 * rate as f64) as i32
}

/// Percent change between the mean of the newest samples and the mean of
/// the oldest samples. `prices` is newest-first, matching the wire order
/// of `RecentSales`.
///
/// Returns `None` below 4 samples — a two-point "trend" is noise wearing a
/// percentage sign. With an odd count the middle sample is skipped so the
/// two windows never overlap.
pub fn price_drift_pct(prices: &[i32]) -> Option<f32> {
    if prices.len() < 4 {
        return None;
    }
    let take = 3.min(prices.len() / 2);
    let newest: i64 = prices[..take].iter().map(|p| *p as i64).sum();
    let oldest: i64 = prices[prices.len() - take..]
        .iter()
        .map(|p| *p as i64)
        .sum();
    if oldest == 0 {
        return None;
    }
    Some(((newest - oldest) as f32 / oldest as f32) * 100.0)
}

/// The noise floor a signed percent must clear before it is coloured.
/// Origin: the flip finder's Drift cell, where ±1% inside a six-sale window
/// is noise wearing a percentage sign. Reused by the recipe analyzer's
/// Drift column and its Price "vs median" tell, which read the same kind of
/// small, sample-limited percentage.
pub const DELTA_DEAD_BAND_PCT: f32 = 1.0;

/// The colour class for a signed percentage: green above `+dead_band`, red
/// below `-dead_band`, muted inside the band and when there is no figure.
/// `dead_band` is the caller's noise floor (0.0 colours every non-zero
/// sign). A NaN falls through both comparisons and reads neutral.
pub fn signed_delta_class(pct: Option<f32>, dead_band: f32) -> &'static str {
    match pct {
        Some(p) if p > dead_band => "text-emerald-300",
        Some(p) if p < -dead_band => "text-red-300",
        _ => "text-[color:var(--color-text-muted)]",
    }
}

/// Percent change across a sparkline window, from its first traded price to
/// its last. The server sends the first and last *non-zero* points
/// (`arrayFilter(x -> x > 0, points)`, `ultros-clickhouse/src/queries.rs:158-167`),
/// so `first == 0` means nothing traded anywhere in the window: no baseline
/// exists, and 0 is not a price.
pub fn first_to_last_pct(first: u32, last: u32) -> Option<f32> {
    (first > 0).then(|| (last as f32 - first as f32) / first as f32 * 100.0)
}

/// Return on investment as a percentage, computed in f64 and clamped to
/// [`ROI_DISPLAY_CEILING`].
pub fn return_on_investment(profit: i32, cheapest_price: i32) -> i32 {
    if cheapest_price <= 0 {
        return 0;
    }
    let roi = (profit as f64 / cheapest_price as f64) * 100.0;
    roi.clamp(-(ROI_DISPLAY_CEILING as f64), ROI_DISPLAY_CEILING as f64) as i32
}

/// Trustworthiness of a row's numbers when ClickHouse has no rollup for it.
/// Replaces the page-level disclaimer copy with a per-row statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedConfidence {
    High,
    Medium,
    Low,
}

/// Band a row from its buffer depth and observed velocity. A full buffer
/// only earns `High` if the sales are actually recent — six sales spread
/// over a decade is a dead item, not a confident one.
pub fn derived_confidence(summary: &SaleSummary) -> DerivedConfidence {
    let velocity = velocity_per_day(summary).unwrap_or(0.0);
    if summary.num_sold >= 6 && velocity >= 1.0 {
        DerivedConfidence::High
    } else if summary.num_sold >= 4 && velocity >= 0.2 {
        DerivedConfidence::Medium
    } else {
        DerivedConfidence::Low
    }
}

/// Sniper-clamp threshold: drop any sale priced below this fraction of the raw median.
const SNIPER_FRACTION: f64 = 0.1;

pub fn median_in_place_i32(sorted: &mut [i32]) -> i32 {
    if sorted.is_empty() {
        return 0;
    }
    let n = sorted.len();
    if n % 2 == 1 {
        let (_, &mut val, _) = sorted.select_nth_unstable(n / 2);
        val
    } else {
        let (left, &mut right, _) = sorted.select_nth_unstable(n / 2);
        let left_max = *left.iter().max().unwrap();
        ((left_max as i64 + right as i64) / 2) as i32
    }
}

/// Listings whose price is at least this multiple of the row's median sale are treated as troll
/// listings and ignored when picking the world floor.
const TROLL_MULTIPLE: i64 = 50;

pub fn is_troll_listing(price: i32, median: i32) -> bool {
    median > 0 && (price as i64) > (median as i64).saturating_mul(TROLL_MULTIPLE)
}

/// Sniper-clamped price set: drops sales priced below `SNIPER_FRACTION` of the
/// raw median. If the clamp would remove everything, the raw set is kept.
/// Shared by the analyzer's `compute_summary` and the item-page flip card.
///
/// # Note
/// The clamp runs in-place, so the order of the returned elements is
/// **undefined** — neither the input order nor sorted. Every caller feeds the
/// result straight into `median_in_place_i32`, a min/max/sum, or
/// `filter_outliers_iqr_in_place`, all of which are order-independent. Sort the
/// result yourself if you ever need a stable order.
pub fn sniper_clamp(mut prices: Vec<i32>) -> Vec<i32> {
    if prices.is_empty() {
        return prices;
    }
    let raw_median = median_in_place_i32(&mut prices);
    let floor = (raw_median as f64 * SNIPER_FRACTION) as i32;

    let has_valid = prices.iter().any(|&p| p >= floor);
    if has_valid {
        prices.retain(|&p| p >= floor);
    }
    prices
}

/// Flip estimate shared by the flip-finder table and the item-page flip card:
/// median of recent sales, capped by the sell world's current floor. A floor
/// more than `TROLL_MULTIPLE`× the median is a troll listing and is ignored.
pub fn flip_estimated_sale_price(median_price: i32, world_floor: Option<i32>) -> i32 {
    match world_floor.filter(|floor| !is_troll_listing(*floor, median_price)) {
        Some(floor) => median_price.min(floor),
        None => median_price,
    }
}

/// Per-unit flip profit. The 5% market-board tax comes off the sale, not the buy.
pub fn flip_profit(estimated_sale_price: i32, buy_price: i32, include_tax: bool) -> i32 {
    let estimated = if include_tax {
        (estimated_sale_price as f32 * 0.95) as i32
    } else {
        estimated_sale_price
    };
    estimated - buy_price
}

/// Gil the 5% market-board tax takes off a sale at this price. Shares
/// `flip_profit`'s truncating math so `buy + profit + tax == sale` exactly.
pub fn sale_tax(estimated_sale_price: i32) -> i32 {
    estimated_sale_price - (estimated_sale_price as f32 * 0.95) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use ultros_api_types::recent_sales::{SaleData, Sales};

    /// `sniper_clamp` returns its survivors in an undefined order, so compare
    /// the multiset rather than pinning whatever `select_nth_unstable` happened
    /// to leave behind.
    fn sorted_clamp(prices: Vec<i32>) -> Vec<i32> {
        let mut out = sniper_clamp(prices);
        out.sort_unstable();
        out
    }

    #[test]
    fn sniper_clamp_drops_prices_below_ten_percent_of_median() {
        // raw median of [10, 1000, 1100, 1200, 1300] is 1100; floor = 110 → 10 dropped
        assert_eq!(
            sorted_clamp(vec![10, 1000, 1100, 1200, 1300]),
            vec![1000, 1100, 1200, 1300]
        );
    }

    #[test]
    fn sniper_clamp_keeps_raw_set_when_clamp_would_empty_it() {
        // all equal → floor = 100 * 0.1 = 10, nothing dropped; and empty stays empty
        assert_eq!(sniper_clamp(vec![100]), vec![100]);
        assert_eq!(sniper_clamp(Vec::new()), Vec::<i32>::new());
    }

    /// The in-place clamp must keep the same survivors as the straightforward
    /// clone-and-filter it replaced. Sizes here straddle 20, the length at
    /// which `select_nth_unstable` stops insertion-sorting the whole slice and
    /// starts leaving the input genuinely unordered.
    #[test]
    fn sniper_clamp_matches_clone_and_filter_reference() {
        fn reference(prices: Vec<i32>) -> Vec<i32> {
            if prices.is_empty() {
                return prices;
            }
            let mut raw = prices.clone();
            let raw_median = median_in_place_i32(&mut raw);
            let floor = (raw_median as f64 * SNIPER_FRACTION) as i32;
            let clamped: Vec<i32> = prices.iter().copied().filter(|p| *p >= floor).collect();
            if clamped.is_empty() { prices } else { clamped }
        }

        // Deterministic LCG — no rand dependency in this crate's test deps.
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) % 5000) as i32 + 1
        };

        for len in [1usize, 2, 3, 5, 19, 20, 21, 64, 257] {
            for _ in 0..40 {
                let prices: Vec<i32> = (0..len).map(|_| next()).collect();
                let mut expected = reference(prices.clone());
                expected.sort_unstable();
                assert_eq!(
                    sorted_clamp(prices.clone()),
                    expected,
                    "len {len} diverged for {prices:?}"
                );
            }
        }
    }

    /// A lone snipe among realistic prices is dropped even once the input is
    /// long enough that the clamp no longer leaves it sorted.
    #[test]
    fn sniper_clamp_drops_snipes_in_a_large_unsorted_set() {
        let mut prices: Vec<i32> = (0..64).map(|i| 1000 + (i * 37) % 400).collect();
        prices.insert(31, 5); // one snipe, well below 10% of the ~1200 median
        let clamped = sorted_clamp(prices);
        assert_eq!(clamped.len(), 64);
        assert!(!clamped.contains(&5));
    }

    #[test]
    fn flip_estimate_caps_median_by_world_floor() {
        assert_eq!(flip_estimated_sale_price(1000, Some(800)), 800);
        assert_eq!(flip_estimated_sale_price(1000, Some(1200)), 1000);
        assert_eq!(flip_estimated_sale_price(1000, None), 1000);
    }

    #[test]
    fn flip_estimate_ignores_troll_floor() {
        // floor 60_000 vs median 1_000 exceeds TROLL_MULTIPLE (50x) → ignored
        assert_eq!(flip_estimated_sale_price(1000, Some(60_000)), 1000);
    }

    #[test]
    fn flip_profit_applies_five_percent_tax() {
        assert_eq!(flip_profit(1000, 500, true), 450); // 950 - 500
        assert_eq!(flip_profit(1000, 500, false), 500);
    }

    #[test]
    fn sale_tax_reconciles_with_flip_profit() {
        assert_eq!(sale_tax(100_000), 5_000);
        // Truncation must land on the same side as flip_profit's, so the
        // three columns always sum back to the sale price.
        for sale in [999, 1000, 1001, 12_345, i32::MAX] {
            let buy = sale / 2;
            assert_eq!(buy + flip_profit(sale, buy, true) + sale_tax(sale), sale);
        }
    }

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
    fn test_analyze_sales_zero_iqr() {
        use ultros_api_types::recent_sales::Sales;
        let now = Utc::now().naive_utc();
        let sales_list = vec![
            Sales {
                price_per_unit: 100,
                sale_date: now - Duration::days(1),
            },
            Sales {
                price_per_unit: 100,
                sale_date: now - Duration::days(2),
            },
            Sales {
                price_per_unit: 100,
                sale_date: now - Duration::days(3),
            },
            Sales {
                price_per_unit: 100,
                sale_date: now - Duration::days(4),
            },
            Sales {
                price_per_unit: 100,
                sale_date: now - Duration::days(5),
            },
        ];

        let data = SaleData {
            item_id: 1,
            hq: false,
            sales: sales_list,
        };

        // IQR of [100, 100, 100, 100, 100] is 0
        // analyze_sales should handle this without dropping all items.
        let stats = analyze_sales(&[&data], true);

        // 5 total sales
        assert_eq!(stats.total_sales, 5);
        // Average should be exactly 100
        assert_eq!(stats.avg_price, 100);
        // Ensure daily sales calculation doesn't panic and returns a sensible number (5 sales over ~5 days = ~1.0)
        assert!((stats.daily_sales - 1.0).abs() < 0.1);
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

    fn summary_with(num_sold: usize, avg_secs: i64) -> SaleSummary {
        SaleSummary {
            item_id: 1,
            hq: false,
            num_sold,
            avg_sale_duration: Some(Duration::seconds(avg_secs)),
            days_since_last_sale: Some(Duration::hours(1)),
            max_price: 0,
            avg_price: 0,
            median_price: 0,
            min_price: 0,
        }
    }

    #[test]
    fn velocity_full_buffer_over_three_days() {
        // 6 sales spanning 3 days => avg gap = 3d/6 = 12h => 2 sales/day.
        let s = summary_with(6, 12 * 3600);
        let v = velocity_per_day(&s).unwrap();
        assert!((v - 2.0).abs() < 0.001, "expected 2.0, got {v}");
    }

    #[test]
    fn velocity_partial_buffer() {
        // 2 sales spanning 4 days => avg gap = 2 days => 0.5 sales/day.
        let s = summary_with(2, 2 * 86_400);
        let v = velocity_per_day(&s).unwrap();
        assert!((v - 0.5).abs() < 0.001, "expected 0.5, got {v}");
    }

    #[test]
    fn velocity_clamps_zero_span() {
        // Observed in prod: 6 sales sharing one timestamp (one buyer clearing
        // six listings). Span 0 must not divide by zero or return infinity.
        let s = summary_with(6, 0);
        let v = velocity_per_day(&s).unwrap();
        assert!(v.is_finite(), "velocity must stay finite, got {v}");
        assert!((v - 6.0 / MIN_VELOCITY_SPAN_DAYS).abs() < 0.001);
    }

    #[test]
    fn velocity_decade_old_buffer_is_near_zero() {
        // Observed max span: 94,041 hours. 6 sales over ~10.7 years.
        let s = summary_with(6, 94_041 * 3600 / 6);
        let v = velocity_per_day(&s).unwrap();
        assert!(v < 0.01, "expected near-zero velocity, got {v}");
    }

    #[test]
    fn profit_per_day_scales_up_for_fast_sellers() {
        // 6 sales, avg gap 6h => 4 sales/day. 100 gil profit => 400/day,
        // not clamped down to the flat profit figure.
        let s = summary_with(6, 6 * 3600);
        assert_eq!(
            profit_per_day_from_rate(100, velocity_per_day(&s).unwrap_or(0.0)),
            400
        );
    }

    #[test]
    fn profit_per_day_scales_down_for_slow_sellers() {
        // avg gap 2 days => 0.5 sales/day => half the profit per day.
        let s = summary_with(2, 2 * 86_400);
        assert_eq!(
            profit_per_day_from_rate(100, velocity_per_day(&s).unwrap_or(0.0)),
            50
        );
    }

    #[test]
    fn profit_per_day_zero_without_sale_history() {
        let mut s = summary_with(0, 0);
        s.avg_sale_duration = None;
        assert_eq!(
            profit_per_day_from_rate(100, velocity_per_day(&s).unwrap_or(0.0)),
            0
        );
    }

    #[test]
    fn profit_per_day_from_rate_is_the_shared_form() {
        // The flip finder's buffer velocity and the recipe's rollup rate
        // feed the same arithmetic.
        assert_eq!(profit_per_day_from_rate(1_000, 2.5), 2_500);
        assert_eq!(profit_per_day_from_rate(1_000, 0.25), 250);
        assert_eq!(profit_per_day_from_rate(-300, 3.0), -900);
        assert_eq!(profit_per_day_from_rate(1_000, 0.0), 0);
        // Truncation, not rounding: 999 * 1.5 = 1498.5.
        assert_eq!(profit_per_day_from_rate(999, 1.5), 1_498);
        // A float -> int cast saturates rather than wrapping.
        assert_eq!(profit_per_day_from_rate(i32::MAX, 1_000.0), i32::MAX);
    }

    #[test]
    fn signed_delta_class_has_a_dead_band() {
        assert_eq!(signed_delta_class(Some(4.0), 1.0), "text-emerald-300");
        assert_eq!(signed_delta_class(Some(-4.0), 1.0), "text-red-300");
        // Inside the band, and exactly on it, read neutral.
        let muted = "text-[color:var(--color-text-muted)]";
        assert_eq!(signed_delta_class(Some(0.4), 1.0), muted);
        assert_eq!(signed_delta_class(Some(1.0), 1.0), muted);
        assert_eq!(signed_delta_class(Some(-1.0), 1.0), muted);
        assert_eq!(signed_delta_class(None, 1.0), muted);
        // A zero dead band colours any non-zero sign (the movers' rule).
        assert_eq!(signed_delta_class(Some(0.2), 0.0), "text-emerald-300");
        // NaN is neither above nor below: neutral, never a panic.
        assert_eq!(signed_delta_class(Some(f32::NAN), 1.0), muted);
    }

    /// `analyzer.rs`'s three Drift arms cut at ±1.0 with `text-emerald-300`
    /// / `text-red-300` / muted; the new const and fn must reproduce exactly
    /// those thresholds (`signed_delta_class_has_a_dead_band` passes `1.0`
    /// by hand and so cannot pin the const). The cell's *text* is unchanged
    /// by construction — the fold touches only the class, and `+{d:.0}%`
    /// and `{d:.0}%` are `{d:+.0}%` over the ranges the old arms guarded, a
    /// property of the `+` flag rather than of this code — so the byte
    /// identity of `/flip-finder` rides on `routes::analyzer`'s 69 existing
    /// tests plus manual check 9 in the PR body.
    #[test]
    fn signed_delta_class_reproduces_the_flip_finders_drift_arms() {
        for d in [1.4f32, 4.6, 12.5, 99.5, 100.4] {
            assert_eq!(
                signed_delta_class(Some(d), DELTA_DEAD_BAND_PCT),
                "text-emerald-300"
            );
        }
        for d in [-1.4f32, -3.6, -50.0] {
            assert_eq!(
                signed_delta_class(Some(d), DELTA_DEAD_BAND_PCT),
                "text-red-300"
            );
        }
        for d in [0.0f32, 0.9, -0.9] {
            assert_eq!(
                signed_delta_class(Some(d), DELTA_DEAD_BAND_PCT),
                "text-[color:var(--color-text-muted)]"
            );
        }
    }

    #[test]
    fn first_to_last_pct_needs_a_first_trade() {
        assert_eq!(first_to_last_pct(100, 150), Some(50.0));
        assert_eq!(first_to_last_pct(100, 50), Some(-50.0));
        assert_eq!(first_to_last_pct(100, 100), Some(0.0));
        // No trade in the window's first bucket: no percentage exists.
        assert_eq!(first_to_last_pct(0, 150), None);
        assert_eq!(first_to_last_pct(0, 0), None);
    }

    #[test]
    fn velocity_none_when_no_sales() {
        let mut s = summary_with(0, 0);
        s.avg_sale_duration = None;
        assert_eq!(velocity_per_day(&s), None);
    }

    #[test]
    fn drift_detects_rising_price() {
        // newest-first: newest 3 mean 200, oldest 3 mean 100 => +100%.
        let prices = [200, 200, 200, 100, 100, 100];
        let d = price_drift_pct(&prices).unwrap();
        assert!((d - 100.0).abs() < 0.01, "expected +100.0, got {d}");
    }

    #[test]
    fn drift_detects_falling_price() {
        let prices = [50, 50, 50, 100, 100, 100];
        let d = price_drift_pct(&prices).unwrap();
        assert!((d + 50.0).abs() < 0.01, "expected -50.0, got {d}");
    }

    #[test]
    fn drift_flat_is_zero() {
        let prices = [100, 100, 100, 100, 100, 100];
        assert!(price_drift_pct(&prices).unwrap().abs() < 0.01);
    }

    #[test]
    fn drift_none_below_four_samples() {
        assert_eq!(price_drift_pct(&[100, 100, 100]), None);
        assert_eq!(price_drift_pct(&[100]), None);
        assert_eq!(price_drift_pct(&[]), None);
    }

    #[test]
    fn drift_with_five_samples_skips_the_middle() {
        // len 5 => take 2 from each end, index 2 ignored.
        let prices = [200, 200, 999_999, 100, 100];
        let d = price_drift_pct(&prices).unwrap();
        assert!((d - 100.0).abs() < 0.01, "expected +100.0, got {d}");
    }

    #[test]
    fn roi_does_not_saturate_at_i32_max() {
        // The prod bug: buy 2 gil, profit 213,749,998 previously produced
        // i32::MAX (2147483647) via an f32 -> i32 saturating cast.
        let roi = return_on_investment(213_749_998, 2);
        assert_eq!(roi, ROI_DISPLAY_CEILING);
        assert_ne!(roi, i32::MAX);
    }

    #[test]
    fn roi_normal_range_is_exact() {
        assert_eq!(return_on_investment(50, 100), 50);
        assert_eq!(return_on_investment(300, 100), 300);
    }

    #[test]
    fn roi_zero_price_is_zero() {
        assert_eq!(return_on_investment(1000, 0), 0);
        assert_eq!(return_on_investment(1000, -5), 0);
    }

    #[test]
    fn roi_negative_profit_is_negative() {
        assert_eq!(return_on_investment(-50, 100), -50);
    }

    #[test]
    fn confidence_bands_track_buffer_and_velocity() {
        // Full buffer + brisk velocity (6 sales over 3 days = 2/day).
        assert_eq!(
            derived_confidence(&summary_with(6, 12 * 3600)),
            DerivedConfidence::High
        );
        // Mid buffer.
        assert_eq!(
            derived_confidence(&summary_with(4, 86_400)),
            DerivedConfidence::Medium
        );
        // Thin buffer.
        assert_eq!(
            derived_confidence(&summary_with(1, 86_400)),
            DerivedConfidence::Low
        );
        // Full buffer but glacial (6 sales over ~10 years) is not High.
        assert_eq!(
            derived_confidence(&summary_with(6, 94_041 * 3600 / 6)),
            DerivedConfidence::Low
        );
    }

    #[test]
    fn test_get_sales_cadence() {
        use SalesCadence::*;

        // NotEnoughData: fewer than 3 sales
        assert_eq!(get_sales_cadence(10.0, 0), NotEnoughData);
        assert_eq!(get_sales_cadence(10.0, 1), NotEnoughData);
        assert_eq!(get_sales_cadence(10.0, 2), NotEnoughData);

        // NotEnoughData: non-positive cadence
        assert_eq!(get_sales_cadence(0.0, 3), NotEnoughData);
        assert_eq!(get_sales_cadence(-1.0, 3), NotEnoughData);

        // Fast: >= 5 sales/day
        assert_eq!(get_sales_cadence(5.0, 3), Fast);
        assert_eq!(get_sales_cadence(10.0, 3), Fast);
        assert_eq!(get_sales_cadence(4.99, 3), Steady); // Just below Fast

        // Steady: >= 1 sale/day
        assert_eq!(get_sales_cadence(1.0, 3), Steady);
        assert_eq!(get_sales_cadence(4.9, 3), Steady);
        assert_eq!(get_sales_cadence(0.99, 3), Slow); // Just below Steady

        // Slow: positive below 1
        assert_eq!(get_sales_cadence(0.1, 3), Slow);
        assert_eq!(get_sales_cadence(0.5, 3), Slow);
        assert_eq!(get_sales_cadence(0.0001, 3), Slow);

        // Boundary cases
        assert_eq!(get_sales_cadence(0.999, 3), Slow);
        assert_eq!(get_sales_cadence(1.0, 3), Steady);
        assert_eq!(get_sales_cadence(4.999, 3), Steady);
        assert_eq!(get_sales_cadence(5.0, 3), Fast);
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
