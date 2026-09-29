//! URL-backed relative pricing scope. The selected home world stays independent
//! from the market used for prices and optional market-statistics columns.
use leptos::prelude::*;

use super::{
    calculation::{CONNECTED_PLACE, CalculationPlace},
    connected_regions::{ConnectedRegions, PartnerListings, use_connected_regions},
};
use crate::{
    columnar_wire::ColumnarJson,
    error::AppResult,
    global_state::region_for_world::{use_datacenter_for_world, use_region_for_world},
    i18n::*,
    query_defaults::query_signal_or_default,
};

#[derive(Clone, Copy)]
pub struct MarketScope {
    /// The market prices and statistics are read from. The connected tier
    /// reads its home region here: only the buy side's listings widen (see
    /// [`MarketScope::connected_listings`]).
    pub name: Memo<String>,
    selected: Memo<String>,
    set: SignalSetter<Option<String>>,
    world: Signal<Option<String>>,
    datacenter: Signal<Option<String>>,
    region: Signal<String>,
    /// Present only for a scope that prices a purchase.
    connected: Option<ConnectedRegions>,
}

/// A scope for a market that is read, not bought from: world, datacenter or
/// region.
pub fn use_market_scope(world: Signal<Option<String>>) -> MarketScope {
    market_scope(world, false)
}

/// A scope for the listings a player *buys*, which adds a fourth tier: the
/// home region plus every region a character can travel to. Use only where
/// the listings price nothing but a purchase; a scope that also prices a sale
/// reads that side from [`MarketScope::name`], which stays home.
pub fn use_buy_market_scope(world: Signal<Option<String>>) -> MarketScope {
    market_scope(world, true)
}

fn market_scope(world: Signal<Option<String>>, allow_connected: bool) -> MarketScope {
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
    let connected =
        allow_connected.then(|| use_connected_regions(Signal::derive(move || Some(region.get()))));
    let selected = Memo::new(move |_| {
        match raw.get().as_deref() {
            Some("world") if world.get().is_some() => "world",
            Some("datacenter") if datacenter.get().is_some() => "datacenter",
            Some(CONNECTED_PLACE) if connected.is_some_and(|c| c.available()) => CONNECTED_PLACE,
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
        connected,
    }
}

impl MarketScope {
    /// Whether the buy side reaches into the connected regions.
    pub fn is_connected(self) -> bool {
        self.selected.get() == CONNECTED_PLACE
    }

    /// The other connected regions' cheapest listings while the connected
    /// tier is selected, and none otherwise. Merge them over the
    /// [`MarketScope::name`] board with
    /// [`merge_cheapest_listings`](super::connected_regions::merge_cheapest_listings)
    /// for the buy side only.
    pub fn connected_listings(self) -> ArcResource<AppResult<PartnerListings>, ColumnarJson> {
        match self.connected {
            Some(connected) => {
                connected.listings_resource(Signal::derive(move || self.is_connected()))
            }
            None => crate::columnar_wire::columnar_resource(
                || (),
                |_| async { Ok(PartnerListings::default()) },
            ),
        }
    }

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
                let region = self.region.get();
                let mut options = vec![
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
                    ("region", region.clone(), true),
                ];
                if let Some(connected) = self.connected {
                    options.push((
                        CONNECTED_PLACE,
                        t_string!(i18n, analyzer_scope_connected_regions, region = region)
                            .to_string(),
                        connected.available(),
                    ));
                }
                options
            }),
            on_change: Callback::new(move |value: String| self.set.set(Some(value))),
            connected: self.connected,
        }
    }
}
