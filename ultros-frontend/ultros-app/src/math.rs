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

pub fn mean(data: &[i32]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let sum: i64 = data.iter().map(|&x| x as i64).sum();
    sum as f64 / data.len() as f64
}

pub fn standard_deviation(data: &[i32]) -> f64 {
    if data.len() < 2 {
        return 0.0;
    }
    let m = mean(data);
    let variance: f64 = data
        .iter()
        .map(|&value| {
            let diff = m - (value as f64);
            diff * diff
        })
        .sum::<f64>()
        / (data.len() - 1) as f64; // Sample standard deviation
    variance.sqrt()
}

pub fn coefficient_of_variation(data: &[i32]) -> f64 {
    let m = mean(data);
    if m == 0.0 {
        return 0.0;
    }
    let s = standard_deviation(data);
    s / m
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
    fn test_mean() {
        let data = vec![1, 2, 3, 4, 5];
        assert_eq!(mean(&data), 3.0);
        assert_eq!(mean(&[]), 0.0);
    }

    #[test]
    fn test_standard_deviation() {
        // Population: 2, 4, 4, 4, 5, 5, 7, 9
        // Mean: 5
        // Sample Std Dev should be ~2.138
        let data = vec![2, 4, 4, 4, 5, 5, 7, 9];
        let std_dev = standard_deviation(&data);
        assert!((std_dev - 2.138).abs() < 0.001);

        assert_eq!(standard_deviation(&[1]), 0.0);
        assert_eq!(standard_deviation(&[]), 0.0);
    }

    #[test]
    fn test_coefficient_of_variation() {
        // Mean 5, StdDev ~2.138. CV should be ~0.427
        let data = vec![2, 4, 4, 4, 5, 5, 7, 9];
        let cv = coefficient_of_variation(&data);
        assert!((cv - 0.427).abs() < 0.001);
    }
}
