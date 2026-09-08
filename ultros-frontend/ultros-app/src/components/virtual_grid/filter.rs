use super::layout::ColumnFilter;
use crate::{components::app_link::use_location_or_default, i18n::*};
use leptos::prelude::*;
use leptos_router::params::ParamsMap;

/// Keys whose landing default means "unlimited", spelled as an explicit empty
/// value. Removing one outright would let `seed_query_default` put the default
/// back on the next navigation, so clearing them writes the empty string
/// instead — which is also why `grid-filter-active` tests for a non-empty
/// value rather than for the key's presence.
fn clears_to_empty(key: &str) -> bool {
    matches!(key, "next-sale" | "last-sold" | "min-sales")
}

/// Write the packed metric filters back into `gf`, dropping the param once
/// nothing is left in it.
fn write_metric_filters(query: &mut ParamsMap, filters: MetricFilters) {
    query.remove("gf");
    if !filters.is_empty() {
        query.insert("gf", serde_json::to_string(&filters).unwrap_or_default());
    }
}

/// `query` with every filter on one column cleared: metric filters drop out of
/// the packed `gf` map, plain filters drop their own key.
///
/// One call clears a whole column, which is what the header's clear button
/// needs — a column like Cost / unit carries four separate filters, and the
/// popover only offers them one at a time.
pub fn cleared_query(query: &ParamsMap, filters: &[ColumnFilter]) -> ParamsMap {
    let mut query = query.clone();
    let mut metrics = parse_filters(query.get("gf").as_deref());
    let mut touched_metrics = false;
    for filter in filters {
        if filter.metric.is_some() {
            touched_metrics |= metrics.remove(filter.key).is_some();
        } else {
            query.remove(filter.key);
            if clears_to_empty(filter.key) {
                query.insert(filter.key, String::new());
            }
        }
    }
    if touched_metrics {
        write_metric_filters(&mut query, metrics);
    }
    query
}

#[component]
pub fn ColumnFilterEditor(filter: ColumnFilter) -> impl IntoView {
    if let Some(kind) = filter.metric {
        return view! { <MetricFilterEditor column=filter.key label=filter.label kind/> }
            .into_any();
    }
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let location = use_location_or_default();
    let query = location.query;
    let key = filter.key;
    let value = RwSignal::new(query.with_untracked(|q| q.get(key).unwrap_or_default()));
    Effect::new(move |_| value.set(query.with(|q| q.get(key).unwrap_or_default())));
    #[cfg(feature = "hydrate")]
    let navigate = leptos_router::hooks::use_navigate();
    let commit = Callback::new(move |next: Option<String>| {
        let mut q = query.get_untracked();
        q.remove(key);
        if let Some(next) = next {
            q.insert(key, next);
        } else if clears_to_empty(key) {
            q.insert(key, String::new());
        }
        #[cfg(feature = "hydrate")]
        navigate(
            &format!(
                "{}{}",
                location.pathname.get_untracked(),
                q.to_query_string()
            ),
            leptos_router::NavigateOptions {
                replace: true,
                scroll: false,
                ..Default::default()
            },
        );
    });
    let options = filter.options;
    let step = if matches!(key, "min-sales" | "vel") {
        "any"
    } else {
        "1"
    };
    let max = (key == "sales"
        && location
            .pathname
            .get_untracked()
            .starts_with("/flip-finder"))
    .then_some("6");
    view! {
        <form class="grid-column-filter" data-filter=key on:submit=move |e| {
            e.prevent_default();
            commit.run(crate::components::filter_chip::committed_value(&value.get_untracked()));
        }>
            <label>
                <span>{filter.label}</span>
                {if options.is_empty() {
                    view! {<input type=if filter.numeric {"number"} else {"text"} step=step max=max
                        prop:value=move || value.get() on:input=move |e| value.set(event_target_value(&e))/>}.into_any()
                } else {
                    view! {<select prop:value=move || value.get() on:change=move |e| value.set(event_target_value(&e))>
                        <option value="">{t!(i18n, grid_filter_any)}</option>
                        {options.into_iter().map(|(value,label)| view! {<option value=value>{label}</option>}).collect_view()}
                    </select>}.into_any()
                }}
            </label>
            <div class="grid-menu-actions">
                <button type="submit">{t!(i18n, grid_filter_apply)}</button>
                <button type="button" on:click=move |_| commit.run(None)>{t!(i18n, grid_filter_clear)}</button>
            </div>
        </form>
    }.into_any()
}

use super::metrics::{FilterOp, MetricFilter, MetricFilters, ValueKind, parse_filters};

#[component]
pub fn MetricSortControls(column: &'static str) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let location = use_location_or_default();
    let href = move |dir: &str| {
        let mut q = location.query.get();
        q.insert("sort", format!("grid:{column}"));
        q.insert("dir", dir.to_string());
        format!("{}{}", location.pathname.get(), q.to_query_string())
    };
    view! {
        <div class="grid-menu-actions">
            <leptos_router::components::A href=move || href("asc") scroll=false>{t!(i18n,grid_query_ascending)}</leptos_router::components::A>
            <leptos_router::components::A href=move || href("desc") scroll=false>{t!(i18n,grid_query_descending)}</leptos_router::components::A>
        </div>
    }
}

#[component]
fn MetricFilterEditor(column: &'static str, label: String, kind: ValueKind) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let location = use_location_or_default();
    let query = location.query;
    let initial = parse_filters(query.with_untracked(|q| q.get("gf")).as_deref())
        .remove(column)
        .unwrap_or_else(|| MetricFilter {
            op: if kind == ValueKind::Number {
                FilterOp::Gte
            } else {
                FilterOp::Contains
            },
            value: String::new(),
        });
    let value = RwSignal::new(initial.value);
    let op = RwSignal::new(initial.op);
    let invalid = RwSignal::new(false);
    Effect::new(move |_| {
        let active = parse_filters(query.with(|q| q.get("gf")).as_deref());
        if let Some(filter) = active.get(column) {
            value.set(filter.value.clone());
            op.set(filter.op);
        } else {
            value.set(String::new());
        }
        invalid.set(false);
    });
    #[cfg(feature = "hydrate")]
    let navigate = leptos_router::hooks::use_navigate();
    let commit = Callback::new(move |clear: bool| {
        let mut q = query.get_untracked();
        let mut filters = parse_filters(q.get("gf").as_deref());
        if clear {
            filters.remove(column);
        } else {
            let filter = MetricFilter {
                op: op.get_untracked(),
                value: value.get_untracked().trim().to_string(),
            };
            if !filter.valid(kind) {
                invalid.set(true);
                return;
            }
            filters.insert(column.to_string(), filter);
        }
        write_metric_filters(&mut q, filters);
        #[cfg(feature = "hydrate")]
        navigate(
            &format!(
                "{}{}",
                location.pathname.get_untracked(),
                q.to_query_string()
            ),
            leptos_router::NavigateOptions {
                replace: true,
                scroll: false,
                ..Default::default()
            },
        );
    });
    let options = [
        (FilterOp::Eq, t_string!(i18n, grid_query_eq).to_string()),
        (FilterOp::Ne, t_string!(i18n, grid_query_ne).to_string()),
        (
            FilterOp::Contains,
            t_string!(i18n, grid_query_contains).to_string(),
        ),
        (FilterOp::Gte, t_string!(i18n, grid_query_gte).to_string()),
        (FilterOp::Lte, t_string!(i18n, grid_query_lte).to_string()),
        (
            FilterOp::Missing,
            t_string!(i18n, grid_query_missing).to_string(),
        ),
        (
            FilterOp::Present,
            t_string!(i18n, grid_query_present).to_string(),
        ),
    ];
    let token = |op: FilterOp| {
        serde_json::to_string(&op)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string()
    };
    view! {
        <form class="grid-column-filter" data-metric-filter=column on:submit=move |e| {e.prevent_default();commit.run(false);}>
            <label><span>{label}</span>
                <select aria-label=t_string!(i18n,grid_query_filter).to_string()
                    prop:value=move || token(op.get())
                    on:change=move |e| {if let Ok(next)=serde_json::from_value(serde_json::Value::String(event_target_value(&e))) {op.set(next);}}>
                    {options.into_iter().filter(|(op,_)| match kind {
                        ValueKind::Number => *op != FilterOp::Contains,
                        ValueKind::Text => !matches!(op,FilterOp::Gte|FilterOp::Lte),
                        ValueKind::Mixed => true,
                    }).map(|(op,label)|view! {<option value=token(op)>{label}</option>}).collect_view()}
                </select>
            </label>
            <input aria-label=t_string!(i18n,grid_query_value).to_string()
                type=if kind==ValueKind::Number {"number"} else {"text"} step="any"
                disabled=move || matches!(op.get(),FilterOp::Missing|FilterOp::Present)
                prop:value=move || value.get() on:input=move |e|value.set(event_target_value(&e))/>
            {move || invalid.get().then(||view! {<span role="alert">{t!(i18n,grid_query_invalid)}</span>})}
            <div class="grid-menu-actions">
                <button type="submit">{t!(i18n,grid_filter_apply)}</button>
                <button type="button" on:click=move |_|commit.run(true)>{t!(i18n,grid_filter_clear)}</button>
            </div>
        </form>
    }
}

#[cfg(test)]
mod tests {
    use super::super::metrics::ValueKind;
    use super::*;

    fn params(pairs: &[(&'static str, &str)]) -> ParamsMap {
        let mut q = ParamsMap::new();
        for (k, v) in pairs {
            q.insert(*k, (*v).to_string());
        }
        q
    }

    fn plain(key: &'static str) -> ColumnFilter {
        ColumnFilter::new(key, key.to_string(), false)
    }

    fn metric(key: &'static str) -> ColumnFilter {
        ColumnFilter::metric(key, key.to_string(), ValueKind::Number)
    }

    #[test]
    fn a_plain_filter_drops_its_key_and_leaves_the_rest_alone() {
        let q = cleared_query(
            &params(&[("world", "Gilgamesh"), ("sort", "profit")]),
            &[plain("world")],
        );
        assert_eq!(q.get("world"), None);
        assert_eq!(q.get("sort").as_deref(), Some("profit"));
    }

    #[test]
    fn an_unlimited_default_clears_to_an_explicit_empty_value() {
        // Removing the key outright would let the landing default seed itself
        // back in, so "cleared" has to be spelled out.
        let q = cleared_query(&params(&[("min-sales", "3")]), &[plain("min-sales")]);
        assert_eq!(q.get("min-sales").as_deref(), Some(""));
    }

    #[test]
    fn a_metric_filter_drops_out_of_gf_without_disturbing_its_neighbours() {
        let gf =
            r#"{"grid:profit":{"op":"gte","value":"100"},"grid:roi":{"op":"gte","value":"5"}}"#;
        let q = cleared_query(&params(&[("gf", gf)]), &[metric("grid:profit")]);
        let left = parse_filters(q.get("gf").as_deref());
        assert!(!left.contains_key("grid:profit"), "{left:?}");
        assert_eq!(left.get("grid:roi").map(|f| f.value.as_str()), Some("5"));
    }

    #[test]
    fn gf_disappears_once_its_last_filter_is_cleared() {
        let gf = r#"{"grid:profit":{"op":"gte","value":"100"}}"#;
        let q = cleared_query(&params(&[("gf", gf)]), &[metric("grid:profit")]);
        assert_eq!(q.get("gf"), None);
    }

    #[test]
    fn one_call_clears_every_filter_a_column_carries() {
        // Cost / unit carries four; the popover only offers them one at a time.
        let gf = r#"{"grid:cost":{"op":"lte","value":"9"}}"#;
        let q = cleared_query(
            &params(&[
                ("gf", gf),
                ("cost-basis", "listing"),
                ("subcrafts", "1"),
                ("min-sales", "3"),
            ]),
            &[
                metric("grid:cost"),
                plain("cost-basis"),
                plain("subcrafts"),
                plain("min-sales"),
            ],
        );
        assert_eq!(q.get("gf"), None);
        assert_eq!(q.get("cost-basis"), None);
        assert_eq!(q.get("subcrafts"), None);
        assert_eq!(q.get("min-sales").as_deref(), Some(""));
    }

    #[test]
    fn a_column_with_no_live_filter_leaves_gf_untouched() {
        let gf = r#"{"grid:roi":{"op":"gte","value":"5"}}"#;
        let q = cleared_query(&params(&[("gf", gf)]), &[metric("grid:profit")]);
        assert_eq!(q.get("gf").as_deref(), Some(gf));
    }
}
