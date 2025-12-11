use std::{sync::Arc, time::Duration};

use aide::axum::IntoApiResponse;
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use schemars::JsonSchema;
use serde::Serialize;
use ultros_db::world_cache::{AnySelector, WorldCache};

use crate::{analyzer_service::AnalyzerService, web::error::ApiError};

#[derive(Serialize, Debug, JsonSchema)]
pub(crate) struct CheapestListingData {
    item_id: i32,
    hq: bool,
    cheapest_price: i32,
    world_id: i32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct CheapestPerWorld {
    cheapest_listings: Vec<CheapestListingData>,
}

pub(crate) async fn cheapest_per_world(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world): Path<String>,
) -> Result<impl IntoApiResponse, ApiError> {
    let value = world_cache.lookup_value_by_name(&world)?;
    let selector = AnySelector::from(&value);
    let cheapest_listings = analyzer
        .read_cheapest_items(&selector, |listings| {
            listings
                .item_map
                .iter()
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
