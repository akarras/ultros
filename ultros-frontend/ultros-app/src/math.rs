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

pub fn standard_deviation(data: &[i32]) -> f32 {
    if data.len() < 2 {
        return 0.0;
    }
    let sum: i64 = data.iter().map(|&x| x as i64).sum();
    let mean = sum as f32 / data.len() as f32;
    let variance = data
        .iter()
        .map(|&value| {
            let diff = mean - value as f32;
            diff * diff
        })
        .sum::<f32>()
        / (data.len() - 1) as f32;
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
    fn test_standard_deviation() {
        let data = vec![10, 12, 23, 23, 16, 23, 21, 16];
        let std_dev = standard_deviation(&data);
        // Mean = 18.
        // Variance = 26.
        // Std Dev = sqrt(26) = 5.099
        assert!((std_dev - 5.099).abs() < 0.001);

        let data = vec![100];
        assert_eq!(standard_deviation(&data), 0.0);
    }
}
