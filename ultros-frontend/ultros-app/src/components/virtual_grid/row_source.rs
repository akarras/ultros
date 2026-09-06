//! Borrow the complete upstream rows or one cached query result without copying
//! a table every time the virtualized window, counts, or cells read it.
use leptos::prelude::*;

use super::metrics::QueryResult;

pub struct RowSource<T: Send + Sync + 'static> {
    original: Signal<Vec<T>>,
    result: Memo<QueryResult<T>>,
}

impl<T: Send + Sync + 'static> Copy for RowSource<T> {}

impl<T: Send + Sync + 'static> Clone for RowSource<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Send + Sync + 'static> RowSource<T> {
    pub fn new(original: Signal<Vec<T>>, result: Memo<QueryResult<T>>) -> Self {
        Self { original, result }
    }

    pub fn with<R>(&self, read: impl FnOnce(&Vec<T>) -> R) -> R {
        self.result.with(|result| match &result.rows {
            Some(rows) => read(rows),
            None => self.original.with(read),
        })
    }

    pub fn with_untracked<R>(&self, read: impl FnOnce(&Vec<T>) -> R) -> R {
        self.result.with_untracked(|result| match &result.rows {
            Some(rows) => read(rows),
            None => self.original.with_untracked(read),
        })
    }

    /// Use only when a consumer needs ownership, such as a scheduled autofit.
    #[cfg(any(feature = "hydrate", test))]
    pub fn get_untracked(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.with_untracked(Clone::clone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::virtual_grid::metrics::{GridMetric, GridValue, query_rows};
    use std::{
        collections::BTreeMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    struct CountedRow {
        value: usize,
        clones: Arc<AtomicUsize>,
    }

    impl Clone for CountedRow {
        fn clone(&self) -> Self {
            self.clones.fetch_add(1, Ordering::Relaxed);
            Self {
                value: self.value,
                clones: self.clones.clone(),
            }
        }
    }

    impl PartialEq for CountedRow {
        fn eq(&self, other: &Self) -> bool {
            self.value == other.value
        }
    }

    #[test]
    fn passthrough_reads_borrow_upstream_and_only_explicit_owned_read_clones() {
        let owner = Owner::new();
        owner.with(|| {
            let clones = Arc::new(AtomicUsize::new(0));
            let original = RwSignal::new(
                (0..300)
                    .map(|value| CountedRow {
                        value,
                        clones: clones.clone(),
                    })
                    .collect::<Vec<_>>(),
            );
            let result = Memo::new(move |_| {
                original.with(|rows| query_rows(rows, &[], &BTreeMap::new(), None, true))
            });
            let source = RowSource::new(original.into(), result);
            for _ in 0..10 {
                assert_eq!(source.with(Vec::len), 300);
                assert_eq!(source.with_untracked(|rows| rows[299].value), 299);
            }
            // The query remains equal (None), so the borrowing consumer must
            // also subscribe to the upstream rows instead of just the query.
            let first = Memo::new(move |_| source.with(|rows| rows[0].value));
            assert_eq!(first.get(), 0);
            original.update(|rows| rows[0].value = 500);
            assert_eq!(first.get(), 500);
            assert!(result.with(|result| result.rows.is_none()));
            assert_eq!(clones.load(Ordering::Relaxed), 0);
            let owned = source.get_untracked();
            assert_eq!(owned.len(), 300);
            assert_eq!(clones.load(Ordering::Relaxed), 300);
        });
    }

    #[test]
    fn active_query_runs_once_per_invalidation_and_reads_borrow_its_cached_rows() {
        let owner = Owner::new();
        owner.with(|| {
            let clones = Arc::new(AtomicUsize::new(0));
            let reads = Arc::new(AtomicUsize::new(0));
            let original = RwSignal::new(
                (0..300)
                    .map(|value| CountedRow {
                        value,
                        clones: clones.clone(),
                    })
                    .collect::<Vec<_>>(),
            );
            let provider_reads = reads.clone();
            let metrics = vec![GridMetric::number("value", move |row: &CountedRow| {
                provider_reads.fetch_add(1, Ordering::Relaxed);
                GridValue::Number(row.value as f64)
            })];
            let result = Memo::new(move |_| {
                original
                    .with(|rows| query_rows(rows, &metrics, &BTreeMap::new(), Some("value"), false))
            });
            let source = RowSource::new(original.into(), result);
            for _ in 0..10 {
                assert_eq!(source.with(Vec::len), 300);
                assert_eq!(source.with_untracked(|rows| rows[0].value), 299);
            }
            assert_eq!(clones.load(Ordering::Relaxed), 300);
            assert_eq!(reads.load(Ordering::Relaxed), 300);

            original.update(|rows| rows[0].value = 400);
            for _ in 0..10 {
                assert_eq!(source.with(|rows| rows[0].value), 400);
                assert_eq!(source.with_untracked(Vec::len), 300);
            }
            assert_eq!(clones.load(Ordering::Relaxed), 600);
            assert_eq!(reads.load(Ordering::Relaxed), 600);
        });
    }
}
