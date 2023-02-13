use ultros_api_types::cheapest_listings::CheapestListings;

use leptos::*;

use crate::api::get_cheapest_listings;

/// Maintains a set of the cheapest prices constantly.
#[derive(Copy, Clone)]
pub(crate) struct CheapestPrices {
    pub read_listings: ReadSignal<CheapestListings>,
}

impl CheapestPrices {
    pub fn new(
        cx: Scope,
        read_listings: ReadSignal<CheapestListings>,
        write_listings: WriteSignal<CheapestListings>,
    ) -> Self {
        leptos::spawn_local(async move {
            // TODO: This world name should be configurable
            let listings = get_cheapest_listings(cx, "North-America").await;
            log::info!("fetching listings {listings:?}");
            write_listings(listings.unwrap_or_default());
        });

        Self { read_listings }
    }
}
