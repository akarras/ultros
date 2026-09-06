use super::layout::ColumnFilter;
use crate::{components::app_link::use_location_or_default, i18n::*};
use leptos::prelude::*;

#[component]
pub fn ColumnFilterEditor(filter: ColumnFilter) -> impl IntoView {
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
    }
}
