//! URL persistence shared by all analyzer tables. Existing `cols` and JSON
//! `layout` links remain readable; new layouts use a small `l` delta.
pub use super::filter::MetricSortHeader;
use super::metrics::{GridMetric, parse_filters, query_rows_with_tiebreak};
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
    /// Forwarded to [`VirtualGrid`]: data-row index to scroll into view.
    // `optional_no_strip`: forwarded as an `Option` from the layer above.
    #[prop(optional_no_strip)]
    reveal_index: Option<Signal<Option<usize>>>,
    #[prop(into)] id: String,
    #[prop(into)] label: String,
) -> impl IntoView
where
    T: Clone + PartialEq + Send + Sync + 'static,
    K: Clone + Ord + Hash + Send + Sync + 'static,
    KF: Fn(&T) -> K + Send + Sync + 'static,
    H: Fn(&'static str) -> AnyView + Send + Sync + 'static,
    F: Fn(T, &'static str) -> AnyView + Send + Sync + 'static,
    M: Fn(&T, &'static str) -> (String, f64) + Send + Sync + 'static,
{
    let location = use_location_or_default();
    let query = location.query;
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    let metrics = StoredValue::new(metrics);
    let key = StoredValue::new(key);
    let registry = use_context::<super::registry::FilterRegistry>();
    if let Some(registry) = registry {
        registry.register_sort_columns(
            columns,
            metrics.with_value(|metrics| {
                metrics
                    .iter()
                    .filter(|metric| !metric.partial)
                    .map(|metric| metric.id)
                    .collect()
            }),
        );
    }
    let sort_ascending = move |q: &leptos_router::params::ParamsMap| {
        registry
            .map(|registry| registry.sort_ascending(q))
            .unwrap_or_else(|| q.get("dir").as_deref() == Some("asc"))
    };
    let read_filters = move |q: &leptos_router::params::ParamsMap| {
        registry
            .map(|r| r.filters(q))
            .unwrap_or_else(|| parse_filters(q.get("gf").as_deref()))
    };
    // "Within the last 7 days" must pick the same rows on the server and in
    // the hydrating browser, so the render's clock travels with the page.
    // Only a URL with a relative bound pays for it; both sides read the same
    // URL, so both create (or skip) the shared value together.
    let initial = query.with_untracked(read_filters);
    let rendered_at = initial
        .values()
        .any(|f| f.has_relative())
        .then(|| SharedValue::new(now_unix).into_inner());
    let filters = Memo::new(move |_| {
        let raw = query.with(read_filters);
        let unchanged = raw == initial;
        let mut filters = raw;
        metrics.with_value(|metrics| {
            filters.retain(|id, f| metrics.iter().any(|m| m.id == id && f.valid(m.kind)))
        });
        if filters.values().any(|f| f.has_relative()) {
            // Keep the render's clock until the filters change, then use the
            // moment of the edit.
            let now = rendered_at.filter(|_| unchanged).unwrap_or_else(now_unix);
            for filter in filters.values_mut() {
                *filter = filter.resolved(now);
            }
        }
        filters
    });
    // Registered hosts read retired native tokens as metric sorts (issue
    // #1344); an unregistered grid only recognizes `grid:<id>`.
    let sort_column = move |q: &leptos_router::params::ParamsMap| {
        let sort = q.get("sort");
        match registry {
            Some(registry) => registry.sort_column(sort.as_deref()),
            None => super::registry::resolve_sort(sort.as_deref(), &[]),
        }
    };
    let result = Memo::new(move |_| {
        let sort = query.with(sort_column);
        let ascending = query.with(sort_ascending);
        each.with(|rows| {
            metrics.with_value(|metrics| {
                query_rows_with_tiebreak(
                    rows,
                    metrics,
                    &filters.get(),
                    sort.as_deref(),
                    ascending,
                    |a, b| key.with_value(|key| key(a).cmp(&key(b))),
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
        let sort = query.with(sort_column);
        let sort = sort.as_deref();
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
                        filter.unit = metric.unit;
                        filter.choices = alias_choices.clone();
                        col.filters.push(filter);
                    }
                    col.query_sort = !metric.partial;
                    if sort == Some(col.id) && !metric.partial {
                        col.aria_sort = if query.with(sort_ascending) {
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
    // Active filters are shown and edited by the `ControlBar` sharing this
    // grid's `FilterRegistry` (issue #1351). The grid itself renders none, so
    // a host has exactly one filter surface.
    view! {
        {show_saved_views.then(||view! {<div class="flex justify-end px-3 py-2"><super::saved_views::GridSavedViews id=saved_views_id/></div>})}
        {move || (result.with(|r|r.lacking_data)>0).then(||view! {
            <div class="px-3 py-2 text-xs text-[color:var(--color-text-muted)]" role="status" data-grid-query-coverage>
                <span>{t!(i18n, analyzer_rows_lacking_data, count = move || result.with(|r| r.lacking_data))}</span>
                " "{t!(i18n,grid_query_partial)}
            </div>
        })}
        {move || result.with(|r|r.sort_pending).then(||view! {<div class="px-3 py-2 text-xs" role="status">{t!(i18n,grid_query_pending)}</div>})}
        <VirtualGrid each=queried columns=resolved layout on_change reset_scroll=reset visible_range=range
            reveal_index key=move |row: &T| key.with_value(|key| key(row)) header view measure measure_version row_height id label/>
    }
}

/// Unix seconds now, from the browser clock when hydrated.
pub(crate) fn now_unix() -> f64 {
    #[cfg(feature = "hydrate")]
    {
        js_sys::Date::now() / 1000.0
    }
    #[cfg(not(feature = "hydrate"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or_default()
    }
}
