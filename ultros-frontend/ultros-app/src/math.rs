pub fn filter_outliers_iqr_in_place(data: &mut [i32]) -> &[i32] {
    if data.len() < 4 {
        return data;
    }

    data.sort_unstable();

    let n = data.len();
    let q1 = data[n / 4];
    let q3 = data[n * 3 / 4];
    let iqr = q3 - q1;

    let lower_bound = q1 as f64 - 1.5 * iqr as f64;
    let upper_bound = q3 as f64 + 1.5 * iqr as f64;

    // Find the first element >= lower_bound
    let start_idx = data.partition_point(|&x| (x as f64) < lower_bound);
    // Find the first element > upper_bound
    let end_idx = data.partition_point(|&x| (x as f64) <= upper_bound);

    &data[start_idx..end_idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_outliers_iqr_in_place() {
        let mut data = vec![1, 2, 3, 4, 5, 100];
        let filtered = filter_outliers_iqr_in_place(&mut data);
        assert_eq!(filtered, &[1, 2, 3, 4, 5]);

        let mut data = vec![1, 2, 3];
        let filtered = filter_outliers_iqr_in_place(&mut data);
        assert_eq!(filtered, &[1, 2, 3]);

        // Note: the function sorts the input array in place, so order is not preserved relative to input
        // but the output slice is sorted.
        let mut data = vec![100, 1, 2, 3, 4, 5];
        let filtered = filter_outliers_iqr_in_place(&mut data);
        assert_eq!(filtered, &[1, 2, 3, 4, 5]);

        let mut data = vec![100, 5, 4, 3, 2, 1];
        let filtered = filter_outliers_iqr_in_place(&mut data);
        assert_eq!(filtered, &[1, 2, 3, 4, 5]);
    }
}
