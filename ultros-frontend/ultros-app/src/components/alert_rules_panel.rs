use icondata as i;
use leptos::{prelude::*, task::spawn_local};
use ultros_api_types::alert::{Alert, AlertTrigger, Endpoint, UpdateAlertRequest};
use xiv_gen::ItemId;

use crate::api::{delete_alert, get_alerts, list_endpoints, patch_alert};
use crate::components::create_alert_drawer::CreateAlertDrawer;
use crate::components::icon::Icon;
use crate::global_state::home_world::use_home_world;
use crate::global_state::toasts::use_toast;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, use_i18n};

#[component]
pub fn AlertRulesPanel() -> impl IntoView {
    let i18n = use_i18n();
    let version = RwSignal::new(0u64);
    let alerts = Resource::new(move || version.get(), move |_| get_alerts());
    let endpoints = Resource::new(move || version.get(), move |_| list_endpoints());
    let toasts = use_toast();
    let (drawer_visible, set_drawer_visible) = signal(false);
    // Default the world picker in the create drawer to the user's home world
    // (when set), mirroring how AlertConfigDrawer is opened from item pages.
    let (home_world, _) = use_home_world();
    let default_world = Signal::derive(move || {
        home_world
            .get()
            .map(|w| ultros_api_types::world_helper::AnySelector::World(w.id))
    });

    // Refresh the alerts list when the drawer closes (best-effort: we can't tell if
    // the user actually saved without threading a callback, so we just bump the
    // version on every close — cheap enough).
    Effect::new(move |_| {
        let visible = drawer_visible.get();
        if !visible {
            version.update(|v| *v += 1);
        }
    });

    let toggle = move |alert: Alert| {
        let new_enabled = !alert.enabled;
        spawn_local(async move {
            match patch_alert(
                alert.id,
                UpdateAlertRequest {
                    enabled: Some(new_enabled),
                    price_threshold: None,
                },
            )
            .await
            {
                Ok(()) => {
                    if let Some(t) = toasts {
                        t.success(if new_enabled {
                            "Alert enabled"
                        } else {
                            "Alert disabled"
                        });
                    }
                    version.update(|v| *v += 1);
                }
                Err(e) => {
                    if let Some(t) = toasts {
                        t.error(format!("{e}"));
                    }
                }
            }
        });
    };

    let remove = move |id: i32| {
        spawn_local(async move {
            match delete_alert(id).await {
                Ok(()) => {
                    if let Some(t) = toasts {
                        t.success("Alert deleted");
                    }
                    version.update(|v| *v += 1);
                }
                Err(e) => {
                    if let Some(t) = toasts {
                        t.error(format!("{e}"));
                    }
                }
            }
        });
    };

    view! {
        <div class="space-y-3">
            <div class="flex justify-end">
                <button class="btn" on:click=move |_| set_drawer_visible.set(true)>
                    <Icon icon=i::BsBell />
                    <span class="ml-1">{t!(i18n, create_alert_button)}</span>
                </button>
            </div>
            <Show when=move || drawer_visible.get()>
                <CreateAlertDrawer
                    default_world=default_world
                    set_visible=set_drawer_visible.into()
                />
            </Show>
            <Suspense fallback=move || view! { <div>"Loading..."</div> }>
            {move || {
                let endpoint_list: Vec<Endpoint> = endpoints
                    .get()
                    .and_then(|r| r.ok())
                    .unwrap_or_default();
                let ep_name = move |id: i32| {
                    endpoint_list
                        .iter()
                        .find(|e| e.id == id)
                        .map(|e| e.name.clone())
                        .unwrap_or_else(|| format!("#{id}"))
                };
                alerts
                    .get()
                    .map(|r| match r {
                        Ok(rows) if rows.is_empty() => {
                            view! {
                                <p class="opacity-70">
                                    "No alerts yet. Add one from any item on a list."
                                </p>
                            }
                                .into_any()
                        }
                        Ok(rows) => {
                            view! {
                                <div class="overflow-x-auto">
                                    <table class="w-full text-sm">
                                        <thead>
                                            <tr>
                                                <th class="text-left p-1">"Item"</th>
                                                <th class="text-left p-1">"Threshold"</th>
                                                <th class="text-left p-1">"World"</th>
                                                <th class="text-left p-1">"HQ"</th>
                                                <th class="text-left p-1">"Endpoints"</th>
                                                <th class="text-left p-1">"Status"</th>
                                                <th class="text-left p-1">"Actions"</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            <For
                                                each=move || rows.clone()
                                                key=|a| a.id
                                                children=move |a: Alert| {
                                                    // Display strings differ per trigger variant. List-scoped alerts
                                                    // don't carry a single item/world/hq — render those columns with
                                                    // the list id and "—" placeholders so the table stays uniform.
                                                    let (item_name, threshold_str, world_str, hq_str): (
                                                        String,
                                                        String,
                                                        String,
                                                        &'static str,
                                                    ) = match a.trigger.clone() {
                                                        AlertTrigger::BelowThreshold {
                                                            item_id,
                                                            price_threshold,
                                                            hq_only,
                                                            world_selector,
                                                        } => {
                                                            let name = tracked_data()
                                                                .items
                                                                .get(&ItemId(item_id))
                                                                .map(|it| it.name.as_str().to_string())
                                                                .unwrap_or_else(|| format!("Item {item_id}"));
                                                            let threshold = format!("≤ {price_threshold} gil");
                                                            let world = match world_selector {
                                                                ultros_api_types::world_helper::AnySelector::World(id) => {
                                                                    format!("World({id})")
                                                                }
                                                                ultros_api_types::world_helper::AnySelector::Datacenter(id) => {
                                                                    format!("DC({id})")
                                                                }
                                                                ultros_api_types::world_helper::AnySelector::Region(id) => {
                                                                    format!("Region({id})")
                                                                }
                                                            };
                                                            let hq = if hq_only { "HQ" } else { "any" };
                                                            (name, threshold, world, hq)
                                                        }
                                                        AlertTrigger::ListItemThreshold { list_id } => (
                                                            format!("List #{list_id}"),
                                                            "per-item target".to_string(),
                                                            "list-defined".to_string(),
                                                            "—",
                                                        ),
                                                    };
                                                    let endpoints_str = a
                                                        .endpoint_ids
                                                        .iter()
                                                        .map(|id| ep_name(*id))
                                                        .collect::<Vec<_>>()
                                                        .join(", ");
                                                    let enabled = a.enabled;
                                                    let a_clone = a.clone();
                                                    let id = a.id;
                                                    view! {
                                                        <tr class="border-t">
                                                            <td class="p-1">{item_name}</td>
                                                            <td class="p-1">{threshold_str}</td>
                                                            <td class="p-1">{world_str}</td>
                                                            <td class="p-1">{hq_str}</td>
                                                            <td class="p-1">{endpoints_str}</td>
                                                            <td class="p-1">
                                                                {if enabled { "enabled" } else { "disabled" }}
                                                            </td>
                                                            <td class="p-1 flex gap-1">
                                                                <button
                                                                    class="btn-ghost"
                                                                    aria-label="Toggle enabled"
                                                                    on:click=move |_| toggle(a_clone.clone())
                                                                >
                                                                    <Icon icon=if enabled {
                                                                        i::BsPauseFill
                                                                    } else {
                                                                        i::BsPlayFill
                                                                    } />
                                                                </button>
                                                                <button
                                                                    class="btn-ghost text-red-400"
                                                                    aria-label="Delete alert"
                                                                    on:click=move |_| remove(id)
                                                                >
                                                                    <Icon icon=i::BiTrashSolid />
                                                                </button>
                                                            </td>
                                                        </tr>
                                                    }
                                                }
                                            />
                                        </tbody>
                                    </table>
                                </div>
                            }
                                .into_any()
                        }
                        Err(e) => {
                            view! { <div class="text-red-500">{format!("{e}")}</div> }.into_any()
                        }
                    })
            }}
            </Suspense>
        </div>
    }
}
