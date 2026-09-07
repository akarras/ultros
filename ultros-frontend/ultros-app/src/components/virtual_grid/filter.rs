use super::layout::ColumnFilter;
use crate::{components::app_link::use_location_or_default, i18n::*};
use leptos::prelude::*;

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
        }
        // These landing defaults need an explicit empty value to mean unlimited.
        else if matches!(key, "next-sale" | "last-sold" | "min-sales") {
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

use super::metrics::{FilterOp, MetricFilter, ValueKind, parse_filters};

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
        q.remove("gf");
        if !filters.is_empty() {
            q.insert("gf", serde_json::to_string(&filters).unwrap_or_default());
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
