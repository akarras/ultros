use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ultros_db::world_cache::{AnySelector, WorldCache};

use crate::{analyzer_service::AnalyzerService, web::error::WebError};

#[derive(Serialize, Debug)]
struct CheapestListingData {
    item_id: i32,
    hq: bool,
    cheapest_price: i32,
    world_id: i32,
}

#[derive(Debug, Serialize)]
pub(crate) struct CheapestPerWorld {
    cheapest_listings: Vec<CheapestListingData>,
}

#[derive(Deserialize)]
pub(crate) struct Filter {
    #[serde(default)]
    exclude: Vec<String>,
}

pub(crate) async fn cheapest_per_world(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world): Path<String>,
    Query(filter): Query<Filter>,
) -> Result<impl IntoResponse, WebError> {
    let value = world_cache.lookup_value_by_name(&world)?;
    let selector = AnySelector::from(&value);
    let mut excluded_worlds = HashSet::new();
    for s in filter.exclude {
        if let Ok(value) = world_cache.lookup_value_by_name(&s) {
            excluded_worlds.insert(AnySelector::from(&value));
        }
    }
    let cheapest_listings = analyzer
        .read_cheapest_items(&selector, |listings| {
            listings
                .item_map
                .iter()
                .filter(|(_, v)| !excluded_worlds.contains(&AnySelector::World(v.world_id)))
                .map(|(i, v)| CheapestListingData {
                    item_id: i.item_id,
                    hq: i.hq,
                    cheapest_price: v.price,
                    world_id: v.world_id,
                })
                .collect::<Vec<_>>()
        })
        .await?;
    let mut response = Json(CheapestPerWorld { cheapest_listings }).into_response();
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(15)));
    Ok(response)
}
