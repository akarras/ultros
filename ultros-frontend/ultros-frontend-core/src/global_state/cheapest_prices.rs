use leptos::prelude::*;
use ultros_api_types::cheapest_listings::CheapestListingsMap;

use crate::{api::get_cheapest_listings, error::AppError};

use super::home_world::get_price_zone;

pub type CheapestListingsResource = LocalResource<Result<CheapestListingsMap, AppError>>;

/// The cheapest listing per `(item, hq)` for the visitor's price zone,
/// shared by every price cell on the site.
///
/// Two deliberate properties:
///
/// * **Client-only.** This is a [`LocalResource`], so the server never fetches
///   or serializes it. The row-of-objects map is ~1.2 MB of JSON; as a plain
///   `Resource` it was inlined into every SSR page even though every consumer
///   already gates its read behind a post-hydration signal (the `hydrated`
///   idiom from #740) and so never used it on the server.
/// * **Lazy.** Nothing is fetched until a consumer calls [`demand`]. Pages
///   without a price cell (lists, alerts, settings, …) never pay for it; pages
///   with one fetch it once per zone for the SPA session.
///
/// [`demand`]: CheapestPrices::demand
#[derive(Clone, Copy)]
pub struct CheapestPrices {
    listings: CheapestListingsResource,
    wanted: RwSignal<bool>,
}

impl Default for CheapestPrices {
    fn default() -> Self {
        Self::new()
    }
}

impl CheapestPrices {
    pub fn new() -> Self {
        let (zone, _) = get_price_zone();
        let wanted = RwSignal::new(false);
        let listings = LocalResource::new(move || {
            // Both reads are tracked: flipping `wanted` re-runs the fetcher,
            // which drops the never-resolving future below and issues the
            // real request; a zone change refetches as before.
            let wanted = wanted.get();
            let zone = zone.get();
            async move {
                if !wanted {
                    // Nothing on this page has asked for prices: stay pending
                    // (consumers read `None` and show their skeleton) and
                    // never hit the network.
                    std::future::pending::<()>().await;
                }
                let zone_name = zone
                    .as_ref()
                    .map(|w| w.get_name())
                    .unwrap_or("North-America");
                get_cheapest_listings(zone_name)
                    .await
                    .map(CheapestListingsMap::from)
            }
        });
        Self { listings, wanted }
    }

    /// Wraps a resource some scope built itself (e.g. the item explorer's
    /// `?world=` override) so descendants' [`demand`] calls are no-ops.
    ///
    /// [`demand`]: CheapestPrices::demand
    pub fn already_demanded(listings: CheapestListingsResource) -> Self {
        Self {
            listings,
            wanted: RwSignal::new(true),
        }
    }

    /// Marks the map as needed on this page and returns the resource. Call
    /// once at component setup (not inside a render closure) and read the
    /// returned handle from there; it is `Copy`.
    pub fn demand(&self) -> CheapestListingsResource {
        if !self.wanted.get_untracked() {
            self.wanted.set(true);
        }
        self.listings
    }
}
