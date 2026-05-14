//! Modal that creates an `AlertTrigger::ListItemThreshold` for a given list.
//! Mirrors `AlertConfigDrawer` but scoped to a whole list — the only inputs
//! are the endpoints to fan out to. Per-item `target_price` is set on the
//! list page itself, so this drawer stays minimal.

use icondata as i;
use leptos::{prelude::*, reactive::wrappers::write::SignalSetter, task::spawn_local};
use std::collections::HashSet;
use ultros_api_types::alert::{Alert, AlertTrigger, CreateAlertRequest, UpdateAlertRequest};

use crate::api::{create_alert, get_alerts, list_endpoints, patch_alert};
use crate::components::{icon::Icon, modal::Modal};
use crate::global_state::toasts::use_toast;
use crate::i18n::*;

#[component]
pub fn ListSubscribeDrawer(
    list_id: i32,
    list_name: String,
    set_visible: SignalSetter<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let endpoints = Resource::new(|| (), |_| list_endpoints());
    let alerts = Resource::new(|| (), |_| get_alerts());
    let selected = RwSignal::new(HashSet::<i32>::new());
    let (mode, set_mode) = signal::<&'static str>("list_updates");
    let (error, set_error) = signal::<Option<String>>(None);
    let toasts = use_toast();

    let toggle = move |id: i32| {
        selected.update(|s| {
            if !s.insert(id) {
                s.remove(&id);
            }
        });
    };

    let existing_alert = Memo::new(move |_| {
        alerts.get().and_then(|result| {
            result.ok().and_then(|rows| {
                rows.into_iter().find(|alert| match &alert.trigger {
                    AlertTrigger::ListUpdate { list_id: id } => {
                        mode.get() == "list_updates" && *id == list_id
                    }
                    AlertTrigger::ListItemThreshold { list_id: id } => {
                        mode.get() == "price_targets" && *id == list_id
                    }
                    _ => false,
                })
            })
        })
    });

    Effect::new(move |_| {
        if let Some(alert) = existing_alert.get() {
            selected.set(alert.endpoint_ids.into_iter().collect());
        } else {
            selected.set(HashSet::new());
        }
    });

    let submit = move |_| {
        set_error.set(None);
        let endpoint_ids: Vec<i32> = selected.get().into_iter().collect();
        if endpoint_ids.is_empty() {
            set_error.set(Some(
                t_string!(i18n, alert_drawer_err_endpoint_required).to_string(),
            ));
            return;
        }
        let trigger = if mode.get() == "list_updates" {
            AlertTrigger::ListUpdate { list_id }
        } else {
            AlertTrigger::ListItemThreshold { list_id }
        };
        let req = CreateAlertRequest {
            trigger,
            delivery: None,
            endpoint_ids,
            cooldown_seconds: None,
        };
        let existing: Option<Alert> = existing_alert.get();
        spawn_local(async move {
            let result = if let Some(alert) = existing {
                patch_alert(
                    alert.id,
                    UpdateAlertRequest {
                        enabled: Some(true),
                        price_threshold: None,
                        endpoint_ids: Some(req.endpoint_ids.clone()),
                        cooldown_seconds: None,
                    },
                )
                .await
                .map(|_| alert)
            } else {
                create_alert(req).await
            };
            match result {
                Ok(_) => {
                    if let Some(t) = toasts {
                        t.success(if mode.get() == "list_updates" {
                            t_string!(i18n, list_update_subscribe_success_toast)
                        } else {
                            t_string!(i18n, list_subscribe_success_toast)
                        });
                    }
                    set_visible.set(false);
                }
                Err(e) => {
                    set_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    view! {
        <Modal set_visible>
            <div class="p-4 space-y-4 w-[28rem]">
                <h2 class="text-xl font-bold">{t!(i18n, list_subscribe_title, name = list_name.clone())}</h2>
                <Show when=move || existing_alert.get().is_some()>
                    <div class="rounded-lg border border-[color:var(--color-outline)] bg-[color:var(--color-background-panel)] px-3 py-2 text-sm text-[color:var(--color-text-muted)]">
                        "Editing existing settings for this list."
                    </div>
                </Show>
                <p class="text-sm opacity-80">
                    {move || if mode.get() == "list_updates" {
                        t_string!(i18n, list_update_subscribe_description).to_string()
                    } else {
                        t_string!(i18n, list_subscribe_description).to_string()
                    }}
                </p>

                <div class="grid grid-cols-2 gap-2">
                    <button
                        type="button"
                        class="btn-ghost"
                        class:bg-brand-500=move || mode.get() == "price_targets"
                        on:click=move |_| set_mode.set("price_targets")
                    >
                        {t!(i18n, list_subscribe_price_targets_mode)}
                    </button>
                    <button
                        type="button"
                        class="btn-ghost"
                        class:bg-brand-500=move || mode.get() == "list_updates"
                        on:click=move |_| set_mode.set("list_updates")
                    >
                        {t!(i18n, list_subscribe_updates_mode)}
                    </button>
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
                        <span class="ml-1">
                            {move || if mode.get() == "list_updates" {
                                t_string!(i18n, list_update_subscribe_submit).to_string()
                            } else {
                                t_string!(i18n, list_subscribe_submit).to_string()
                            }}
                        </span>
                    </button>
                </div>
            </div>
        </Modal>
    }
}
