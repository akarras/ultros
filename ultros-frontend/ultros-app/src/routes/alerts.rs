use icondata as i;
use leptos::{prelude::*, task::spawn_local};
use ultros_api_types::alert::{Alert, AlertDelivery, AlertEvent, AlertTrigger, UpdateAlertRequest};

use crate::api::{delete_alert, get_alert_events, get_alerts, patch_alert};
use crate::components::icon::Icon;
use crate::global_state::toasts::use_toast;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string, use_i18n};
use xiv_gen::ItemId;

#[component]
pub fn Alerts() -> impl IntoView {
    let i18n = use_i18n();
    let action_version = RwSignal::new(0u64);
    let alerts = Resource::new(move || action_version.get(), move |_| get_alerts());
    let events = Resource::new(move || action_version.get(), move |_| get_alert_events());
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
                            t_string!(i18n, alerts_alert_enabled)
                        } else {
                            t_string!(i18n, alerts_alert_disabled)
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
                        t.success(t_string!(i18n, alerts_alert_deleted));
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
            <h1 class="text-2xl font-bold">{t!(i18n, alerts_page_heading)}</h1>

            <section>
                <h2 class="text-lg font-semibold mb-2">{t!(i18n, alerts_active_rules_heading)}</h2>
                <Suspense fallback=move || view! { <div>{t!(i18n, loading)}</div> }>
                    {move || {
                        alerts.get().map(|r| match r {
                            Ok(rows) if rows.is_empty() => view! {
                                <p class="opacity-70">
                                    {t!(i18n, alerts_empty_state)}
                                </p>
                            }.into_any(),
                            Ok(rows) => view! {
                                <div class="overflow-x-auto">
                                    <table class="w-full text-sm">
                                        <thead>
                                            <tr>
                                                <th class="text-left p-1">{t!(i18n, alerts_col_item)}</th>
                                                <th class="text-left p-1">{t!(i18n, alerts_col_threshold)}</th>
                                                <th class="text-left p-1">{t!(i18n, alerts_col_world)}</th>
                                                <th class="text-left p-1">{t!(i18n, alerts_col_hq)}</th>
                                                <th class="text-left p-1">{t!(i18n, alerts_col_delivery)}</th>
                                                <th class="text-left p-1">{t!(i18n, alerts_col_status)}</th>
                                                <th class="text-left p-1">{t!(i18n, alerts_col_actions)}</th>
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
                                                            let hq_str = if *hq_only {
                                                                t_string!(i18n, alerts_hq_any)
                                                            } else {
                                                                t_string!(i18n, alerts_any)
                                                            };
                                                            let delivery_label = match &a.delivery {
                                                                AlertDelivery::DiscordDm => {
                                                                    t_string!(i18n, alerts_delivery_discord_dm).to_string()
                                                                }
                                                                AlertDelivery::Webhook { .. } => {
                                                                    t_string!(i18n, alerts_delivery_webhook).to_string()
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
                                                                            t_string!(i18n, alerts_status_enabled)
                                                                        } else {
                                                                            t_string!(i18n, alerts_status_disabled)
                                                                        }}
                                                                    </td>
                                                                    <td class="p-1 flex gap-1">
                                                                        <button
                                                                            class="btn-ghost"
                                                                            aria-label=t_string!(i18n, alerts_toggle_aria)
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
                                                                            aria-label=t_string!(i18n, alerts_delete_aria)
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
                <h2 class="text-lg font-semibold mb-2">{t!(i18n, alerts_recent_fires_heading)}</h2>
                <Suspense fallback=move || view! { <div>{t!(i18n, loading)}</div> }>
                    {move || {
                        events.get().map(|r| match r {
                            Ok(rows) if rows.is_empty() => view! {
                                <p class="opacity-70">{t!(i18n, alerts_no_fires)}</p>
                            }.into_any(),
                            Ok(rows) => view! {
                                <div class="overflow-x-auto">
                                    <table class="w-full text-sm">
                                        <thead>
                                            <tr>
                                                <th class="text-left p-1">{t!(i18n, alerts_col_time)}</th>
                                                <th class="text-left p-1">{t!(i18n, alerts_col_item)}</th>
                                                <th class="text-left p-1">{t!(i18n, alerts_col_matched_price)}</th>
                                                <th class="text-left p-1">{t!(i18n, alerts_col_delivered)}</th>
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
