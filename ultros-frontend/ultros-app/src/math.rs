pub fn filter_outliers_iqr(data: &[i32]) -> Vec<i32> {
    if data.len() < 4 {
        return data.to_vec();
    }

    let mut sorted = data.to_vec();
    sorted.sort_unstable();

    let n = sorted.len();
    let q1 = sorted[n / 4];
    let q3 = sorted[n * 3 / 4];
    let iqr = q3 - q1;

    // Using 1.5 * IQR is standard.
    // We calculate bounds in f64 then cast back, or just use integer arithmetic carefully?
    // Let's use f64 for the multiplier to be safe and accurate.
    let lower_bound = q1 as f64 - 1.5 * iqr as f64;
    let upper_bound = q3 as f64 + 1.5 * iqr as f64;

    data.iter()
        .filter(|&&x| (x as f64) >= lower_bound && (x as f64) <= upper_bound)
        .cloned()
        .collect()
}

/// Calculates the average number of sales per day.
pub fn calculate_sales_velocity(num_sales: usize, duration_seconds: f64) -> f64 {
    if duration_seconds <= 0.0 {
        return 0.0;
    }
    // Sales per day
    (num_sales as f64) / (duration_seconds / 86400.0)
}

/// Calculates the coefficient of variation (CV) for a set of prices.
/// CV = Standard Deviation / Mean
/// Returns 0.0 if the input is empty or mean is 0.
pub fn calculate_volatility(prices: &[i32]) -> f64 {
    if prices.is_empty() {
        return 0.0;
    }
    let n = prices.len() as f64;
    let mean = prices.iter().map(|&p| p as f64).sum::<f64>() / n;
    if mean == 0.0 {
        return 0.0;
    }
    let variance = prices
        .iter()
        .map(|&p| {
            let diff = p as f64 - mean;
            diff * diff
        })
        .sum::<f64>()
        / n;
    let std_dev = variance.sqrt();
    std_dev / mean
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_outliers_iqr() {
        let data = vec![1, 2, 3, 4, 5, 100];
        let filtered = filter_outliers_iqr(&data);
        assert_eq!(filtered, vec![1, 2, 3, 4, 5]);

        let data = vec![1, 2, 3];
        let filtered = filter_outliers_iqr(&data);
        assert_eq!(filtered, vec![1, 2, 3]);

        let data = vec![100, 1, 2, 3, 4, 5];
        let filtered = filter_outliers_iqr(&data);
        assert_eq!(filtered, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_calculate_sales_velocity() {
        // 10 sales in 1 day (86400 seconds) -> 10.0 sales/day
        assert!((calculate_sales_velocity(10, 86400.0) - 10.0).abs() < 1e-6);
        // 10 sales in 2 days -> 5.0 sales/day
        assert!((calculate_sales_velocity(10, 172800.0) - 5.0).abs() < 1e-6);
        // 0 duration -> 0.0
        assert_eq!(calculate_sales_velocity(10, 0.0), 0.0);
    }

    #[test]
    fn test_calculate_volatility() {
        // Uniform prices -> 0 volatility
        let prices = vec![100, 100, 100];
        assert_eq!(calculate_volatility(&prices), 0.0);

        // [100, 200]
        // Mean = 150
        // Variance = ((50^2) + (-50^2)) / 2 = (2500 + 2500) / 2 = 2500
        // StdDev = 50
        // CV = 50 / 150 = 0.333...
        let prices = vec![100, 200];
        assert!((calculate_volatility(&prices) - (1.0 / 3.0)).abs() < 1e-6);

        // Empty -> 0.0
        let prices: Vec<i32> = vec![];
        assert_eq!(calculate_volatility(&prices), 0.0);
    }
}
