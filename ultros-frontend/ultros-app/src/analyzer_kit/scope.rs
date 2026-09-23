//! URL-backed relative pricing scope. The selected home world stays independent
//! from the market used for prices and optional market-statistics columns.
use leptos::prelude::*;

use super::calculation::CalculationPlace;
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
    world: Signal<Option<String>>,
    datacenter: Signal<Option<String>>,
    region: Signal<String>,
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
        world,
        datacenter: Signal::derive(move || datacenter.get()),
        region: Signal::derive(move || region.get()),
    }
}

impl MarketScope {
    /// The scope as a term chip's place picker: `Cost · [Aether ▾]`. Each
    /// option is labelled with the place it resolves to; a tier the selected
    /// world cannot resolve keeps its generic name and is disabled.
    pub fn place(self) -> CalculationPlace {
        let i18n = crate::i18n_fallback::use_i18n_or_default();
        CalculationPlace {
            value: self.selected.into(),
            options: Signal::derive(move || {
                let world = self.world.get();
                let datacenter = self.datacenter.get();
                vec![
                    (
                        "world",
                        world
                            .clone()
                            .unwrap_or_else(|| t_string!(i18n, analyzer_scope_world).to_string()),
                        world.is_some(),
                    ),
                    (
                        "datacenter",
                        datacenter.clone().unwrap_or_else(|| {
                            t_string!(i18n, analyzer_scope_datacenter).to_string()
                        }),
                        datacenter.is_some(),
                    ),
                    ("region", self.region.get(), true),
                ]
            }),
            on_change: Callback::new(move |value: String| self.set.set(Some(value))),
        }
    }
}
