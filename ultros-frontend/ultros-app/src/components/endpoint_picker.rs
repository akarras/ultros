//! Shared "Deliver to" endpoint checkbox list used by the alert drawers.
//! The caller owns the endpoints resource (some drawers also need it for
//! name lookups) and the selection set; this renders the label, the
//! loading/empty/error states, and the checkboxes.

use leptos::prelude::*;
use std::collections::HashSet;
use ultros_api_types::alert::Endpoint;

use crate::error::AppResult;
use crate::i18n::{t, use_i18n};

#[component]
pub(crate) fn EndpointPicker(
    endpoints: Resource<AppResult<Vec<Endpoint>>>,
    selected: RwSignal<HashSet<i32>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let toggle = move |id: i32| {
        selected.update(|s| {
            if !s.insert(id) {
                s.remove(&id);
            }
        });
    };
    view! {
        <div class="space-y-1">
            <label class="text-sm font-semibold">{t!(i18n, alert_drawer_deliver_to)}</label>
            <Suspense fallback=move || {
                view! { <div class="text-sm opacity-70">{t!(i18n, alert_drawer_loading_endpoints)}</div> }
            }>
                {move || endpoints.get().map(|r| match r {
                    Ok(list) if list.is_empty() => view! {
                        <p class="text-sm opacity-70">
                            {t!(i18n, alert_drawer_no_endpoints_prefix)}
                            <a href="/alerts" class="underline">{t!(i18n, alert_drawer_no_endpoints_link)}</a>
                            {t!(i18n, alert_drawer_no_endpoints_suffix)}
                        </p>
                    }.into_any(),
                    Ok(list) => view! {
                        <ul class="space-y-1">
                            {list.into_iter().map(|e| {
                                let id = e.id;
                                let is_sel = move || selected.get().contains(&id);
                                view! {
                                    <li>
                                        <label class="flex items-center gap-2">
                                            <input
                                                type="checkbox"
                                                prop:checked=is_sel
                                                on:change=move |_| toggle(id)
                                            />
                                            <span>{e.name}</span>
                                        </label>
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    }.into_any(),
                    Err(e) => view! {
                        <div class="text-red-500">{format!("{e}")}</div>
                    }.into_any(),
                })}
            </Suspense>
        </div>
    }
}
