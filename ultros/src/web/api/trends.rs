use std::sync::Arc;

use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use tracing::instrument;
use ultros_api_types::trends::TrendsData;
use ultros_db::world_cache::{AnySelector, WorldCache};

use crate::{analyzer_service::AnalyzerService, web::error::WebError};

#[instrument(skip(analyzer, world_cache))]
pub async fn get_trends(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world_name): Path<String>,
) -> Result<Json<TrendsData>, WebError> {
    let selector = world_cache
        .lookup_value_by_name(&world_name)
        .map_err(|_| WebError::NotFound)?;
    let selector = AnySelector::from(&selector);

    // Currently only supporting trends for specific Worlds, as AnalyzerService::get_trends takes a world_id
    // If we want DC trends, we'd need to aggregate or the AnalyzerService needs to support it.
    // For now, if it's a datacenter, we error or pick a default?
    // Let's stick to World for now, or map DC to its worlds and aggregate?
    // Aggregating is expensive. Let's just enforce World for V1.

    let world_id = match selector {
        AnySelector::World(id) => id,
        // TODO: Implement Data Center aggregation.
        // This is computationally expensive to do on-the-fly. Consider pre-aggregating or
        // caching DC trends in the background worker.
        _ => return Err(WebError::BadRequest),
    };

    // If get_trends returns None, it means the analyzer is not fully initialized or data is missing for that world.
    // Instead of a hard InternalError, we return an empty dataset so the UI can handle it gracefully.
    let trends = analyzer.get_trends(world_id).await.unwrap_or(TrendsData {
        high_velocity: vec![],
        rising_price: vec![],
        falling_price: vec![],
    });

    Ok(Json(trends))
}
