//! URL-backed relative pricing scope. The selected home world stays independent
//! from the market used for prices and optional market-statistics columns.
use leptos::prelude::*;

use crate::{
    global_state::region_for_world::{use_datacenter_for_world, use_region_for_world},
    i18n::*,
    query_defaults::query_signal_or_default,
};

#[derive(Clone, Copy)]
pub struct MarketScope {
    pub name: Memo<String>,
    selected: Memo<String>,
    set: SignalSetter<Option<String>>,
    world_available: Memo<bool>,
    datacenter_available: Memo<bool>,
}

pub fn use_market_scope(world: Signal<Option<String>>) -> MarketScope {
    let region = use_region_for_world(move || world.get());
    let datacenter = use_datacenter_for_world(move || world.get());
    let (raw, set) = query_signal_or_default::<String>(
        "market-scope",
        leptos_router::NavigateOptions {
            replace: true,
            scroll: false,
            ..Default::default()
        },
    );
    let selected = Memo::new(move |_| {
        match raw.get().as_deref() {
            Some("world") if world.get().is_some() => "world",
            Some("datacenter") if datacenter.get().is_some() => "datacenter",
            _ => "region",
        }
        .to_string()
    });
    let name = Memo::new(move |_| match selected.get().as_str() {
        "world" => world.get().unwrap_or_else(|| region.get()),
        "datacenter" => datacenter.get().unwrap_or_else(|| region.get()),
        _ => region.get(),
    });
    MarketScope {
        name,
        selected,
        set,
        world_available: Memo::new(move |_| world.get().is_some()),
        datacenter_available: Memo::new(move |_| datacenter.get().is_some()),
    }
}

#[component]
pub fn MarketScopeControl(scope: MarketScope) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    view! {
        <label class="filter-chip" data-testid="analyzer-price-scope">
            <span>{t!(i18n, analyzer_price_scope)}</span>
            <select class="filter-chip-value"
                prop:value=move || scope.selected.get()
                on:change=move |ev| scope.set.set(Some(event_target_value(&ev)))>
                <option value="world" disabled=move || !scope.world_available.get() selected=move || scope.selected.get() == "world">{t!(i18n, analyzer_scope_world)}</option>
                <option value="datacenter" disabled=move || !scope.datacenter_available.get() selected=move || scope.selected.get() == "datacenter">{t!(i18n, analyzer_scope_datacenter)}</option>
                <option value="region" selected=move || scope.selected.get() == "region">{t!(i18n, analyzer_scope_region)}</option>
            </select>
            <span class="text-[color:var(--color-text-muted)]">{move || scope.name.get()}</span>
        </label>
    }
}
