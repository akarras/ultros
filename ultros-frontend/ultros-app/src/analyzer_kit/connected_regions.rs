//! Connected regions: the buy side of an analyzer reaching past the home
//! region into every region a character can data-center travel to.
//!
//! Only a *buy* can widen this way. A retainer lists on its own world, so a
//! sale price, a revenue estimate or a market statistic stays on the home
//! market; the other regions only ever add cheaper places to buy from.
//!
//! State lives in the URL like every other analyzer choice. Whether the tier is
//! on belongs to the caller (the Flip Finder's `?cross=true`, the shared scope
//! picker's `?market-scope=connected`); which of the other regions take part is
//! shared, one `?<Region>=false` opt-out per region, so a link that skipped a
//! region keeps skipping it on every analyzer that reads it.
use std::collections::HashMap;

use leptos::prelude::*;
use ultros_api_types::cheapest_listings::{CheapestListingItem, CheapestListings};

use crate::{
    api::get_cheapest_listings_live, columnar_wire::ColumnarJson, error::AppResult, i18n::*,
    query_defaults::filter_query_signal,
};

/// Regions a character can travel between. China, Korea and Taiwan run their
/// own services and are never connected.
pub const CONNECTED_REGIONS: &[&str] = &["Europe", "Japan", "North-America", "Oceania"];

/// Whether `region` can reach the others at all.
pub fn is_connected_region(region: &str) -> bool {
    CONNECTED_REGIONS.contains(&region)
}

/// The regions a character in `home` can travel to, excluding `home` itself.
/// Empty when `home` is not a connected region.
pub fn partner_regions(home: &str) -> Vec<&'static str> {
    if !is_connected_region(home) {
        return Vec::new();
    }
    CONNECTED_REGIONS
        .iter()
        .copied()
        .filter(|region| *region != home)
        .collect()
}

/// One board of cheapest listings covering `home` and `others`: the cheapest
/// listing per `(item, hq)` across all of them. A tie keeps the earlier board,
/// so the home region wins an equal price and nobody is sent abroad for
/// nothing. Rows come out in `(item_id, hq)` order, like the server's.
pub fn merge_cheapest_listings(
    home: &CheapestListings,
    others: &[CheapestListings],
) -> CheapestListings {
    if others.is_empty() {
        return home.clone();
    }
    let mut cheapest: HashMap<(i32, bool), CheapestListingItem> =
        HashMap::with_capacity(home.cheapest_listings.len());
    for listing in std::iter::once(home)
        .chain(others)
        .flat_map(|board| &board.cheapest_listings)
    {
        cheapest
            .entry((listing.item_id, listing.hq))
            .and_modify(|current| {
                if listing.cheapest_price < current.cheapest_price {
                    *current = listing.clone();
                }
            })
            .or_insert_with(|| listing.clone());
    }
    let mut cheapest_listings: Vec<_> = cheapest.into_values().collect();
    cheapest_listings.sort_unstable_by_key(|listing| (listing.item_id, listing.hq));
    CheapestListings { cheapest_listings }
}

/// For a page whose home board also prices a sale: the buy side's own board,
/// or `None` when no partner widens it and the home board already is one.
pub fn widened_listings(
    home: &CheapestListings,
    partners: &[CheapestListings],
) -> Option<CheapestListings> {
    (!partners.is_empty()).then(|| merge_cheapest_listings(home, partners))
}

/// The buy side's board from the two resources a page reads: the home board
/// and [`MarketScope::connected_listings`](super::scope::MarketScope::connected_listings).
/// `None` while either is still loading, so a table never renders home-only
/// prices under a chip that says connected regions. A home failure is the
/// page's error; a partner failure only means fewer places to buy from.
pub fn buy_listings(
    home: Option<AppResult<CheapestListings>>,
    partners: Option<AppResult<Vec<CheapestListings>>>,
) -> Option<AppResult<CheapestListings>> {
    let home = match home? {
        Ok(home) => home,
        Err(error) => return Some(Err(error)),
    };
    let partners = partners?.unwrap_or_default();
    Some(Ok(merge_cheapest_listings(&home, &partners)))
}

/// Fetch each region's board concurrently. A region that fails to load is
/// left out rather than failing the page: the home region's own board is
/// fetched separately and still prices every row, so a missing partner only
/// means fewer places to buy from.
pub async fn get_partner_listings(
    regions: Vec<&'static str>,
    refresh_version: u64,
) -> AppResult<Vec<CheapestListings>> {
    Ok(futures::future::join_all(
        regions
            .into_iter()
            .map(|region| get_cheapest_listings_live(region, refresh_version)),
    )
    .await
    .into_iter()
    .filter_map(Result::ok)
    .collect())
}

#[derive(Clone, Copy)]
struct RegionToggle {
    region: &'static str,
    /// `Some(false)` when opted out. Absent means included: the default is
    /// every connected region, and a default stays out of the URL.
    value: Memo<Option<bool>>,
    set: SignalSetter<Option<bool>>,
}

/// The home region plus the URL-backed per-region opt-outs.
#[derive(Clone, Copy)]
pub struct ConnectedRegions {
    home: Signal<Option<String>>,
    toggles: StoredValue<Vec<RegionToggle>>,
}

/// Create at the route owner, outside any reactive closure: each region owns
/// a query signal.
pub fn use_connected_regions(home: Signal<Option<String>>) -> ConnectedRegions {
    let toggles = CONNECTED_REGIONS
        .iter()
        .map(|&region| {
            let (value, set) = filter_query_signal::<bool>(region);
            RegionToggle { region, value, set }
        })
        .collect();
    ConnectedRegions {
        home,
        toggles: StoredValue::new(toggles),
    }
}

impl ConnectedRegions {
    /// Whether the home region can reach any other region.
    pub fn available(self) -> bool {
        self.home
            .with(|home| home.as_deref().is_some_and(is_connected_region))
    }

    /// Every other region the home region can reach, opted out or not.
    pub fn partners(self) -> Vec<&'static str> {
        self.home
            .with(|home| home.as_deref().map(partner_regions).unwrap_or_default())
    }

    pub fn is_included(self, region: &str) -> bool {
        self.toggles.with_value(|toggles| {
            toggles
                .iter()
                .find(|toggle| toggle.region == region)
                .is_none_or(|toggle| toggle.value.get() != Some(false))
        })
    }

    /// The partners to buy from: [`Self::partners`] minus the opt-outs.
    pub fn included(self) -> Vec<&'static str> {
        self.partners()
            .into_iter()
            .filter(|region| self.is_included(region))
            .collect()
    }

    pub fn set_included(self, region: &str, included: bool) {
        self.toggles.with_value(|toggles| {
            if let Some(toggle) = toggles.iter().find(|toggle| toggle.region == region) {
                toggle.set.set((!included).then_some(false));
            }
        });
    }

    /// The other regions' boards while `active` holds, none otherwise. The
    /// home region's own board is not included: every caller already fetches
    /// it, and it is what a failed partner falls back to.
    pub fn listings_resource(
        self,
        active: Signal<bool>,
    ) -> ArcResource<AppResult<Vec<CheapestListings>>, ColumnarJson> {
        crate::columnar_wire::columnar_resource(
            move || {
                if active.get() {
                    self.included()
                } else {
                    Vec::new()
                }
            },
            move |regions| get_partner_listings(regions, 0),
        )
    }
}

/// Inline control for a price chip whose market can widen to connected
/// regions. Off, it offers the widening in one click; on, it lists the other
/// regions as toggles so one can be dropped without leaving the chip.
#[component]
pub fn ConnectedRegionsControl(
    regions: ConnectedRegions,
    #[prop(into)] active: Signal<bool>,
    #[prop(into)] on_activate: Callback<()>,
) -> impl IntoView {
    let i18n = crate::i18n_fallback::use_i18n_or_default();
    move || {
        if !regions.available() {
            return None;
        }
        if !active.get() {
            return Some(
                view! {
                    <button type="button" class="connected-region-toggle"
                        data-testid="connected-regions-enable"
                        title=move || t_string!(i18n, connected_regions_enable_help).to_string()
                        on:click=move |_| on_activate.run(())>
                        {t!(i18n, connected_regions_enable)}
                    </button>
                }
                .into_any(),
            );
        }
        let partners = regions.partners();
        Some(
            view! {
                <span class="inline-flex flex-wrap items-center gap-1" role="group"
                    data-testid="connected-regions"
                    aria-label=move || t_string!(i18n, connected_regions_group).to_string()>
                    {partners.into_iter().map(|region| {
                        let included = move || regions.is_included(region);
                        // Dropping the last partner would leave the chip naming
                        // connected regions while pricing the home region only.
                        let last = move || included() && regions.included().len() == 1;
                        view! {
                            <button type="button" class="connected-region-toggle"
                                data-connected-region=region
                                aria-pressed=move || included().to_string()
                                disabled=last
                                title=move || {
                                    if included() {
                                        t_string!(i18n, connected_regions_exclude, region = region).to_string()
                                    } else {
                                        t_string!(i18n, connected_regions_include, region = region).to_string()
                                    }
                                }
                                on:click=move |_| regions.set_included(region, !included())>
                                {region}
                            </button>
                        }
                    }).collect_view()}
                </span>
            }
            .into_any(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(rows: &[(i32, bool, i32, i32)]) -> CheapestListings {
        CheapestListings {
            cheapest_listings: rows
                .iter()
                .map(
                    |&(item_id, hq, cheapest_price, world_id)| CheapestListingItem {
                        item_id,
                        hq,
                        cheapest_price,
                        world_id,
                    },
                )
                .collect(),
        }
    }

    #[test]
    fn partners_exclude_home_and_unconnected_regions_have_none() {
        assert_eq!(
            partner_regions("North-America"),
            vec!["Europe", "Japan", "Oceania"]
        );
        assert!(partner_regions("China").is_empty());
        assert!(!is_connected_region("Korea"));
    }

    #[test]
    fn merge_keeps_the_cheapest_listing_per_item_and_quality() {
        let home = board(&[(1, false, 100, 10), (1, true, 500, 10), (2, false, 50, 11)]);
        let europe = board(&[(1, false, 80, 20), (1, true, 600, 20), (3, false, 7, 21)]);
        let japan = board(&[(1, false, 90, 30), (2, false, 40, 31)]);
        let merged = merge_cheapest_listings(&home, &[europe, japan]);
        assert_eq!(
            merged,
            board(&[
                (1, false, 80, 20),
                (1, true, 500, 10),
                (2, false, 40, 31),
                (3, false, 7, 21),
            ])
        );
    }

    #[test]
    fn a_tie_stays_in_the_home_region() {
        let home = board(&[(1, false, 100, 10)]);
        let europe = board(&[(1, false, 100, 20)]);
        let merged = merge_cheapest_listings(&home, &[europe]);
        assert_eq!(merged, home);
    }

    #[test]
    fn no_partners_is_the_home_board() {
        let home = board(&[(2, true, 5, 1), (1, false, 9, 1)]);
        assert_eq!(merge_cheapest_listings(&home, &[]), home);
    }
}
