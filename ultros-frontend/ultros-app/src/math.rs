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

/// Calculates sales per day based on the time range between the oldest sale and now.
/// `timestamps` should be in milliseconds.
/// `now` should be the current time in milliseconds.
pub fn calculate_sales_velocity(timestamps: &[i64], now: i64) -> f64 {
    if timestamps.is_empty() {
        return 0.0;
    }
    // Find the oldest timestamp.
    let oldest = timestamps.iter().min().unwrap();
    let duration_ms = (now - oldest).max(0);

    // If duration is effectively zero (e.g. all sales happened this millisecond),
    // we can't calculate a meaningful velocity.
    if duration_ms < 1000 {
        return 0.0;
    }

    let duration_days = duration_ms as f64 / (1000.0 * 60.0 * 60.0 * 24.0);
    if duration_days <= 0.0 {
        return 0.0;
    }

    timestamps.len() as f64 / duration_days
}

/// Calculates the standard deviation of the prices.
pub fn calculate_standard_deviation(data: &[i32]) -> f64 {
    if data.len() < 2 {
        return 0.0;
    }
    let mean = data.iter().map(|&x| x as f64).sum::<f64>() / data.len() as f64;
    let variance = data.iter().map(|&x| {
        let diff = mean - x as f64;
        diff * diff
    }).sum::<f64>() / (data.len() - 1) as f64; // Sample standard deviation (N-1)
    variance.sqrt()
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
        let now = 100_000_000;
        let day_ms = 24 * 60 * 60 * 1000;

        // 1 sale 1 day ago
        let timestamps = vec![now - day_ms];
        let velocity = calculate_sales_velocity(&timestamps, now);
        assert!((velocity - 1.0).abs() < 0.001);

        // 2 sales, one 1 day ago, one 0.5 days ago
        let timestamps = vec![now - day_ms, now - (day_ms / 2)];
        let velocity = calculate_sales_velocity(&timestamps, now);
        // Duration is 1 day. Count is 2. Velocity should be 2.0.
        assert!((velocity - 2.0).abs() < 0.001);

        // 10 sales in 10 days
        let timestamps = vec![now - 10 * day_ms; 10]; // duration will be 10 days
        let velocity = calculate_sales_velocity(&timestamps, now);
        assert!((velocity - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_standard_deviation() {
        let data = vec![10, 12, 23, 23, 16, 23, 21, 16];
        let std_dev = calculate_standard_deviation(&data);
        // Mean = 18.0
        // Variance (sample) = 192 / 7 ≈ 27.42857
        // StdDev ≈ 5.2372
        assert!((std_dev - 5.2372).abs() < 0.001);

        let data = vec![10, 10, 10];
        assert_eq!(calculate_standard_deviation(&data), 0.0);
    }
}
