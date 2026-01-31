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

pub fn calculate_mean(data: &[i32]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let sum: i64 = data.iter().map(|&x| x as i64).sum();
    sum as f64 / data.len() as f64
}

pub fn calculate_std_dev(data: &[i32], mean: f64) -> f64 {
    if data.len() < 2 {
        return 0.0;
    }
    let variance = data
        .iter()
        .map(|&value| {
            let diff = mean - (value as f64);
            diff * diff
        })
        .sum::<f64>()
        / (data.len() - 1) as f64; // Sample variance
    variance.sqrt()
}

pub fn calculate_coefficient_of_variation(data: &[i32]) -> f64 {
    let mean = calculate_mean(data);
    if mean == 0.0 {
        return 0.0;
    }
    let std_dev = calculate_std_dev(data, mean);
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
    fn test_mean_std_dev_cv() {
        let data = vec![10, 20, 30, 40, 50];
        let mean = calculate_mean(&data);
        assert_eq!(mean, 30.0);

        // Sample Std Dev of 10, 20, 30, 40, 50
        // Mean = 30
        // Variance = ((10-30)^2 + (20-30)^2 + 0 + (40-30)^2 + (50-30)^2) / 4
        // = (400 + 100 + 0 + 100 + 400) / 4 = 1000 / 4 = 250
        // Std Dev = sqrt(250) ≈ 15.811
        let std_dev = calculate_std_dev(&data, mean);
        assert!((std_dev - 15.811388).abs() < 0.0001);

        let cv = calculate_coefficient_of_variation(&data);
        assert!((cv - (15.811388 / 30.0)).abs() < 0.0001);

        // Test with empty/single
        assert_eq!(calculate_mean(&[]), 0.0);
        assert_eq!(calculate_std_dev(&[1], 1.0), 0.0);
        assert_eq!(calculate_coefficient_of_variation(&[1]), 0.0);
    }
}
