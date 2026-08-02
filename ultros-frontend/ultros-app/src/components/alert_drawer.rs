//! Unified "Add alert" modal shared by the /alerts rules panel and the
//! /retainers/undercuts page. A type toggle switches between the two alert
//! shapes (item price threshold vs retainer undercut), and the same surface
//! lists the user's existing alerts of the selected type with a remove
//! button, so pages embedding this drawer don't need their own manager UI.

use icondata as i;
use leptos::{prelude::*, reactive::wrappers::write::SignalSetter, task::spawn_local};
use std::cmp::Reverse;
use std::collections::HashSet;
use ultros_api_types::{
    alert::{Alert, AlertTrigger, CreateAlertRequest, Endpoint},
    icon_size::IconSize,
    world_helper::AnySelector,
};
use xiv_gen::{Item, ItemId};

use crate::api::{create_alert, delete_alert, get_alerts, list_endpoints};
use crate::components::{
    endpoint_picker::EndpointPicker, icon::Icon, item_icon::ItemIcon, modal::Modal,
    world_picker::WorldPicker,
};
use crate::global_state::home_world::use_home_world;
use crate::global_state::toasts::use_toast;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string, use_i18n};

/// Which alert shape the drawer is currently configuring.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AlertKind {
    #[default]
    ItemPrice,
    Undercut,
}

/// Whether an existing alert belongs under the given drawer tab. List-scoped
/// alerts are managed from their list pages and never show here.
fn trigger_matches_kind(trigger: &AlertTrigger, kind: AlertKind) -> bool {
    matches!(
        (trigger, kind),
        (AlertTrigger::BelowThreshold { .. }, AlertKind::ItemPrice)
            | (AlertTrigger::RetainerUndercut { .. }, AlertKind::Undercut)
    )
}

#[component]
pub fn AlertDrawer(
    #[prop(optional)] initial_kind: AlertKind,
    set_visible: SignalSetter<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let kind = RwSignal::new(initial_kind);
    // Cache-buster bumped after a delete so the active list refreshes without
    // closing the drawer.
    let version = RwSignal::new(0u64);
    let alerts = Resource::new(move || version.get(), move |_| get_alerts());
    let endpoints = Resource::new(|| (), |_| list_endpoints());
    let selected_endpoints = RwSignal::new(HashSet::<i32>::new());
    let (error, set_error) = signal::<Option<String>>(None);
    let toasts = use_toast();

    // Item-price form state. Default the world picker to the user's home world
    // when set, mirroring how AlertConfigDrawer is opened from item pages.
    let (home_world, _) = use_home_world();
    let (search, set_search) = signal::<String>("".into());
    let selected_item = RwSignal::<Option<(i32, String)>>::new(None);
    let (world, set_world) =
        signal::<Option<AnySelector>>(home_world.get_untracked().map(|w| AnySelector::World(w.id)));
    let (price_threshold, set_price_threshold) = signal::<String>("".into());
    let (hq_only, set_hq_only) = signal(false);

    // Undercut form state.
    let (margin, set_margin) = signal("0".to_string());

    // Same scoring shape as routes/list_view.rs — substring match, descending by
    // item level so common high-IL items rank ahead of vendor trash that shares
    // a prefix. We cap at 50 matches so the dropdown stays usable.
    let item_results = move || -> Vec<(ItemId, &'static Item)> {
        let s = search.get();
        if s.trim().is_empty() {
            return Vec::new();
        }
        let s_lower = s.to_lowercase();
        let items = &tracked_data().items;
        let mut matches: Vec<(&ItemId, &'static Item)> = items
            .iter()
            .filter(|(_, i)| i.item_search_category > 0)
            .filter(|(_, i)| i.name.to_lowercase().contains(&s_lower))
            .collect();
        matches.sort_by_key(|(_, i)| Reverse(i.level_item));
        matches
            .into_iter()
            .take(50)
            .map(|(id, item)| (*id, item))
            .collect()
    };

    let submit = move |_| {
        set_error.set(None);
        let trigger = match kind.get() {
            AlertKind::ItemPrice => {
                let Some((item_id, _)) = selected_item.get() else {
                    set_error.set(Some(
                        t_string!(i18n, create_alert_err_pick_item).to_string(),
                    ));
                    return;
                };
                let Some(world_selector) = world.get() else {
                    set_error.set(Some(
                        t_string!(i18n, alert_drawer_err_pick_world).to_string(),
                    ));
                    return;
                };
                let Ok(threshold) = price_threshold.get().parse::<i32>() else {
                    set_error.set(Some(
                        t_string!(i18n, alert_drawer_err_threshold_int).to_string(),
                    ));
                    return;
                };
                if threshold <= 0 {
                    set_error.set(Some(
                        t_string!(i18n, alert_drawer_err_threshold_positive).to_string(),
                    ));
                    return;
                }
                AlertTrigger::BelowThreshold {
                    item_id,
                    world_selector,
                    price_threshold: threshold,
                    hq_only: hq_only.get(),
                }
            }
            AlertKind::Undercut => {
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
                AlertTrigger::RetainerUndercut { margin_percent }
            }
        };
        let endpoint_ids: Vec<i32> = selected_endpoints.get().into_iter().collect();
        if endpoint_ids.is_empty() {
            set_error.set(Some(
                t_string!(i18n, alert_drawer_err_endpoint_required).to_string(),
            ));
            return;
        }
        let req = CreateAlertRequest {
            trigger,
            delivery: None,
            endpoint_ids,
            cooldown_seconds: None,
        };
        let created_kind = kind.get();
        spawn_local(async move {
            match create_alert(req).await {
                Ok(_) => {
                    if let Some(t) = toasts {
                        t.success(match created_kind {
                            AlertKind::ItemPrice => {
                                t_string!(i18n, alert_drawer_created_toast).to_string()
                            }
                            AlertKind::Undercut => {
                                t_string!(i18n, undercut_alert_created_toast).to_string()
                            }
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

    let remove = move |id: i32| {
        spawn_local(async move {
            match delete_alert(id).await {
                Ok(()) => {
                    if let Some(t) = toasts {
                        t.success(t_string!(i18n, alerts_alert_deleted).to_string());
                    }
                    version.update(|v| *v += 1);
                }
                Err(e) => {
                    set_error.set(Some(format!("{e}")));
                }
            }
        });
    };

    let kind_btn = move |target: AlertKind, label: String| {
        view! {
            <button
                type="button"
                class="btn-ghost"
                class:bg-brand-500=move || kind.get() == target
                on:click=move |_| {
                    kind.set(target);
                    set_error.set(None);
                }
            >
                {label}
            </button>
        }
    };

    view! {
        <Modal set_visible>
            <div class="p-4 space-y-4 w-[28rem] max-h-[80vh] overflow-y-auto">
                <h2 class="text-xl font-bold">{t!(i18n, add_alert_button)}</h2>

                <div class="space-y-1">
                    <label class="text-sm font-semibold">{t!(i18n, alert_kind_label)}</label>
                    <div class="grid grid-cols-2 gap-2">
                        {kind_btn(AlertKind::ItemPrice, t_string!(i18n, alert_kind_item_price).to_string())}
                        {kind_btn(AlertKind::Undercut, t_string!(i18n, alert_kind_undercut).to_string())}
                    </div>
                </div>

                <Show when=move || kind.get() == AlertKind::Undercut>
                    <p class="text-sm opacity-80">{t!(i18n, undercut_alert_description)}</p>
                </Show>

                <Show when=move || kind.get() == AlertKind::ItemPrice>
                    <div class="space-y-4">
                        <div class="space-y-1">
                            <label class="text-sm font-semibold" for="create-alert-search">{t!(i18n, create_alert_item_label)}</label>
                            {move || match selected_item.get() {
                                Some((id, name)) => view! {
                                    <div class="flex items-center justify-between gap-2 rounded border border-[color:var(--color-outline)] p-2">
                                        <div class="flex items-center gap-2 min-w-0">
                                            <ItemIcon item_id=id icon_size=IconSize::Small />
                                            <span class="truncate font-medium">{name}</span>
                                        </div>
                                        <button class="btn-ghost text-xs"
                                            on:click=move |_| {
                                                selected_item.set(None);
                                                set_search.set("".into());
                                            }>
                                            {t!(i18n, create_alert_change_item)}
                                        </button>
                                    </div>
                                }.into_any(),
                                None => view! {
                                    <div class="space-y-1">
                                        <input
                                            id="create-alert-search"
                                            class="input w-full"
                                            placeholder=t_string!(i18n, create_alert_search_placeholder)
                                            prop:value=search
                                            on:input=move |e| set_search.set(event_target_value(&e))
                                        />
                                        {move || {
                                            let results = item_results();
                                            if results.is_empty() {
                                                view! { <div class="text-xs opacity-60 px-1">""</div> }.into_any()
                                            } else {
                                                view! {
                                                    <ul class="max-h-48 overflow-y-auto rounded border border-[color:var(--color-outline)] divide-y divide-[color:var(--color-outline)]">
                                                        {results.into_iter().map(|(id, item)| {
                                                            let item_id = id.0;
                                                            let item_name = item.name.as_str().to_string();
                                                            let item_name_for_button = item_name.clone();
                                                            view! {
                                                                <li>
                                                                    <button
                                                                        type="button"
                                                                        class="flex items-center gap-2 w-full text-left p-2 hover:bg-[color:var(--color-background-panel)]"
                                                                        on:click=move |_| {
                                                                            selected_item.set(Some((item_id, item_name_for_button.clone())));
                                                                        }
                                                                    >
                                                                        <ItemIcon item_id=item_id icon_size=IconSize::Small />
                                                                        <span class="truncate">{item_name}</span>
                                                                    </button>
                                                                </li>
                                                            }
                                                        }).collect_view()}
                                                    </ul>
                                                }.into_any()
                                            }
                                        }}
                                    </div>
                                }.into_any(),
                            }}
                        </div>

                        <div class="space-y-1">
                            <label class="text-sm font-semibold">{t!(i18n, alert_drawer_world_label)}</label>
                            <WorldPicker
                                current_world=world.into()
                                set_current_world=set_world.into()
                            />
                        </div>

                        <div class="space-y-1">
                            <label class="text-sm font-semibold" for="create-alert-threshold">{t!(i18n, alert_drawer_threshold_label)}</label>
                            <input
                                id="create-alert-threshold"
                                class="input w-full"
                                type="number"
                                min="1"
                                placeholder=t_string!(i18n, alert_drawer_threshold_placeholder)
                                prop:value=price_threshold
                                on:input=move |e| set_price_threshold.set(event_target_value(&e))
                            />
                        </div>

                        <label class="flex items-center gap-2">
                            <input
                                type="checkbox"
                                prop:checked=hq_only
                                on:change=move |e| set_hq_only.set(event_target_checked(&e))
                            />
                            <span class="text-sm">{t!(i18n, hq_only)}</span>
                        </label>
                    </div>
                </Show>

                <Show when=move || kind.get() == AlertKind::Undercut>
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
                </Show>

                <EndpointPicker endpoints selected=selected_endpoints />

                <div class="space-y-1">
                    <label class="text-sm font-semibold">{t!(i18n, alert_drawer_active_heading)}</label>
                    <Suspense fallback=move || {
                        view! { <div class="text-sm opacity-70">{t!(i18n, loading)}</div> }
                    }>
                        {move || alerts.get().map(|r| match r {
                            Ok(rows) => {
                                let endpoint_list: Vec<Endpoint> = endpoints
                                    .get()
                                    .and_then(|r| r.ok())
                                    .unwrap_or_default();
                                let rows: Vec<Alert> = rows
                                    .into_iter()
                                    .filter(|a| trigger_matches_kind(&a.trigger, kind.get()))
                                    .collect();
                                if rows.is_empty() {
                                    view! {
                                        <p class="text-sm opacity-70">{t!(i18n, alert_drawer_active_empty)}</p>
                                    }.into_any()
                                } else {
                                    view! {
                                        <ul class="divide-y divide-[color:var(--color-outline)] rounded border border-[color:var(--color-outline)]">
                                            {rows.into_iter().map(|a| {
                                                let id = a.id;
                                                let description = match &a.trigger {
                                                    AlertTrigger::BelowThreshold { item_id, price_threshold, .. } => {
                                                        let name = tracked_data()
                                                            .items
                                                            .get(&ItemId(*item_id))
                                                            .map(|it| it.name.as_str().to_string())
                                                            .unwrap_or_else(|| format!("Item {item_id}"));
                                                        format!(
                                                            "{name} · {}",
                                                            t_string!(i18n, alert_drawer_threshold_below, price = *price_threshold)
                                                        )
                                                    }
                                                    AlertTrigger::RetainerUndercut { margin_percent } => {
                                                        format!(
                                                            "{} · {}",
                                                            t_string!(i18n, alerts_retainer_undercut_rule),
                                                            t_string!(i18n, alerts_margin_percent, margin = *margin_percent)
                                                        )
                                                    }
                                                    // Filtered out above; keep the match exhaustive.
                                                    _ => String::new(),
                                                };
                                                let endpoint_names = a
                                                    .endpoint_ids
                                                    .iter()
                                                    .map(|id| {
                                                        endpoint_list
                                                            .iter()
                                                            .find(|e| e.id == *id)
                                                            .map(|e| e.name.clone())
                                                            .unwrap_or_else(|| format!("#{id}"))
                                                    })
                                                    .collect::<Vec<_>>()
                                                    .join(", ");
                                                view! {
                                                    <li class="flex items-center justify-between gap-2 p-2">
                                                        <div class="min-w-0">
                                                            <div class="text-sm truncate">{description}</div>
                                                            <div class="text-xs opacity-60 truncate">{endpoint_names}</div>
                                                        </div>
                                                        <button
                                                            class="btn-ghost text-red-400"
                                                            aria-label=t_string!(i18n, alert_rules_aria_delete_alert)
                                                            on:click=move |_| remove(id)
                                                        >
                                                            <Icon icon=i::BiTrashSolid />
                                                        </button>
                                                    </li>
                                                }
                                            }).collect_view()}
                                        </ul>
                                    }.into_any()
                                }
                            }
                            Err(e) => view! {
                                <div class="text-sm text-red-500">{format!("{e}")}</div>
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
                            {move || match kind.get() {
                                AlertKind::ItemPrice => t_string!(i18n, alert_drawer_submit).to_string(),
                                AlertKind::Undercut => t_string!(i18n, undercut_alert_submit).to_string(),
                            }}
                        </span>
                    </button>
                </div>
            </div>
        </Modal>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_price_kind_matches_only_below_threshold() {
        let below = AlertTrigger::BelowThreshold {
            item_id: 5,
            world_selector: AnySelector::World(34),
            price_threshold: 1000,
            hq_only: false,
        };
        assert!(trigger_matches_kind(&below, AlertKind::ItemPrice));
        assert!(!trigger_matches_kind(&below, AlertKind::Undercut));
    }

    #[test]
    fn undercut_kind_matches_only_retainer_undercut() {
        let undercut = AlertTrigger::RetainerUndercut { margin_percent: 5 };
        assert!(trigger_matches_kind(&undercut, AlertKind::Undercut));
        assert!(!trigger_matches_kind(&undercut, AlertKind::ItemPrice));
    }

    #[test]
    fn list_scoped_alerts_never_show_in_drawer() {
        for trigger in [
            AlertTrigger::ListItemThreshold { list_id: 1 },
            AlertTrigger::ListUpdate { list_id: 1 },
        ] {
            assert!(!trigger_matches_kind(&trigger, AlertKind::ItemPrice));
            assert!(!trigger_matches_kind(&trigger, AlertKind::Undercut));
        }
    }
}
