use ultros_api_types::cheapest_listings::{CheapestListings, CheapestListingsMap};

use leptos::*;

use crate::api::get_cheapest_listings;

use super::home_world::get_price_zone;

/// Maintains a set of the cheapest prices constantly.
#[derive(Copy, Clone)]
pub(crate) struct CheapestPrices {
    pub read_listings: Resource<
        Option<ultros_api_types::world_helper::OwnedResult>,
        Result<(CheapestListings, CheapestListingsMap), crate::error::AppError>,
    >,
}

impl CheapestPrices {
    pub fn new() -> Self {
        let (read, _) = get_price_zone();
        let read_listings = create_local_resource(read, move |world| async move {
            get_cheapest_listings(
                world
                    .as_ref()
                    .map(|w| w.get_name())
                    .unwrap_or("North-America"),
            )
            .await
            .map(|cheapest_prices| {
                let map = CheapestListingsMap::from(cheapest_prices.clone());
                (cheapest_prices, map)
            })
        });

        Self { read_listings }
    }
}
