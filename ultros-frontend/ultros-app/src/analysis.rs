use crate::math::filter_outliers_iqr;
use chrono::Utc;
use ultros_api_types::recent_sales::SaleData;

#[derive(Clone, Copy, Debug)]
pub struct SalesStats {
    pub daily_sales: f32,
    pub avg_price: i32,
    pub total_sales: usize,
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
        let filtered = filter_outliers_iqr(&prices);
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
