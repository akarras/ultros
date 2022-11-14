use std::{sync::Arc};

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;

use crate::{
    analyzer_service::AnalyzerService,
    web::error::WebError,
    world_cache::{AnySelector, WorldCache},
};

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

pub(crate) async fn cheapest_per_world(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world): Path<String>,
) -> Result<Json<CheapestPerWorld>, WebError> {
    let value = world_cache.lookup_value_by_name(&world)?;
    let selector = AnySelector::from(&value);
    let cheapest_listings = analyzer
        .read_cheapest_items(|listings| {
            listings
                .get(&selector)
                .map(|listing| {
                    listing
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
                .unwrap_or_default()
        })
        .await?;
    Ok(Json(CheapestPerWorld { cheapest_listings }))
}
