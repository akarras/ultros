//! Route-level controls use the same editor as metric filters while retaining
//! their pre-calculation URL contract. New grid hosts provide this registry in
//! the owner shared by ControlBar and MarketGrid/QueryGrid.
use crate::components::virtual_grid::{
    ColumnFilter,
    registry::{FilterAlias, FilterRegistry},
};
use crate::i18n::*;
use leptos::prelude::*;

pub fn register_filters(
    aliases: Vec<FilterAlias>,
    controls: Signal<Vec<ColumnFilter>>,
) -> FilterRegistry {
    FilterRegistry::provide(aliases, controls)
}

pub fn toggle_control(key: &'static str, label: String) -> ColumnFilter {
    let mut filter = ColumnFilter::new(key, label, false);
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    filter.options = vec![
        ("true", t_string!(i18n, toolbar_pill_on).to_string()),
        ("false", t_string!(i18n, toolbar_pill_off).to_string()),
    ];
    filter
}

/// A calculation input: its URL key is read before pricing, and Clear all
/// preserves the selected basis just as it preserves the market window.
pub fn price_control(
    key: &'static str,
    label: String,
    window: super::window::MarketWindow,
    listing_label: String,
) -> ColumnFilter {
    use super::stat_columns::{StatKind, stat_label};
    let mut filter = ColumnFilter::new(key, label, false);
    filter.default_value = Some("listing-min".into());
    filter.clear_with_filters = false;
    filter.options = vec![
        ("listing-min", listing_label),
        ("sale-min", stat_label(StatKind::Min, window.selected.get())),
        (
            "sale-median",
            stat_label(StatKind::Median, window.selected.get()),
        ),
        (
            "sale-avg",
            stat_label(StatKind::Average, window.selected.get()),
        ),
    ];
    filter
}

/// Duration filters use seconds without truncating subsecond boundaries.
/// Negative or absent ages are missing, matching the legacy duration controls.
pub fn duration_value(
    duration: Option<chrono::Duration>,
) -> crate::components::virtual_grid::metrics::GridValue {
    use crate::components::virtual_grid::metrics::GridValue;
    duration
        .and_then(|duration| duration.to_std().ok())
        .map(|duration| GridValue::Number(duration.as_secs_f64()))
        .unwrap_or(GridValue::Missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::virtual_grid::metrics::{FilterOp, GridValue, MetricFilter};

    #[test]
    fn duration_metrics_preserve_fractional_boundaries_and_negative_absence() {
        let filter = MetricFilter {
            op: FilterOp::Lte,
            value: "1.5".into(),
        };
        for (milliseconds, expected) in [(1499, true), (1500, true), (1501, false), (-1, false)] {
            let value = duration_value(Some(chrono::Duration::milliseconds(milliseconds)));
            assert_eq!(filter.matches(&value, false), Some(expected));
        }
        assert_eq!(duration_value(None), GridValue::Missing);
        assert_eq!(
            duration_value(Some(chrono::Duration::milliseconds(-1))),
            GridValue::Missing
        );
    }
}
