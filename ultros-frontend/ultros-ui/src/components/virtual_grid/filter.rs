use super::layout::ColumnFilter;
use crate::{components::app_link::use_location_or_default, i18n::*};
use leptos::prelude::*;
use leptos_router::params::ParamsMap;

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
            super::registry::clear_key(&mut query, filter.key);
        }
    }
    if touched_metrics {
        super::registry::write_filters(&mut query, &metrics);
    }
    query
}

#[component]
pub fn ColumnFilterEditor(filter: ColumnFilter) -> impl IntoView {
    if let Some(kind) = filter.metric {
        let mut choices = filter.choices;
        choices.extend(
            filter
                .options
                .into_iter()
                .map(|(key, label)| (key.to_string(), label)),
        );
        return view! { <MetricFilterEditor column=filter.key label=filter.label kind choices/> }
            .into_any();
    }
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let location = use_location_or_default();
    let query = location.query;
    let key = filter.key;
    let registry = use_context::<super::registry::FilterRegistry>();
    let default_value = StoredValue::new(filter.default_value);
    let current = move |q: &ParamsMap| {
        q.get(key)
            .or_else(|| default_value.get_value())
            .unwrap_or_default()
    };
    let value = RwSignal::new(query.with_untracked(current));
    Effect::new(move |_| value.set(query.with(current)));
    #[cfg(feature = "hydrate")]
    let navigate = leptos_router::hooks::use_navigate();
    let commit = Callback::new(move |next: Option<String>| {
        let q = query.get_untracked();
        let mut q = registry.map(|r| r.canonical(&q)).unwrap_or(q);
        super::registry::clear_key(&mut q, key);
        if let Some(next) = next {
            q.replace(key, next);
        }
        if let Some(registry) = registry {
            registry.editing.set(None);
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
    let multiple = filter.multiple;
    let mut options = filter.choices;
    options.extend(
        filter
            .options
            .into_iter()
            .map(|(key, label)| (key.to_string(), label)),
    );
    let step = if matches!(key, "min-sales" | "vel") {
        "any"
    } else {
        "1"
    };
    let min = (key == "sales").then_some("0");
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
            <fieldset class="min-w-0">
                <legend>{filter.label.clone()}</legend>
                {if multiple {
                    view! { <div class="flex flex-col gap-2">
                        {options.into_iter().map(|(token, label)| {
                            let checked_token = token.clone();
                            view! { <label class="flex items-center gap-2"><input type="checkbox"
                                prop:checked=move || value.with(|raw| raw.split(',').any(|v| v == checked_token))
                                on:change=move |event| {
                                    value.update(|raw| {
                                        let mut selected = raw.split(',').filter(|v| !v.is_empty()).map(str::to_string).collect::<std::collections::BTreeSet<_>>();
                                        if event_target_checked(&event) { selected.insert(token.clone()); } else { selected.remove(&token); }
                                        *raw = selected.into_iter().collect::<Vec<_>>().join(",");
                                    });
                                }/>{label}</label> }
                        }).collect_view()}
                    </div> }.into_any()
                } else if options.is_empty() {
                    view! {<input aria-label=filter.label.clone() type=if filter.numeric {"number"} else {"text"} step=step min=min max=max
                        prop:value=move || value.get() on:input=move |e| value.set(event_target_value(&e))/>}.into_any()
                } else {
                    view! {<select aria-label=filter.label.clone() prop:value=move || value.get() on:change=move |e| value.set(event_target_value(&e))>
                        <option value="">{t!(i18n, grid_filter_any)}</option>
                        {options.into_iter().map(|(value,label)| view! {<option value=value>{label}</option>}).collect_view()}
                    </select>}.into_any()
                }}
            </fieldset>
            <div class="grid-menu-actions">
                <button type="submit">{t!(i18n, grid_filter_apply)}</button>
                <button type="button" on:click=move |_| commit.run(None)>{t!(i18n, grid_filter_clear)}</button>
            </div>
        </form>
    }.into_any()
}

use super::metrics::{FilterOp, MetricFilter, ValueKind, parse_filters};

#[component]
pub fn MetricSortControls(column: &'static str) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let location = use_location_or_default();
    let href = move |dir: &str| {
        metric_sort_href(&location.pathname.get(), location.query.get(), column, dir)
    };
    view! {
        <div class="grid-menu-actions">
            <leptos_router::components::A href=move || href("asc") scroll=false>{t!(i18n,grid_query_ascending)}</leptos_router::components::A>
            <leptos_router::components::A href=move || href("desc") scroll=false>{t!(i18n,grid_query_descending)}</leptos_router::components::A>
        </div>
    }
}

fn metric_sort_href(path: &str, mut query: ParamsMap, column: &str, dir: &str) -> String {
    query.replace("sort", format!("grid:{column}"));
    query.replace("dir", dir.to_string());
    format!("{path}{}", query.to_query_string())
}

/// Header action for a complete metric. The caller must exclude partial feeds;
/// QueryGrid owns the column's aria-sort and pending-result behavior.
#[component]
pub fn MetricSortHeader(
    column: &'static str,
    #[prop(into)] label: Signal<String>,
) -> impl IntoView {
    let location = use_location_or_default();
    let active = move || location.query.with(|q| q.get("sort")) == Some(format!("grid:{column}"));
    let ascending = move || location.query.with(|q| q.get("dir")).as_deref() == Some("asc");
    view! {
        <a
            href=move || metric_sort_href(
                &location.pathname.get(), location.query.get(), column,
                if active() && !ascending() { "asc" } else { "desc" },
            )
            data-noscroll=true
            data-metric-sort=column
            class="flex min-h-11 min-w-0 items-center gap-2 px-1 !text-brand-300 hover:text-brand-200 focus-visible:outline focus-visible:outline-2"
        >
            <span class="truncate min-w-0">{move || label.get()}</span>
            {move || active().then(|| view! {
                <span aria-hidden="true" class="shrink-0">{if ascending() { "↑" } else { "↓" }}</span>
            })}
        </a>
    }
}

#[component]
fn MetricFilterEditor(
    column: &'static str,
    label: String,
    kind: ValueKind,
    choices: Vec<(String, String)>,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let location = use_location_or_default();
    let query = location.query;
    let registry = use_context::<super::registry::FilterRegistry>();
    let read = move |q: &ParamsMap| {
        registry
            .map(|r| r.filters(q))
            .unwrap_or_else(|| parse_filters(q.get("gf").as_deref()))
    };
    let initial = query
        .with_untracked(read)
        .remove(column)
        .unwrap_or_else(|| MetricFilter {
            op: if kind == ValueKind::Number {
                FilterOp::Gte
            } else if choices.is_empty() {
                FilterOp::Contains
            } else {
                FilterOp::Eq
            },
            value: String::new(),
        });
    let (lower, upper) = if initial.op == FilterOp::Between {
        initial
            .value
            .split_once(',')
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .unwrap_or_default()
    } else {
        (initial.value, String::new())
    };
    let choices = StoredValue::new(choices);
    let value = RwSignal::new(lower);
    let upper = RwSignal::new(upper);
    let op = RwSignal::new(initial.op);
    let invalid = RwSignal::new(false);
    Effect::new(move |_| {
        let active = query.with(read);
        if let Some(filter) = active.get(column) {
            if filter.op == FilterOp::Between {
                let (a, b) = filter.value.split_once(',').unwrap_or_default();
                value.set(a.to_string());
                upper.set(b.to_string());
            } else {
                value.set(filter.value.clone());
                upper.set(String::new());
            }
            op.set(filter.op);
        } else {
            value.set(String::new());
        }
        invalid.set(false);
    });
    #[cfg(feature = "hydrate")]
    let navigate = leptos_router::hooks::use_navigate();
    let commit = Callback::new(move |clear: bool| {
        let q = query.get_untracked();
        let mut q = registry.map(|r| r.canonical(&q)).unwrap_or(q);
        let mut filters = parse_filters(q.get("gf").as_deref());
        if clear {
            filters.remove(column);
        } else {
            let filter = MetricFilter {
                op: op.get_untracked(),
                value: if op.get_untracked() == FilterOp::Between {
                    format!(
                        "{},{}",
                        value.get_untracked().trim(),
                        upper.get_untracked().trim()
                    )
                } else {
                    value.get_untracked().trim().to_string()
                },
            };
            if !filter.valid(kind)
                || (filter.op == FilterOp::Between
                    && filter.bounds().is_some_and(|(low, high)| low > high))
            {
                invalid.set(true);
                return;
            }
            filters.insert(column.to_string(), filter);
        }
        super::registry::write_filters(&mut q, &filters);
        if let Some(registry) = registry {
            registry.editing.set(None);
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
    let options = [
        (
            FilterOp::Between,
            t_string!(i18n, grid_query_between).to_string(),
        ),
        (FilterOp::Eq, t_string!(i18n, grid_query_eq).to_string()),
        (FilterOp::Ne, t_string!(i18n, grid_query_ne).to_string()),
        (
            FilterOp::Contains,
            t_string!(i18n, grid_query_contains).to_string(),
        ),
        (FilterOp::Gte, t_string!(i18n, grid_query_gte).to_string()),
        (FilterOp::Lte, t_string!(i18n, grid_query_lte).to_string()),
        (FilterOp::Lt, t_string!(i18n, grid_query_lt).to_string()),
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
                        ValueKind::Text => !matches!(op,FilterOp::Gte|FilterOp::Lte|FilterOp::Lt|FilterOp::Between),
                        ValueKind::Mixed => true,
                    }).map(|(op,label)|view! {<option value=token(op)>{label}</option>}).collect_view()}
                </select>
            </label>
            {move || if matches!(op.get(), FilterOp::Eq | FilterOp::Ne) && !choices.with_value(Vec::is_empty) {
                view! { <select aria-label=t_string!(i18n, grid_query_value).to_string() prop:value=move || value.get() on:change=move |e| value.set(event_target_value(&e))>
                    <option value="">{t!(i18n, grid_filter_any)}</option>
                    {choices.get_value().into_iter().map(|(key,label)| view! { <option value=key>{label}</option> }).collect_view()}
                </select> }.into_any()
            } else {
                view! { <input aria-label=t_string!(i18n,grid_query_value).to_string()
                    type=if kind==ValueKind::Number {"number"} else {"text"} step="any"
                    disabled=move || matches!(op.get(),FilterOp::Missing|FilterOp::Present)
                    prop:value=move || value.get() on:input=move |e|value.set(event_target_value(&e))/> }.into_any()
            }}
            {move || (op.get() == FilterOp::Between).then(|| view! {
                <input aria-label=t_string!(i18n, grid_query_upper_bound).to_string() type="number" step="any" prop:value=move || upper.get() on:input=move |e| upper.set(event_target_value(&e))/>
            })}
            {move || invalid.get().then(||view! {<span role="alert">{t!(i18n,grid_query_invalid)}</span>})}
            <div class="grid-menu-actions">
                <button type="submit">{t!(i18n,grid_filter_apply)}</button>
                <button type="button" on:click=move |_|commit.run(true)>{t!(i18n,grid_filter_clear)}</button>
            </div>
        </form>
    }
}

/// One operator label used by menu editors and toolbar chips.
pub fn operator_label(op: FilterOp) -> String {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    match op {
        FilterOp::Eq => t_string!(i18n, grid_query_eq).to_string(),
        FilterOp::Ne => t_string!(i18n, grid_query_ne).to_string(),
        FilterOp::Contains => t_string!(i18n, grid_query_contains).to_string(),
        FilterOp::Gte => t_string!(i18n, grid_query_gte).to_string(),
        FilterOp::Lte => t_string!(i18n, grid_query_lte).to_string(),
        FilterOp::Lt => t_string!(i18n, grid_query_lt).to_string(),
        FilterOp::Between => t_string!(i18n, grid_query_between).to_string(),
        FilterOp::Missing => t_string!(i18n, grid_query_missing).to_string(),
        FilterOp::Present => t_string!(i18n, grid_query_present).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::metrics::ValueKind;
    use super::*;

    #[test]
    fn metric_sort_replaces_native_sort_and_preserves_the_rest_of_the_url() {
        let query = params(&[
            ("sort", "profit"),
            ("dir", "asc"),
            ("window", "30"),
            ("cols", "market-sale-median-7,market-world"),
            ("gf", r#"{"market-quality":{"op":"eq","value":"HQ"}}"#),
            ("l", "saved-layout"),
            ("world", "Gilgamesh"),
        ]);
        for dir in ["desc", "asc"] {
            let href = metric_sort_href(
                "/venture-analyzer",
                query.clone(),
                "market-sale-median-7",
                dir,
            );
            let expected_sort = params(&[("sort", "grid:market-sale-median-7")]).to_query_string();
            assert!(href.contains(&expected_sort[1..]), "{href}");
            assert!(href.contains(&format!("dir={dir}")), "{href}");
            assert_eq!(href.matches("sort=").count(), 1);
            assert_eq!(href.matches("dir=").count(), 1);
            for key in ["window", "cols", "gf", "l", "world"] {
                let expected = params(&[(key, &query.get(key).unwrap())]).to_query_string();
                assert!(href.contains(&expected[1..]), "{href}");
            }
        }
    }

    #[test]
    fn metric_header_renders_a_sort_link_during_ssr() {
        Owner::new().with(|| {
            let html = view! {
                <MetricSortHeader column="market-sale-median-7" label=Signal::derive(|| "7d median".to_string()) />
            }.to_html();
            assert!(html.contains("href=\"?sort=grid"), "{html}");
            assert!(html.contains("market-sale-median-7"), "{html}");
            assert!(html.contains("dir=desc"), "{html}");
            assert!(html.contains("7d median"), "{html}");
        });
    }

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
