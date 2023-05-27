use ultros_api_types::cheapest_listings::CheapestListings;

use leptos::*;

use crate::api::get_cheapest_listings;

use super::home_world::get_price_zone;

/// Maintains a set of the cheapest prices constantly.
#[derive(Copy, Clone)]
pub(crate) struct CheapestPrices {
    pub read_listings: Resource<
        Option<ultros_api_types::world_helper::OwnedResult>,
        Result<CheapestListings, crate::error::AppError>,
    >,
}

impl CheapestPrices {
    pub fn new(cx: Scope) -> Self {
        let (read, _) = get_price_zone(cx);
        let read_listings = create_resource(cx, read, move |world| async move {
            get_cheapest_listings(
                cx,
                world
                    .as_ref()
                    .map(|w| w.get_name())
                    .unwrap_or("North-America"),
            )
            .await
        });

        Self { read_listings }
    }
}
