use std::collections::HashSet;

use icondata as i;
use leptos::{prelude::*, reactive::wrappers::write::SignalSetter, task::spawn_local};
use ultros_api_types::alert::{AlertTrigger, CreateAlertRequest};

use crate::api::{create_alert, list_endpoints};
use crate::components::{icon::Icon, modal::Modal};
use crate::global_state::toasts::use_toast;
use crate::i18n::*;

#[component]
pub fn UndercutAlertDrawer(set_visible: SignalSetter<bool>) -> impl IntoView {
    let i18n = use_i18n();
    let endpoints = Resource::new(|| (), |_| list_endpoints());
    let selected = RwSignal::new(HashSet::<i32>::new());
    let (margin, set_margin) = signal("0".to_string());
    let (error, set_error) = signal::<Option<String>>(None);
    let toasts = use_toast();

    let toggle = move |id: i32| {
        selected.update(|s| {
            if !s.insert(id) {
                s.remove(&id);
            }
        });
    };

    let submit = move |_| {
        set_error.set(None);
        let Ok(margin_percent) = margin.get().trim().parse::<i32>() else {
            set_error.set(Some(
                t_string!(i18n, undercut_alert_err_margin_number).to_string(),
            ));
            return;
        };
        if !(0..=200).contains(&margin_percent) {
            set_error.set(Some(
                t_string!(i18n, undercut_alert_err_margin_range).to_string(),
            ));
            return;
        }
        let endpoint_ids: Vec<i32> = selected.get().into_iter().collect();
        if endpoint_ids.is_empty() {
            set_error.set(Some(
                t_string!(i18n, alert_drawer_err_endpoint_required).to_string(),
            ));
            return;
        }
        let req = CreateAlertRequest {
            trigger: AlertTrigger::RetainerUndercut { margin_percent },
            delivery: None,
            endpoint_ids,
            cooldown_seconds: None,
        };
        spawn_local(async move {
            match create_alert(req).await {
                Ok(_) => {
                    if let Some(t) = toasts {
                        t.success(t_string!(i18n, undercut_alert_created_toast));
                    }
                    set_visible.set(false);
                }
                Err(e) => set_error.set(Some(format!("{e}"))),
            }
        });
    };

    view! {
        <Modal set_visible>
            <div class="p-4 space-y-4 w-[28rem] max-w-[calc(100vw-2rem)]">
                <div>
                    <h2 class="text-xl font-bold">{t!(i18n, undercut_alert_title)}</h2>
                    <p class="text-sm opacity-80">{t!(i18n, undercut_alert_description)}</p>
                </div>

                <div class="space-y-1">
                    <label class="text-sm font-semibold" for="undercut-alert-margin">
                        {t!(i18n, undercut_alert_margin_label)}
                    </label>
                    <input
                        id="undercut-alert-margin"
                        class="input w-full"
                        type="number"
                        min="0"
                        max="200"
                        prop:value=margin
                        on:input=move |e| set_margin.set(event_target_value(&e))
                    />
                </div>

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

                <Show when=move || error.get().is_some()>
                    <div class="text-sm text-red-500">{move || error.get().unwrap_or_default()}</div>
                </Show>

                <div class="flex justify-end gap-2 pt-2">
                    <button class="btn-ghost" on:click=move |_| set_visible.set(false)>
                        {t!(i18n, cancel)}
                    </button>
                    <button class="btn" on:click=submit>
                        <Icon icon=i::BsBell width="1em" height="1em" />
                        <span class="ml-1">{t!(i18n, undercut_alert_submit)}</span>
                    </button>
                </div>
            </div>
        </Modal>
    }
}
