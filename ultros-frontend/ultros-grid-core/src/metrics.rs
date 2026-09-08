//! Typed column queries, independent of cell markup and market-data providers.
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq)]
pub enum GridValue {
    Number(f64),
    Text(String),
    Set(Vec<String>),
    Missing,
    Pending,
    /// The provider finished with an error; no verdict about this row exists.
    Unavailable,
}

impl GridValue {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Missing | Self::Pending | Self::Unavailable)
            || matches!(self, Self::Number(n) if !n.is_finite())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Number,
    Text,
    Mixed,
}

pub type ValueExtractor<T> = Arc<dyn Fn(&T) -> GridValue + Send + Sync>;
pub type TierExtractor<T> = Arc<dyn Fn(&T) -> u8 + Send + Sync>;

pub struct GridMetric<T> {
    pub id: &'static str,
    pub kind: ValueKind,
    /// Incomplete data may filter known values, but cannot rank all rows.
    pub partial: bool,
    pub value: ValueExtractor<T>,
    pub tier: Option<TierExtractor<T>>,
}

impl<T> Clone for GridMetric<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            kind: self.kind,
            partial: self.partial,
            value: self.value.clone(),
            tier: self.tier.clone(),
        }
    }
}

impl<T> GridMetric<T> {
    pub fn number(
        id: &'static str,
        value: impl Fn(&T) -> GridValue + Send + Sync + 'static,
    ) -> Self {
        Self {
            id,
            kind: ValueKind::Number,
            partial: false,
            value: Arc::new(value),
            tier: None,
        }
    }
    pub fn text(id: &'static str, value: impl Fn(&T) -> GridValue + Send + Sync + 'static) -> Self {
        Self {
            id,
            kind: ValueKind::Text,
            partial: false,
            value: Arc::new(value),
            tier: None,
        }
    }
    pub fn partial(mut self) -> Self {
        self.partial = true;
        self
    }
    pub fn mixed(
        id: &'static str,
        value: impl Fn(&T) -> GridValue + Send + Sync + 'static,
    ) -> Self {
        let mut metric = Self::number(id, value);
        metric.kind = ValueKind::Mixed;
        metric
    }
    /// Keep incompletely priced rows below fully priced rows under either direction.
    pub fn tier(mut self, value: impl Fn(&T) -> u8 + Send + Sync + 'static) -> Self {
        self.tier = Some(Arc::new(value));
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FilterOp {
    #[default]
    Eq,
    Ne,
    Contains,
    Gte,
    Lte,
    Missing,
    Present,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricFilter {
    pub op: FilterOp,
    #[serde(default)]
    pub value: String,
}

pub type MetricFilters = BTreeMap<String, MetricFilter>;

pub fn parse_filters(raw: Option<&str>) -> MetricFilters {
    raw.filter(|s| s.len() <= 16_384)
        .and_then(|s| serde_json::from_str::<MetricFilters>(s).ok())
        .filter(|m| m.len() <= 128)
        .unwrap_or_default()
}

pub fn active_metric_columns(raw: Option<&str>) -> HashSet<String> {
    parse_filters(raw).into_keys().collect()
}

impl MetricFilter {
    pub fn valid(&self, kind: ValueKind) -> bool {
        if matches!(self.op, FilterOp::Missing | FilterOp::Present) {
            return true;
        }
        match kind {
            ValueKind::Number => {
                !matches!(self.op, FilterOp::Contains)
                    && self.value.parse::<f64>().is_ok_and(f64::is_finite)
            }
            ValueKind::Text => {
                !matches!(self.op, FilterOp::Gte | FilterOp::Lte) && !self.value.trim().is_empty()
            }
            ValueKind::Mixed => {
                if matches!(self.op, FilterOp::Gte | FilterOp::Lte) {
                    self.value.parse::<f64>().is_ok_and(f64::is_finite)
                } else {
                    !self.value.trim().is_empty()
                }
            }
        }
    }

    /// `None` means this row cannot yet be evaluated. Keep it visible and count
    /// it in the coverage notice, so a lazy feed can still fetch its subject.
    pub fn matches(&self, value: &GridValue, partial: bool) -> Option<bool> {
        if matches!(value, GridValue::Pending | GridValue::Unavailable) {
            return None;
        }
        if matches!(self.op, FilterOp::Missing) {
            return Some(value.is_unknown());
        }
        if matches!(self.op, FilterOp::Present) {
            return Some(!value.is_unknown());
        }
        if value.is_unknown() {
            return if partial { None } else { Some(false) };
        }
        let equal = match value {
            GridValue::Number(n) => {
                let Some(rhs) = self.value.parse::<f64>().ok().filter(|v| v.is_finite()) else {
                    return Some(self.op == FilterOp::Ne);
                };
                return Some(match self.op {
                    FilterOp::Eq => *n == rhs,
                    FilterOp::Ne => *n != rhs,
                    FilterOp::Gte => *n >= rhs,
                    FilterOp::Lte => *n <= rhs,
                    _ => false,
                });
            }
            GridValue::Text(text) => text.to_lowercase() == self.value.to_lowercase(),
            GridValue::Set(values) => values
                .iter()
                .any(|v| v.to_lowercase() == self.value.to_lowercase()),
            _ => return None,
        };
        Some(match self.op {
            FilterOp::Eq => equal,
            FilterOp::Ne => !equal,
            FilterOp::Contains => {
                let needle = self.value.to_lowercase();
                match value {
                    GridValue::Text(text) => text.to_lowercase().contains(&needle),
                    GridValue::Set(values) => {
                        values.iter().any(|v| v.to_lowercase().contains(&needle))
                    }
                    _ => false,
                }
            }
            _ => false,
        })
    }
}

/// Missing values stay last in either direction. Equal values retain upstream
/// stable ordering; changing enrichment never changes row identities.
pub fn compare_values(a: &GridValue, b: &GridValue, ascending: bool) -> Ordering {
    match (a.is_unknown(), b.is_unknown()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    let order = match (a, b) {
        (GridValue::Number(a), GridValue::Number(b)) => a.total_cmp(b),
        (GridValue::Text(a), GridValue::Text(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
        (GridValue::Set(a), GridValue::Set(b)) => a.cmp(b),
        (GridValue::Number(_), _) => Ordering::Less,
        (_, GridValue::Number(_)) => Ordering::Greater,
        (GridValue::Text(_), _) => Ordering::Less,
        (_, GridValue::Text(_)) => Ordering::Greater,
        _ => Ordering::Equal,
    };
    if ascending { order } else { order.reverse() }
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryResult<T> {
    /// None borrows the upstream rows unchanged; Some owns an active query's result.
    pub rows: Option<Vec<T>>,
    pub lacking_data: usize,
    pub sort_pending: bool,
}

pub fn query_rows<T: Clone>(
    rows: &[T],
    metrics: &[GridMetric<T>],
    filters: &MetricFilters,
    sort_id: Option<&str>,
    ascending: bool,
) -> QueryResult<T> {
    let active: Vec<_> = metrics
        .iter()
        .filter_map(|m| {
            filters
                .get(m.id)
                .filter(|f| f.valid(m.kind))
                .map(|f| (m, f))
        })
        .collect();
    let sort_metric = metrics.iter().find(|m| Some(m.id) == sort_id && !m.partial);
    if active.is_empty() && sort_metric.is_none() {
        return QueryResult {
            rows: None,
            lacking_data: 0,
            sort_pending: false,
        };
    }
    let mut lacking_data = 0;
    let mut kept = Vec::new();
    for row in rows {
        let mut unknown = false;
        let mut keep = true;
        for (metric, filter) in &active {
            match filter.matches(&(metric.value)(row), metric.partial) {
                Some(false) => keep = false,
                None => unknown = true,
                _ => {}
            }
        }
        if keep {
            lacking_data += usize::from(unknown);
            kept.push(row.clone());
        }
    }
    let mut sort_pending = false;
    if let Some(metric) = sort_metric {
        // Compute each key once: never re-read a reactive provider O(n log n).
        let mut decorated: Vec<_> = kept
            .into_iter()
            .map(|row| {
                let value = (metric.value)(&row);
                (row, value)
            })
            .collect();
        sort_pending = decorated
            .iter()
            .any(|(_, value)| matches!(value, GridValue::Pending | GridValue::Unavailable));
        if !sort_pending {
            decorated.sort_by(|(row_a, a), (row_b, b)| {
                metric
                    .tier
                    .as_ref()
                    .map(|tier| tier(row_a).cmp(&tier(row_b)))
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| compare_values(a, b, ascending))
            });
        }
        kept = decorated.into_iter().map(|(row, _)| row).collect();
    }
    QueryResult {
        rows: Some(kept),
        lacking_data,
        sort_pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untouched_queries_never_clone_rows_or_read_providers() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct CountedRow(Arc<AtomicUsize>);
        impl Clone for CountedRow {
            fn clone(&self) -> Self {
                self.0.fetch_add(1, Ordering::Relaxed);
                Self(self.0.clone())
            }
        }
        let clones = Arc::new(AtomicUsize::new(0));
        let reads = Arc::new(AtomicUsize::new(0));
        let rows = (0..600)
            .map(|_| CountedRow(clones.clone()))
            .collect::<Vec<_>>();
        let bulk_reads = reads.clone();
        let partial_reads = reads.clone();
        let metrics = vec![
            GridMetric::number("value", move |_: &CountedRow| {
                bulk_reads.fetch_add(1, Ordering::Relaxed);
                GridValue::Number(1.0)
            }),
            GridMetric::number("partial", move |_: &CountedRow| {
                partial_reads.fetch_add(1, Ordering::Relaxed);
                GridValue::Pending
            })
            .partial(),
        ];
        let ignored_filters = BTreeMap::from([
            (
                "value".into(),
                MetricFilter {
                    op: FilterOp::Gte,
                    value: "invalid".into(),
                },
            ),
            (
                "unregistered".into(),
                MetricFilter {
                    op: FilterOp::Eq,
                    value: "1".into(),
                },
            ),
        ]);
        for filters in [&BTreeMap::new(), &ignored_filters] {
            for sort in [None, Some("unregistered"), Some("partial")] {
                let result = query_rows(&rows, &metrics, filters, sort, true);
                assert!(result.rows.is_none());
                assert_eq!(result.lacking_data, 0);
                assert!(!result.sort_pending);
            }
        }
        assert_eq!(clones.load(Ordering::Relaxed), 0);
        assert_eq!(reads.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn sorting_and_filtering_preserve_every_match_beyond_former_result_caps() {
        let rows = (0..600).rev().collect::<Vec<_>>();
        let metric =
            GridMetric::number("value", |value: &i32| GridValue::Number(f64::from(*value)));
        for minimum in [None, Some(250)] {
            let filters = minimum
                .map(|minimum| {
                    BTreeMap::from([(
                        "value".into(),
                        MetricFilter {
                            op: FilterOp::Gte,
                            value: minimum.to_string(),
                        },
                    )])
                })
                .unwrap_or_default();
            for ascending in [true, false] {
                let result = query_rows(
                    &rows,
                    std::slice::from_ref(&metric),
                    &filters,
                    Some("value"),
                    ascending,
                );
                let mut expected = (minimum.unwrap_or(0)..600).collect::<Vec<_>>();
                if !ascending {
                    expected.reverse();
                }
                assert!(expected.len() > 250);
                assert_eq!(result.rows, Some(expected));
            }
        }
    }

    #[test]
    fn mixed_hop_values_support_status_and_numeric_filters() {
        let rows = vec![
            GridValue::Number(0.0),
            GridValue::Text("needed".into()),
            GridValue::Number(1_000.0),
            GridValue::Missing,
        ];
        let metric = GridMetric::mixed("hop", |value: &GridValue| value.clone());
        for (op, value, expected) in [
            (FilterOp::Eq, "NEEDED", vec![rows[1].clone()]),
            (FilterOp::Contains, "need", vec![rows[1].clone()]),
            (
                FilterOp::Ne,
                "needed",
                vec![rows[0].clone(), rows[2].clone()],
            ),
            (FilterOp::Gte, "500", vec![rows[2].clone()]),
            (FilterOp::Lte, "0", vec![rows[0].clone()]),
            (FilterOp::Eq, "1000", vec![rows[2].clone()]),
        ] {
            let filter = MetricFilter {
                op,
                value: value.into(),
            };
            assert!(filter.valid(ValueKind::Mixed));
            let result = query_rows(
                &rows,
                std::slice::from_ref(&metric),
                &BTreeMap::from([("hop".into(), filter)]),
                None,
                true,
            );
            assert_eq!(result.rows, Some(expected), "{op:?} {value}");
            assert_eq!(result.lacking_data, 0);
        }
        assert!(
            !MetricFilter {
                op: FilterOp::Gte,
                value: "needed".into(),
            }
            .valid(ValueKind::Mixed)
        );
    }

    #[test]
    fn provider_failure_keeps_rows_without_claiming_their_values_are_missing() {
        let rows = vec![
            GridValue::Number(1.0),
            GridValue::Unavailable,
            GridValue::Missing,
            GridValue::Number(10.0),
        ];
        let metric = GridMetric::number("median", |value: &GridValue| value.clone());
        for (op, value, expected) in [
            (FilterOp::Gte, "5", vec![rows[1].clone(), rows[3].clone()]),
            (
                FilterOp::Missing,
                "",
                vec![rows[1].clone(), rows[2].clone()],
            ),
            (
                FilterOp::Present,
                "",
                vec![rows[0].clone(), rows[1].clone(), rows[3].clone()],
            ),
        ] {
            let result = query_rows(
                &rows,
                std::slice::from_ref(&metric),
                &BTreeMap::from([(
                    "median".into(),
                    MetricFilter {
                        op,
                        value: value.into(),
                    },
                )]),
                None,
                true,
            );
            assert_eq!(result.rows, Some(expected), "{op:?}");
            assert_eq!(result.lacking_data, 1, "{op:?}");
        }
        let result = query_rows(&rows, &[metric], &BTreeMap::new(), Some("median"), true);
        assert!(result.sort_pending);
        assert_eq!(
            result.rows,
            Some(rows),
            "failed bulk data must not rank only known rows"
        );
    }

    #[test]
    fn coverage_tiers_stay_first_and_equal_values_stay_stable_in_both_directions() {
        // (identity, coverage tier, cost): incomplete recipes must stay behind
        // fully priced recipes even when their reported cost is smaller.
        let rows = vec![
            ("partial", 1, 1.0),
            ("tie-a", 0, 10.0),
            ("low", 0, 5.0),
            ("tie-b", 0, 10.0),
        ];
        let metric = GridMetric::number("cost", |row: &(&str, u8, f64)| GridValue::Number(row.2))
            .tier(|row| row.1);
        for (ascending, expected) in [
            (true, vec!["low", "tie-a", "tie-b", "partial"]),
            (false, vec!["tie-a", "tie-b", "low", "partial"]),
        ] {
            let result = query_rows(
                &rows,
                std::slice::from_ref(&metric),
                &BTreeMap::new(),
                Some("cost"),
                ascending,
            );
            assert_eq!(
                result
                    .rows
                    .unwrap()
                    .into_iter()
                    .map(|row| row.0)
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn filters_search_beyond_old_caps_and_preserve_partial_unknowns() {
        let metric = GridMetric::number("value", |n: &i32| {
            if *n == 151 {
                GridValue::Pending
            } else {
                GridValue::Number(*n as f64)
            }
        })
        .partial();
        let filters = BTreeMap::from([(
            "value".into(),
            MetricFilter {
                op: FilterOp::Gte,
                value: "150".into(),
            },
        )]);
        let result = query_rows(
            &(0..153).collect::<Vec<_>>(),
            &[metric],
            &filters,
            Some("value"),
            false,
        );
        assert_eq!(result.rows, Some(vec![150, 151, 152]));
        assert_eq!(result.lacking_data, 1);
    }
    #[test]
    fn missing_is_distinct_from_zero_and_stays_last_in_both_directions() {
        let m = GridMetric::number("n", |v: &GridValue| v.clone());
        for ascending in [true, false] {
            let r = query_rows(
                &[
                    GridValue::Missing,
                    GridValue::Number(0.0),
                    GridValue::Number(5.0),
                ],
                std::slice::from_ref(&m),
                &BTreeMap::new(),
                Some("n"),
                ascending,
            );
            assert_eq!(r.rows.as_ref().unwrap().last(), Some(&GridValue::Missing));
        }
        let f = MetricFilter {
            op: FilterOp::Missing,
            value: String::new(),
        };
        assert_eq!(f.matches(&GridValue::Number(0.0), false), Some(false));
        assert_eq!(f.matches(&GridValue::Missing, false), Some(true));
        assert_eq!(f.matches(&GridValue::Pending, true), None);
    }
    #[test]
    fn bulk_pending_does_not_produce_a_partial_global_sort() {
        let rows = vec![
            GridValue::Number(10.0),
            GridValue::Pending,
            GridValue::Number(1.0),
        ];
        let r = query_rows(
            &rows,
            &[GridMetric::number("n", |v: &GridValue| v.clone())],
            &BTreeMap::new(),
            Some("n"),
            true,
        );
        assert!(r.sort_pending);
        assert_eq!(r.rows, Some(rows));
    }
    #[test]
    fn world_sets_filter_by_membership_and_malformed_queries_are_ignored() {
        let f = MetricFilter {
            op: FilterOp::Eq,
            value: "cactuar".into(),
        };
        assert_eq!(
            f.matches(
                &GridValue::Set(vec!["Gilgamesh".into(), "Cactuar".into()]),
                false
            ),
            Some(true)
        );
        assert!(parse_filters(Some("invalid")).is_empty());
        assert!(
            !MetricFilter {
                op: FilterOp::Gte,
                value: "NaN".into()
            }
            .valid(ValueKind::Number)
        );
    }
}
