//! Generic "Create alert" modal for the /alerts page. Mirrors
//! [`crate::components::alert_config_drawer::AlertConfigDrawer`] but lets the
//! user pick an arbitrary item via a search box instead of being scoped to a
//! single item id passed in by the caller. This is the entry point for users
//! who want to set up a price alert without first navigating to an item page.

use icondata as i;
use leptos::{prelude::*, reactive::wrappers::write::SignalSetter, task::spawn_local};
use std::cmp::Reverse;
use std::collections::HashSet;
use ultros_api_types::{
    alert::{AlertTrigger, CreateAlertRequest},
    icon_size::IconSize,
    world_helper::AnySelector,
};
use xiv_gen::{Item, ItemId};

use crate::api::{create_alert, list_endpoints};
use crate::components::{icon::Icon, item_icon::ItemIcon, modal::Modal, world_picker::WorldPicker};
use crate::global_state::toasts::use_toast;
use crate::global_state::xiv_data::tracked_data;
use crate::i18n::{t, t_string, use_i18n};

#[component]
pub fn CreateAlertDrawer(
    #[prop(into)] default_world: Signal<Option<AnySelector>>,
    set_visible: SignalSetter<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let (search, set_search) = signal::<String>("".into());
    let selected_item = RwSignal::<Option<(i32, String)>>::new(None);
    let (world, set_world) = signal::<Option<AnySelector>>(default_world.get_untracked());
    let (price_threshold, set_price_threshold) = signal::<String>("".into());
    let (hq_only, set_hq_only) = signal(false);
    let endpoints = Resource::new(|| (), |_| list_endpoints());
    let selected_endpoints = RwSignal::new(HashSet::<i32>::new());
    let (error, set_error) = signal::<Option<String>>(None);
    let toasts = use_toast();

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

    let toggle_endpoint = move |id: i32| {
        selected_endpoints.update(|s| {
            if !s.insert(id) {
                s.remove(&id);
            }
        });
    };

    let submit = move |_| {
        set_error.set(None);
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
        let endpoint_ids: Vec<i32> = selected_endpoints.get().into_iter().collect();
        if endpoint_ids.is_empty() {
            set_error.set(Some(
                t_string!(i18n, alert_drawer_err_endpoint_required).to_string(),
            ));
            return;
        }
        let req = CreateAlertRequest {
            trigger: AlertTrigger::BelowThreshold {
                item_id,
                world_selector,
                price_threshold: threshold,
                hq_only: hq_only.get(),
            },
            delivery: None,
            endpoint_ids,
            cooldown_seconds: None,
        };
        spawn_local(async move {
            match create_alert(req).await {
                Ok(_) => {
                    if let Some(t) = toasts {
                        t.success(t_string!(i18n, alert_drawer_created_toast));
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
            <div class="p-4 space-y-4 w-[28rem] max-h-[80vh] overflow-y-auto">
                <h2 class="text-xl font-bold">{t!(i18n, create_alert_title)}</h2>

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
                                        let is_sel = move || selected_endpoints.get().contains(&id);
                                        view! {
                                            <li>
                                                <label class="flex items-center gap-2">
                                                    <input
                                                        type="checkbox"
                                                        prop:checked=is_sel
                                                        on:change=move |_| toggle_endpoint(id)
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
                        <span class="ml-1">{t!(i18n, alert_drawer_submit)}</span>
                    </button>
                </div>
            </div>
        </Modal>
    }
}
