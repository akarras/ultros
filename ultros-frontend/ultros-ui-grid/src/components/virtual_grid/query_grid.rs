//! URL persistence shared by all analyzer tables. Existing `cols` and JSON
//! `layout` links remain readable; new layouts use a small `l` delta.
pub use super::filter::MetricSortHeader;
use super::metrics::{FilterOp, GridMetric, parse_filters, query_rows};
use super::row_source::RowSource;
use super::{GridChange, GridColumn, VirtualGrid};
use crate::components::app_link::use_location_or_default;
use crate::i18n::*;
use leptos::prelude::*;
use std::{collections::HashSet, hash::Hash};

#[component]
pub fn QueryGrid<T, K, KF, H, F, M>(
    #[prop(into)] each: Signal<Vec<T>>,
    #[prop(into)] columns: Signal<Vec<GridColumn>>,
    key: KF,
    header: H,
    view: F,
    measure: M,
    /// Invalidate sizing when a cell provider changes without replacing rows.
    #[prop(default = Signal::derive(|| 0), into)]
    measure_version: Signal<u64>,
    #[prop(optional)] metrics: Vec<GridMetric<T>>,
    #[prop(optional)] on_rows: Option<Callback<Vec<T>>>,
    #[prop(default = true)] show_saved_views: bool,
    #[prop(default = 40.0)] row_height: f64,
    #[prop(optional)] visible_range: Option<RwSignal<(usize, usize)>>,
    #[prop(into)] id: String,
    #[prop(into)] label: String,
) -> impl IntoView
where
    T: Clone + PartialEq + Send + Sync + 'static,
    K: Clone + Eq + Hash + Send + Sync + 'static,
    KF: Fn(&T) -> K + Send + Sync + 'static,
    H: Fn(&'static str) -> AnyView + Send + Sync + 'static,
    F: Fn(T, &'static str) -> AnyView + Send + Sync + 'static,
    M: Fn(&T, &'static str) -> (String, f64) + Send + Sync + 'static,
{
    let location = use_location_or_default();
    let query = location.query;
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let metrics = StoredValue::new(metrics);
    let registry = use_context::<super::registry::FilterRegistry>();
    let filters = Memo::new(move |_| {
        let mut filters = query.with(|q| {
            registry
                .map(|r| r.filters(q))
                .unwrap_or_else(|| parse_filters(q.get("gf").as_deref()))
        });
        metrics.with_value(|metrics| {
            filters.retain(|id, f| metrics.iter().any(|m| m.id == id && f.valid(m.kind)))
        });
        filters
    });
    let result = Memo::new(move |_| {
        let sort = query.with(|q| q.get("sort"));
        let ascending = query.with(|q| q.get("dir")).as_deref() == Some("asc");
        each.with(|rows| {
            metrics.with_value(|metrics| {
                query_rows(
                    rows,
                    metrics,
                    &filters.get(),
                    sort.as_deref().and_then(|s| s.strip_prefix("grid:")),
                    ascending,
                )
            })
        })
    });
    // Borrow the original rows when no query is active. Queried rows are cached
    // in `result`, so each visible-row read never clones the full collection.
    let queried = RowSource::new(each, result);
    if let Some(on_rows) = on_rows {
        Effect::new(move |_| on_rows.run(queried.with(Clone::clone)));
    }
    let layout = Signal::derive(move || query.with(|q| q.get("l").or_else(|| q.get("layout"))));
    let resolved = Memo::new(move |_| {
        let mut defs = columns.get();
        let sort = query.with(|q| q.get("sort"));
        let sort = sort.as_deref().and_then(|s| s.strip_prefix("grid:"));
        for col in &mut defs {
            let alias_choices: Vec<_> = col
                .filters
                .iter()
                .filter(|f| f.metric.is_none() && registry.is_some_and(|r| r.is_alias(f.key)))
                .flat_map(|f| {
                    f.choices.iter().cloned().chain(
                        f.options
                            .iter()
                            .map(|(key, label)| (key.to_string(), label.clone())),
                    )
                })
                .collect();
            if let Some(registry) = registry {
                col.filters
                    .retain(|f| f.metric.is_some() || !registry.is_alias(f.key));
            }
            if sort.is_some() {
                col.aria_sort = "none";
            }
            metrics.with_value(|metrics| {
                if let Some(metric) = metrics.iter().find(|m| m.id == col.id) {
                    if !col
                        .filters
                        .iter()
                        .any(|filter| filter.key == col.id && filter.metric.is_some())
                    {
                        let mut filter =
                            super::ColumnFilter::metric(col.id, col.label.clone(), metric.kind);
                        filter.choices = alias_choices.clone();
                        col.filters.push(filter);
                    }
                    col.query_sort = !metric.partial;
                    if sort == Some(col.id) && !metric.partial {
                        col.aria_sort = if query.with(|q| q.get("dir")).as_deref() == Some("asc") {
                            "ascending"
                        } else {
                            "descending"
                        };
                    }
                }
            });
        }
        if let Some(raw) = query.with(|q| q.get("cols")) {
            let visible: HashSet<_> = raw.split(',').collect();
            for col in &mut defs {
                if col.optional {
                    col.visible = visible.contains(col.id);
                }
            }
        }
        defs
    });
    let reset = Memo::new(move |_| {
        let mut q = query.get();
        for key in ["l", "layout", "cols"] {
            q.remove(key);
        }
        q.to_query_string()
    });
    #[cfg(feature = "hydrate")]
    let navigate = leptos_router::hooks::use_navigate();
    // Every column change is one URL replacement of the keys it owns; the
    // rest of the query (filters, sort, window, world) rides along untouched.
    // A `Callback` so the three writers below can each hold a copy: the
    // navigator it captures is not `Copy`.
    let commit_query = Callback::new(move |q: leptos_router::params::ParamsMap| {
        #[cfg(not(feature = "hydrate"))]
        let _ = q;
        #[cfg(feature = "hydrate")]
        navigate(
            &format!(
                "{}{}",
                location.pathname.get_untracked(),
                q.to_query_string()
            ),
            leptos_router::NavigateOptions {
                scroll: false,
                ..Default::default()
            },
        );
    });
    // `?cols=` with one optional column flipped, from the resolved defs so
    // the first write lists the page defaults too.
    let write_visibility = move |q: &mut leptos_router::params::ParamsMap, id, visible| {
        let mut defs = resolved.get_untracked();
        if let Some(col) = defs.iter_mut().find(|c| c.id == id) {
            col.visible = visible;
        }
        q.remove("cols");
        q.insert("cols", super::registry::cols_query(&defs));
    };
    let on_change = Callback::new(move |change: GridChange| {
        let mut q = query.get_untracked();
        q.remove("l");
        q.remove("layout");
        if let Some(layout) = change.layout {
            q.insert("l", layout);
        }
        if change.reset {
            q.remove("cols");
        }
        if let Some((id, visible)) = change.visibility {
            write_visibility(&mut q, id, visible);
        }
        commit_query.run(q);
    });
    if let Some(registry) = registry {
        registry.register(resolved.into());
        registry.register_count(Signal::derive(move || queried.with(Vec::len)));
        // The toolbar picker shares these with the header menu, so a tick
        // there and "Hide column" here write the same `?cols=`. Neither
        // touches the layout delta: a picker toggle must not undo a drag.
        registry.register_visibility(super::registry::ColumnVisibility {
            set_visible: Callback::new(move |(id, visible)| {
                let mut q = query.get_untracked();
                write_visibility(&mut q, id, visible);
                commit_query.run(q);
            }),
            reset: Callback::new(move |_| {
                let mut q = query.get_untracked();
                q.remove("cols");
                commit_query.run(q);
            }),
        });
    }
    let range = visible_range.unwrap_or_else(|| RwSignal::new((0, 0)));
    let saved_views_id = id.clone();
    let clear_href = move || {
        let mut q = query.get();
        q.remove("gf");
        format!("{}{}", location.pathname.get(), q.to_query_string())
    };
    view! {
        {show_saved_views.then(||view! {<div class="flex justify-end px-3 py-2"><super::saved_views::GridSavedViews id=saved_views_id/></div>})}
        {move || (registry.is_none() && !filters.with(|f|f.is_empty())).then(||view! {
            <div class="flex flex-wrap items-center gap-2 px-3 py-2 text-sm" data-grid-query-summary>
                <span>{t!(i18n, grid_query_count, count = move || queried.with(Vec::len))}</span>
                <span>{t!(i18n,grid_query_filters)}</span>
                {move ||filters.with(|f|f.iter().map(|(id,filter)| {
                    let label=columns.with(|defs|defs.iter().find(|c|c.id==id).map(|c|c.label.clone())).unwrap_or_else(||id.clone());
                    let mut q=query.get(); let mut next=filters.get();next.remove(id);q.remove("gf");
                    if !next.is_empty(){q.insert("gf",serde_json::to_string(&next).unwrap_or_default());}
                    let href=format!("{}{}",location.pathname.get(),q.to_query_string());
                    let operator = super::filter::operator_label(filter.op);
                    let value = if matches!(filter.op,FilterOp::Missing|FilterOp::Present) { "" } else { &filter.value };
                    view! {<leptos_router::components::A href scroll=false attr:class="rounded-full border px-2 py-1" attr:title=t_string!(i18n,grid_filter_clear).to_string()>{format!("{label}: {operator} {value} ×")}</leptos_router::components::A>}
                }).collect_view())}
                <leptos_router::components::A href=clear_href scroll=false>{t!(i18n,grid_query_clear)}</leptos_router::components::A>
            </div>
        })}
        {move || (result.with(|r|r.lacking_data)>0).then(||view! {
            <div class="px-3 py-2 text-xs text-[color:var(--color-text-muted)]" role="status" data-grid-query-coverage>
                <span>{t!(i18n, analyzer_rows_lacking_data, count = move || result.with(|r| r.lacking_data))}</span>
                " "{t!(i18n,grid_query_partial)}
            </div>
        })}
        {move || result.with(|r|r.sort_pending).then(||view! {<div class="px-3 py-2 text-xs" role="status">{t!(i18n,grid_query_pending)}</div>})}
        <VirtualGrid each=queried columns=resolved layout on_change reset_scroll=reset visible_range=range
            key header view measure measure_version row_height id label/>
    }
}
