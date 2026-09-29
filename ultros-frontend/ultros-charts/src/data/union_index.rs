//! Shared time index for the comparison grid: the sorted set of all bucket
//! timestamps across the visible series, with each series mapped onto it by
//! position (`None` where a series has no bucket at that timestamp).
//!
//! Because every series in a `PriceSeries` response came from one query with
//! one bucket width, the union index is exact — no interpolation or
//! snapping. This generalises what `HoverModel::buckets` already does; here
//! the index is shared state owned above the grid cells, which is what makes
//! one crosshair line up across every cell.

use chrono::NaiveDateTime;
use ultros_api_types::price_series::PriceBucket;

#[derive(Clone, Debug, PartialEq)]
pub struct UnionIndex {
    /// Sorted, distinct bucket timestamps across all indexed series.
    pub timestamps: Vec<NaiveDateTime>,
    /// `positions[series][union_pos]` = index into that series' bucket vec,
    /// `None` where the series has no bucket at that timestamp.
    pub positions: Vec<Vec<Option<usize>>>,
}

impl UnionIndex {
    pub fn is_empty(&self) -> bool {
        self.timestamps.is_empty()
    }

    /// Bucket of series `s` at union position `i`, if any.
    pub fn bucket<'a>(
        &self,
        series_buckets: &'a [PriceBucket],
        s: usize,
        i: usize,
    ) -> Option<&'a PriceBucket> {
        let idx = (*self.positions.get(s)?.get(i)?)?;
        series_buckets.get(idx)
    }
}

/// Build the union index over the given series' bucket slices (callers pass
/// only VISIBLE series — hidden ones must not widen the index). Relies on
/// each series' buckets being sorted by `ts` ascending, which the server
/// guarantees (`PriceSeriesEntry::buckets` doc).
pub fn build_union_index(series: &[&[PriceBucket]]) -> UnionIndex {
    let mut timestamps: Vec<NaiveDateTime> =
        series.iter().flat_map(|b| b.iter().map(|x| x.ts)).collect();
    timestamps.sort_unstable();
    timestamps.dedup();

    let positions = series
        .iter()
        .map(|buckets| {
            // Both sides sorted ascending: single merge pass per series.
            let mut out = vec![None; timestamps.len()];
            let mut bi = 0usize;
            for (ui, ts) in timestamps.iter().enumerate() {
                if bi < buckets.len() && buckets[bi].ts == *ts {
                    out[ui] = Some(bi);
                    bi += 1;
                }
            }
            out
        })
        .collect();

    UnionIndex {
        timestamps,
        positions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::bucket;

    fn buckets_at(secs: &[i64]) -> Vec<PriceBucket> {
        secs.iter()
            .map(|s| bucket(*s, 100, 120, 90, 105, 2))
            .collect()
    }

    #[test]
    fn union_holds_every_distinct_timestamp_once_sorted() {
        let a = buckets_at(&[100, 300, 500]);
        let b = buckets_at(&[200, 300, 700]);
        let u = build_union_index(&[&a, &b]);
        let secs: Vec<i64> = u
            .timestamps
            .iter()
            .map(|t| t.and_utc().timestamp())
            .collect();
        assert_eq!(secs, vec![100, 200, 300, 500, 700]);
        // a maps to positions 0, 2, 3 with gaps at 1, 4
        assert_eq!(u.positions[0], vec![Some(0), None, Some(1), Some(2), None]);
        // b maps to positions 1, 2, 4
        assert_eq!(u.positions[1], vec![None, Some(0), Some(1), None, Some(2)]);
    }

    #[test]
    fn strict_subset_series_maps_without_shifting() {
        let full = buckets_at(&[100, 200, 300, 400]);
        let sub = buckets_at(&[200, 400]);
        let u = build_union_index(&[&full, &sub]);
        assert_eq!(u.timestamps.len(), 4);
        assert_eq!(u.positions[1], vec![None, Some(0), None, Some(1)]);
        // Round-trip through the accessor
        assert_eq!(
            u.bucket(&sub, 1, 1).map(|b| b.ts),
            Some(sub[0].ts),
            "accessor resolves union position to the right bucket"
        );
        assert!(u.bucket(&sub, 1, 0).is_none());
    }

    #[test]
    fn empty_input_yields_empty_index() {
        let u = build_union_index(&[]);
        assert!(u.is_empty());
        assert!(u.positions.is_empty());
    }
}
