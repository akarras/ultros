use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use axum::{Json, extract::State, response::IntoResponse};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use tracing::warn;
use ultros_api_types::world::WorldData;
use ultros_db::world_cache::WorldCache;

use crate::web::country_code_decoder::Region;

pub(crate) async fn world_data(State(world_cache): State<Arc<WorldCache>>) -> impl IntoResponse {
    static ONCE: OnceLock<WorldData> = OnceLock::new();
    let world_data = ONCE.get_or_init(move || WorldData::from(world_cache.as_ref()));
    let mut response = Json(world_data).into_response();
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(60 * 60 * 24)));
    response
}

/// Returns a region- attempts to guess it from the CF Region header
pub(crate) async fn detect_region(region: Option<Region>) -> impl IntoResponse {
    if region.is_none() {
        warn!("Unable to detect region");
    }
    let mut response = region.unwrap_or(Region::NorthAmerica).into_response();
    response.headers_mut().typed_insert(
        CacheControl::new()
            .with_private()
            .with_max_age(Duration::from_secs(604800)),
    );
    response
}
