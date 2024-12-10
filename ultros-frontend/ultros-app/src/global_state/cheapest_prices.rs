use leptos::prelude::*;
use ultros_api_types::cheapest_listings::CheapestListingsMap;

use crate::api::get_cheapest_listings;

use super::home_world::get_price_zone;

/// Maintains a set of the cheapest prices constantly.
#[derive(Clone)]
pub(crate) struct CheapestPrices {
    pub read_listings: Resource<Result<CheapestListingsMap, crate::error::AppError>>,
}

impl CheapestPrices {
    pub fn new() -> Self {
        let (read, _) = get_price_zone();
        let read_listings = Resource::new(
            move || {},
            move |_| async move {
                let world = read.get();
                get_cheapest_listings(
                    world
                        .as_ref()
                        .map(|w| w.get_name())
                        .unwrap_or("North-America"),
                )
                .await
                .map(|cheapest_prices| {
                    let map = CheapestListingsMap::from(cheapest_prices.clone());
                    map
                })
            },
        );

        Self { read_listings }
    }
}
