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
/// * **Client-only.** The resource is a [`LocalResource`], so the server never
///   fetches or serializes it. The row-of-objects map is ~1.2 MB of JSON; as a
///   plain `Resource` it was inlined into every SSR page even though every
///   consumer already gates its read behind a post-hydration signal (the
///   `hydrated` idiom from #740) and so never used it on the server.
/// * **Lazy.** The resource does not exist until a consumer calls [`demand`].
///   Pages without a price cell (lists, alerts, settings, …) never pay for it;
///   pages with one fetch it once per zone for the SPA session.
///
/// The resource is *created* lazily rather than gated by a signal inside its
/// fetcher: `AsyncDerived` awaits the current future before it can pick up
/// the next source notification, so a "stay pending until wanted" future
/// would wedge the resource for good if the demand arrived after its task
/// started polling (the item explorer demands from inside another fetcher).
///
/// [`demand`]: CheapestPrices::demand
#[derive(Clone, Copy)]
pub struct CheapestPrices {
    /// Owner the resource is created under, so a demanding component's
    /// disposal (route change) does not take the shared resource with it.
    owner: StoredValue<Owner>,
    listings: StoredValue<Option<CheapestListingsResource>>,
}

impl Default for CheapestPrices {
    fn default() -> Self {
        Self::new()
    }
}

impl CheapestPrices {
    /// Call from the app root: the resource is later created under whatever
    /// owner is current here.
    pub fn new() -> Self {
        Self {
            owner: StoredValue::new(Owner::current().unwrap_or_default()),
            listings: StoredValue::new(None),
        }
    }

    /// Wraps a resource some scope built itself (e.g. the item explorer's
    /// `?world=` override) so descendants' [`demand`] calls hand back that one.
    ///
    /// [`demand`]: CheapestPrices::demand
    pub fn already_demanded(listings: CheapestListingsResource) -> Self {
        Self {
            owner: StoredValue::new(Owner::current().unwrap_or_default()),
            listings: StoredValue::new(Some(listings)),
        }
    }

    /// Returns the shared resource, creating (and so fetching) it on the first
    /// call. Call at component setup, not inside a render closure, and read
    /// the returned handle from there; it is `Copy`.
    pub fn demand(&self) -> CheapestListingsResource {
        if let Some(listings) = self.listings.get_value() {
            return listings;
        }
        let listings = self
            .owner
            .with_value(|owner| owner.with(Self::create_resource));
        self.listings.set_value(Some(listings));
        listings
    }

    fn create_resource() -> CheapestListingsResource {
        let (zone, _) = get_price_zone();
        LocalResource::new(move || {
            // Tracked, so a zone change refetches.
            let zone = zone.get();
            async move {
                let zone_name = zone
                    .as_ref()
                    .map(|w| w.get_name())
                    .unwrap_or("North-America");
                get_cheapest_listings(zone_name)
                    .await
                    .map(CheapestListingsMap::from)
            }
        })
    }
}
