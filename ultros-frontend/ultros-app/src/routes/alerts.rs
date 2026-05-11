use icondata as i;
use leptos::{prelude::*, task::spawn_local};
use ultros_api_types::alert::{Alert, AlertDelivery, AlertEvent, AlertTrigger, UpdateAlertRequest};

use crate::api::{delete_alert, get_alert_events, get_alerts, patch_alert};
use crate::components::icon::Icon;
use crate::global_state::toasts::use_toast;
use crate::global_state::xiv_data::tracked_data;
use xiv_gen::ItemId;

#[component]
pub fn Alerts() -> impl IntoView {
    let action_version = RwSignal::new(0u64);
    let alerts = Resource::new(
        move || action_version.get(),
        move |_| get_alerts(),
    );
    let events = Resource::new(
        move || action_version.get(),
        move |_| get_alert_events(),
    );
    let toasts = use_toast();

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
                    action_version.update(|v| *v += 1);
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
                    action_version.update(|v| *v += 1);
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
        <div class="p-4 space-y-6">
            <h1 class="text-2xl font-bold">"Price alerts"</h1>

            <section>
                <h2 class="text-lg font-semibold mb-2">"Active rules"</h2>
                <Suspense fallback=move || view! { <div>"Loading..."</div> }>
                    {move || {
                        alerts.get().map(|r| match r {
                            Ok(rows) if rows.is_empty() => view! {
                                <p class="opacity-70">
                                    "No alerts yet. Add one from any item on a list."
                                </p>
                            }.into_any(),
                            Ok(rows) => view! {
                                <div class="overflow-x-auto">
                                    <table class="w-full text-sm">
                                        <thead>
                                            <tr>
                                                <th class="text-left p-1">"Item"</th>
                                                <th class="text-left p-1">"Threshold"</th>
                                                <th class="text-left p-1">"World"</th>
                                                <th class="text-left p-1">"HQ"</th>
                                                <th class="text-left p-1">"Delivery"</th>
                                                <th class="text-left p-1">"Status"</th>
                                                <th class="text-left p-1">"Actions"</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            <For
                                                each=move || rows.clone()
                                                key=|a| a.id
                                                children=move |a: Alert| {
                                                    match &a.trigger {
                                                        AlertTrigger::BelowThreshold {
                                                            item_id,
                                                            price_threshold,
                                                            hq_only,
                                                            world_selector,
                                                        } => {
                                                            let item_name = tracked_data()
                                                                .items
                                                                .get(&ItemId(*item_id))
                                                                .map(|it| it.name.as_str().to_string())
                                                                .unwrap_or_else(|| format!("Item {item_id}"));
                                                            let threshold_str =
                                                                format!("≤ {price_threshold} gil");
                                                            let world_str = match world_selector {
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
                                                            let hq_str =
                                                                if *hq_only { "HQ" } else { "any" };
                                                            let delivery_label = match &a.delivery {
                                                                AlertDelivery::DiscordDm => {
                                                                    "Discord DM".to_string()
                                                                }
                                                                AlertDelivery::Webhook { .. } => {
                                                                    "Webhook".to_string()
                                                                }
                                                            };
                                                            let enabled = a.enabled;
                                                            let a_clone = a.clone();
                                                            let id = a.id;
                                                            view! {
                                                                <tr class="border-t">
                                                                    <td class="p-1">{item_name}</td>
                                                                    <td class="p-1">{threshold_str}</td>
                                                                    <td class="p-1">{world_str}</td>
                                                                    <td class="p-1">{hq_str}</td>
                                                                    <td class="p-1">{delivery_label}</td>
                                                                    <td class="p-1">
                                                                        {if enabled {
                                                                            "enabled"
                                                                        } else {
                                                                            "disabled"
                                                                        }}
                                                                    </td>
                                                                    <td class="p-1 flex gap-1">
                                                                        <button
                                                                            class="btn-ghost"
                                                                            aria-label="Toggle enabled"
                                                                            on:click=move |_| toggle(a_clone.clone())
                                                                        >
                                                                            <Icon
                                                                                icon=if enabled {
                                                                                    i::BsPauseFill
                                                                                } else {
                                                                                    i::BsPlayFill
                                                                                }
                                                                            />
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
                                                            .into_any()
                                                        }
                                                    }
                                                }
                                            />
                                        </tbody>
                                    </table>
                                </div>
                            }.into_any(),
                            Err(e) => view! {
                                <div class="text-red-500">{format!("{e}")}</div>
                            }.into_any(),
                        })
                    }}
                </Suspense>
            </section>

            <section>
                <h2 class="text-lg font-semibold mb-2">"Recent fires"</h2>
                <Suspense fallback=move || view! { <div>"Loading..."</div> }>
                    {move || {
                        events.get().map(|r| match r {
                            Ok(rows) if rows.is_empty() => view! {
                                <p class="opacity-70">"No fires yet."</p>
                            }.into_any(),
                            Ok(rows) => view! {
                                <div class="overflow-x-auto">
                                    <table class="w-full text-sm">
                                        <thead>
                                            <tr>
                                                <th class="text-left p-1">"Time"</th>
                                                <th class="text-left p-1">"Item"</th>
                                                <th class="text-left p-1">"Matched price"</th>
                                                <th class="text-left p-1">"Delivered"</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            <For
                                                each=move || rows.clone()
                                                key=|e| e.id
                                                children=move |e: AlertEvent| {
                                                    let item_name = tracked_data()
                                                        .items
                                                        .get(&ItemId(e.item_id))
                                                        .map(|it| it.name.as_str().to_string())
                                                        .unwrap_or_else(|| {
                                                            format!("Item {}", e.item_id)
                                                        });
                                                    let fired_str = e.fired_at.to_rfc3339();
                                                    let price_str = e
                                                        .matched_price
                                                        .map(|p| p.to_string())
                                                        .unwrap_or_else(|| "\u{2014}".into());
                                                    let delivered_str = if e.delivered {
                                                        "\u{2713}".to_string()
                                                    } else {
                                                        e.delivery_error
                                                            .as_deref()
                                                            .unwrap_or("\u{2717}")
                                                            .to_string()
                                                    };
                                                    view! {
                                                        <tr class="border-t">
                                                            <td class="p-1">{fired_str}</td>
                                                            <td class="p-1">{item_name}</td>
                                                            <td class="p-1">{price_str}</td>
                                                            <td class="p-1">{delivered_str}</td>
                                                        </tr>
                                                    }
                                                }
                                            />
                                        </tbody>
                                    </table>
                                </div>
                            }.into_any(),
                            Err(e) => view! {
                                <div class="text-red-500">{format!("{e}")}</div>
                            }.into_any(),
                        })
                    }}
                </Suspense>
            </section>
        </div>
    }
}
