//! One travel limit for both Build estimates and candidate Shop routes.
//! State belongs to the editor: changing this control never adopts a trip.
// TODO(#1480): rendered only through `list_travel_state::ListTravelPanel`,
// which no list editor mounts yet.
#![allow(dead_code)]
use crate::i18n::{t_string, use_i18n};
use leptos::prelude::*;
use ultros_calc::list_travel::{TravelBlocked, TravelLimit};

#[component]
pub fn ListTravelControls(
    limit: Signal<TravelLimit>,
    on_change: Callback<TravelLimit>,
    home_world: Signal<Option<String>>,
    home_datacenter: Signal<Option<String>>,
    #[prop(optional)] blocked: Option<Signal<Option<TravelBlocked>>>,
    #[prop(optional)] on_clear_unresolved: Option<Callback<()>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let unavailable = move || {
        if let Some(blocked) = blocked {
            return blocked.get().map(|reason| match reason {
                TravelBlocked::WorldData => {
                    t_string!(i18n, list_travel_missing_metadata).to_string()
                }
                TravelBlocked::UnknownDatacenter => {
                    t_string!(i18n, list_travel_unknown_datacenter).to_string()
                }
                TravelBlocked::HomeWorld => t_string!(i18n, list_travel_missing_world).to_string(),
                TravelBlocked::HomeDatacenter => {
                    t_string!(i18n, list_travel_missing_datacenter).to_string()
                }
            });
        }
        match limit.get() {
            TravelLimit::Scope => None,
            _ if home_world.get().is_none() => {
                Some(t_string!(i18n, list_travel_missing_world).to_string())
            }
            TravelLimit::Datacenter if home_datacenter.get().is_none() => {
                Some(t_string!(i18n, list_travel_missing_datacenter).to_string())
            }
            _ => None,
        }
    };
    view! {
        <div class="min-w-0 space-y-1" data-testid="list-travel-controls">
            <label class="flex flex-wrap items-center gap-2 text-sm">
                <span>{move || t_string!(i18n, list_travel_label)}</span>
                <select class="input min-h-11 min-w-0 max-w-full" data-testid="list-travel-limit"
                    prop:value=move || limit.get().to_string()
                    on:change=move |event| {
                        if let Ok(value) = event_target_value(&event).parse::<TravelLimit>() {
                            on_change.run(value);
                        }
                    }>
                    <option value="scope">{move || t_string!(i18n, list_travel_scope)}</option>
                    <option value="world" disabled=move || home_world.get().is_none()>{move || home_world.get().map(|name| t_string!(i18n, list_travel_world, name = name).to_string()).unwrap_or_else(|| t_string!(i18n, list_travel_world_unset).to_string())}</option>
                    <option value="dc" disabled=move || home_world.get().is_none() || home_datacenter.get().is_none()>{move || home_datacenter.get().map(|name| t_string!(i18n, list_travel_datacenter, name = name).to_string()).unwrap_or_else(|| t_string!(i18n, list_travel_datacenter_unset).to_string())}</option>
                </select>
            </label>
            <p role="status" class="text-sm text-[color:var(--color-text-muted)]" data-testid="list-travel-unavailable" class:hidden=move || unavailable().is_none()>{unavailable}</p>
            <Show when=move || on_clear_unresolved.is_some() && blocked.is_some_and(|reason| reason.get() == Some(TravelBlocked::UnknownDatacenter))>
                <button type="button" class="btn-secondary min-h-11" data-testid="list-travel-clear-unresolved" on:click=move |_| { if let Some(clear) = on_clear_unresolved { clear.run(()); } }>{move || t_string!(i18n, list_travel_clear_unknown)}</button>
            </Show>
        </div>
    }
}
