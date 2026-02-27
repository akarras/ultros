/// Filters outliers using the Interquartile Range (IQR) method.
///
/// This function modifies the input slice in-place to partition the data such that
/// the returned sub-slice contains only the "inlier" values.
///
/// # Optimization
/// This implementation uses `select_nth_unstable` and partitioning to achieve O(N) complexity,
/// avoiding a full sort (O(N log N)).
///
/// # Returns
/// A sub-slice of the input `data` containing the filtered values.
///
/// # Note
/// The returned slice is **NOT** guaranteed to be sorted. The order of elements within the
/// returned slice is undefined. The input `data` will be reordered.
pub fn filter_outliers_iqr_in_place(data: &mut [i32]) -> &[i32] {
    if data.len() < 4 {
        return data;
    }

    let n = data.len();
    let q1_idx = n / 4;
    let q3_idx = n * 3 / 4;

    // 1. Select Q3. data[0..q3_idx] <= Q3 <= data[q3_idx+1..]
    let (_, q3_el, _) = data.select_nth_unstable(q3_idx);
    let q3 = *q3_el;

    // 2. Select Q1 within the left part. data[0..q1_idx] <= Q1 <= data[q1_idx+1..q3_idx]
    let (_, q1_el, _) = data[..q3_idx].select_nth_unstable(q1_idx);
    let q1 = *q1_el;

    let iqr = q3 - q1;
    let lower_bound = q1 as f64 - 1.5 * iqr as f64;
    let upper_bound = q3 as f64 + 1.5 * iqr as f64;

    // 3. Partition left tail (0..q1_idx). We want elements >= lower_bound at the end of this slice.
    // So we partition by condition "x < lower_bound" (invalid elements).
    // The valid elements will be at index `count` onwards.
    // Note: Rust's partition_in_place is unstable. We implement a simple swap-based partition.
    let left_slice = &mut data[..q1_idx];
    let invalid_count = {
        let mut i = 0;
        for j in 0..left_slice.len() {
            if (left_slice[j] as f64) < lower_bound {
                left_slice.swap(i, j);
                i += 1;
            }
        }
        i
    };
    let start_idx = invalid_count;

    // 4. Partition right tail (q3_idx+1..). We want elements <= upper_bound at the start of this slice.
    // So we partition by condition "x <= upper_bound" (valid elements).
    let right_slice = &mut data[q3_idx + 1..];
    let valid_count = {
        let mut i = 0;
        for j in 0..right_slice.len() {
            if (right_slice[j] as f64) <= upper_bound {
                right_slice.swap(i, j);
                i += 1;
            }
        }
        i
    };
    let end_idx = q3_idx + 1 + valid_count;

    &data[start_idx..end_idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Lcg {
        state: u64,
    }

    impl Lcg {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }

        fn next(&mut self) -> u32 {
            self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (self.state >> 32) as u32
        }

        fn range(&mut self, min: i32, max: i32) -> i32 {
            let range = (max - min) as u32;
            (self.next() % range) as i32 + min
        }
    }

    fn filter_outliers_iqr_in_place_reference(data: &mut [i32]) -> &[i32] {
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

        let start_idx = data.partition_point(|&x| (x as f64) < lower_bound);
        let end_idx = data.partition_point(|&x| (x as f64) <= upper_bound);

        &data[start_idx..end_idx]
    }

    #[test]
    fn test_filter_outliers_iqr_in_place() {
        let mut data = vec![1, 2, 3, 4, 5, 100];
        let filtered = filter_outliers_iqr_in_place(&mut data);
        let mut filtered = filtered.to_vec();
        filtered.sort();
        assert_eq!(filtered, vec![1, 2, 3, 4, 5]);

        let mut data = vec![1, 2, 3];
        let filtered = filter_outliers_iqr_in_place(&mut data);
        let mut filtered = filtered.to_vec();
        filtered.sort();
        assert_eq!(filtered, vec![1, 2, 3]);

        let mut data = vec![100, 1, 2, 3, 4, 5];
        let filtered = filter_outliers_iqr_in_place(&mut data);
        let mut filtered = filtered.to_vec();
        filtered.sort();
        assert_eq!(filtered, vec![1, 2, 3, 4, 5]);

        let mut data = vec![100, 5, 4, 3, 2, 1];
        let filtered = filter_outliers_iqr_in_place(&mut data);
        let mut filtered = filtered.to_vec();
        filtered.sort();
        assert_eq!(filtered, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_filter_outliers_iqr_in_place_random() {
        let mut rng = Lcg::new(12345);
        for _ in 0..100 {
            let mut data = Vec::new();
            let len = rng.range(0, 100) as usize;
            for _ in 0..len {
                data.push(rng.range(0, 1000));
            }

            let mut d1 = data.clone();
            let f1 = filter_outliers_iqr_in_place_reference(&mut d1);
            let sum1: i64 = f1.iter().map(|&x| x as i64).sum();
            let len1 = f1.len();

            let mut d2 = data.clone();
            let f2 = filter_outliers_iqr_in_place(&mut d2);
            let sum2: i64 = f2.iter().map(|&x| x as i64).sum();
            let len2 = f2.len();

            assert_eq!(len1, len2, "Length mismatch for input: {:?}", data);
            assert_eq!(sum1, sum2, "Sum mismatch for input: {:?}", data);
        }
    }
}
