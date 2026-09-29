//! `/api/v1/market_pulse/{world}` — feeds the home-page Market Pulse strip.
//!
//! Pulls the 24h / 24-48h rollups from ClickHouse plus a snapshot count of
//! active listings from Postgres. Single request fills all four KPI cards.
//! `{world}` may also name a datacenter or region, which sums its worlds.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use ultros_api_types::market_pulse::MarketPulseDto;
use ultros_clickhouse::ClickHouseClient;
use ultros_db::{
    UltrosDb,
    entity::active_listing,
    world_data::world_cache::{AnySelector, WorldCache},
};

use crate::web::{cached_json, error::WebError, home_feed_cache::HomeFeedCache};

pub(crate) async fn get_market_pulse(
    State(ch): State<ClickHouseClient>,
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    State(cache): State<HomeFeedCache>,
    Path(world_name): Path<String>,
) -> Result<impl IntoResponse, WebError> {
    let scope = world_cache.lookup_value_by_name(&world_name)?;
    // A world reports as itself; a datacenter or region sums its worlds and
    // reports `world_id: 0` (the logged-out home page shows a whole region).
    let (scope_id, world_ids) = match AnySelector::from(&scope) {
        AnySelector::World(id) => (id, vec![id]),
        _ => (0, world_cache.get_all_worlds_in(&scope).unwrap_or_default()),
    };
    let ttl = Duration::from_secs(30);
    let cache_key = format!("pulse:{}", scope.get_name());
    if let Some(body) = cache.get(&cache_key) {
        return Ok(cached_json(body, ttl));
    }

    // Two queries concurrently: ClickHouse rollup + PG active-listing count.
    // Both are fast (sub-100ms typical) and fully independent.
    let ch_future = ultros_clickhouse::queries::market_pulse(&ch, scope_id, &world_ids);
    let pg_future = active_listing::Entity::find()
        .filter(active_listing::Column::WorldId.is_in(world_ids.clone()))
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
    let soft_failed = pulse.is_err();
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
            tracing::warn!(error = ?e, scope_id, "market_pulse CH query failed, returning quiet placeholders");
            MarketPulseDto {
                world_id: scope_id,
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

    // 30s TTL: the 5-min refresh interval on the rollup is the lower bound
    // on how often the underlying data changes, so 30s still feels live. A
    // soft-failed placeholder is not cached, so the strip recovers as soon
    // as ClickHouse does.
    let body = serde_json::to_string(&dto).map_err(anyhow::Error::from)?;
    if !soft_failed {
        cache.insert(cache_key, body.clone(), ttl);
    }
    Ok(cached_json(body, ttl))
}
