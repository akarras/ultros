//! `/api/v1/market_pulse/{world}` — feeds the home-page Market Pulse strip.
//!
//! Pulls the 24h / 24-48h rollups from ClickHouse plus a snapshot count of
//! active listings from Postgres. Single request fills all four KPI cards.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use ultros_api_types::market_pulse::MarketPulseDto;
use ultros_clickhouse::ClickHouseClient;
use ultros_db::{
    UltrosDb,
    entity::active_listing,
    world_data::world_cache::{AnySelector, WorldCache},
};

use crate::web::error::WebError;

pub(crate) async fn get_market_pulse(
    State(ch): State<ClickHouseClient>,
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world_name): Path<String>,
) -> Result<impl IntoResponse, WebError> {
    let world = world_cache.lookup_value_by_name(&world_name)?;
    let world_id = match AnySelector::from(&world) {
        AnySelector::World(id) => id,
        // Market Pulse is per-world only — DC/Region rollups would need a
        // separate query path. Reject other selector types up front.
        _ => return Err(WebError::BadRequest),
    };

    // Two queries concurrently: ClickHouse rollup + PG active-listing count.
    // Both are fast (sub-100ms typical) and fully independent.
    let ch_future = ultros_clickhouse::queries::market_pulse(&ch, world_id);
    let pg_future = active_listing::Entity::find()
        .filter(active_listing::Column::WorldId.eq(world_id))
        .count(db.get_connection());

    let (pulse, active_listings) = tokio::join!(ch_future, pg_future);
    let pulse = pulse.map_err(|e| {
        tracing::warn!(error = ?e, world_id, "market_pulse CH query failed");
        anyhow::anyhow!("ClickHouse market_pulse query failed: {e}")
    })?;
    let active_listings = active_listings.unwrap_or(0);

    let dto = MarketPulseDto {
        world_id: pulse.world_id,
        sales_today: pulse.sales_today,
        sales_yesterday: pulse.sales_yesterday,
        gil_volume_today: pulse.gil_volume_today,
        gil_volume_yesterday: pulse.gil_volume_yesterday,
        unit_volume_today: pulse.unit_volume_today,
        unit_volume_yesterday: pulse.unit_volume_yesterday,
        active_listings,
    };

    // 30s cache: the 5-min refresh interval on the rollup is the lower
    // bound on how often the underlying data changes. 30s is a safe
    // browser/CDN cache that still feels live.
    let mut response = Json(dto).into_response();
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(30)));
    Ok(response)
}
