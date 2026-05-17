//! `/api/v1/market_heat/{world}` — feeds the home-page Market Heat band.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use ultros_api_types::{
    market_heat::{CategoryHeat, HeatBand, MarketHeatResponse},
    world_helper::{AnySelector, WorldHelper},
};
use ultros_clickhouse::ClickHouseClient;

use crate::web::error::WebError;

pub(crate) async fn get_market_heat(
    State(ch): State<ClickHouseClient>,
    State(world_helper): State<Arc<WorldHelper>>,
    Path(world_name): Path<String>,
) -> Result<impl IntoResponse, WebError> {
    let world = world_helper
        .lookup_world_by_name(&world_name)
        .ok_or(WebError::NotFound)?;
    let world_id = match AnySelector::from(&world) {
        AnySelector::World(id) => id,
        _ => return Err(WebError::BadRequest),
    };

    let rows = ultros_clickhouse::queries::category_heat(&ch, world_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = ?e, world_id, "market_heat CH query failed");
            anyhow::anyhow!("ClickHouse market_heat query failed: {e}")
        })?;

    // The query returns only categories that had activity. We pad to the
    // full 5-category set so the frontend layout stays stable across
    // worlds with sparse data — a Cool/NoData row is more useful than a
    // missing slot.
    let mut by_id: std::collections::HashMap<u8, _> =
        rows.iter().map(|r| (r.category_id, r.clone())).collect();
    let categories: Vec<CategoryHeat> = (1u8..=5u8)
        .map(|id| {
            let r = by_id.remove(&id);
            let (item_count, pct, vol) = match &r {
                Some(r) => (r.item_count, r.avg_pct_change_24h, r.gil_volume_24h),
                None => (0u32, 0.0, 0u64),
            };
            CategoryHeat {
                category_id: id,
                item_count,
                avg_pct_change_24h: pct,
                gil_volume_24h: vol,
                band: HeatBand::from_pct(pct, item_count),
            }
        })
        .collect();

    let mut response = Json(MarketHeatResponse {
        world_id,
        categories,
    })
    .into_response();
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(60)));
    Ok(response)
}
