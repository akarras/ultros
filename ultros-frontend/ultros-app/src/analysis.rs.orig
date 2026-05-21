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
