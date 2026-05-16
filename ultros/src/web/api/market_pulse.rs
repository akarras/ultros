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
    let active_listings = active_listings.unwrap_or(0);

    // ClickHouse soft-fail: when the rollup query errors (CH down,
    // `ILLEGAL_AGGREGATION` on the world_kpi_5min CTE, etc.) we still
    // want to render the strip with the PG-backed `active_listings`
    // card and zeros for the time-series cards rather than 500ing the
    // whole home page. The frontend's normal zero-state rendering
    // already reads as a "quiet world" — delta chips fall back to "—"
    // when yesterday is zero. Log so the issue is still visible.
    let dto = match pulse {
        Ok(p) => MarketPulseDto {
            world_id: p.world_id,
            sales_today: p.sales_today,
            sales_yesterday: p.sales_yesterday,
            gil_volume_today: p.gil_volume_today,
            gil_volume_yesterday: p.gil_volume_yesterday,
            unit_volume_today: p.unit_volume_today,
            unit_volume_yesterday: p.unit_volume_yesterday,
            active_listings,
        },
        Err(e) => {
            tracing::warn!(error = ?e, world_id, "market_pulse CH query failed, returning quiet placeholders");
            MarketPulseDto {
                world_id,
                sales_today: 0,
                sales_yesterday: 0,
                gil_volume_today: 0,
                gil_volume_yesterday: 0,
                unit_volume_today: 0,
                unit_volume_yesterday: 0,
                active_listings,
            }
        }
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
