//! `/api/v1/movers/{world}` and `/api/v1/sparklines/{world}` — the data
//! feed for the home-page Market Movers list + sparkline-bearing rows
//! anywhere else (Top Deals retrofit, Continue Tracking, etc).

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use serde::Deserialize;
use ultros_api_types::{
    sparklines::{
        MoverItem, MoversResponse, SparklineSeries, SparklinesRequest, SparklinesResponse,
    },
    world_helper::{AnySelector, WorldHelper},
};
use ultros_clickhouse::{ClickHouseClient, queries::MoverDirection};

use crate::web::error::WebError;

#[derive(Debug, Deserialize)]
pub(crate) struct MoversQuery {
    /// `rising` (default), `falling`, or `volume`.
    direction: Option<String>,
    /// Result count; clamped to [1, 50]. Default 10.
    limit: Option<u32>,
}

pub(crate) async fn get_movers(
    State(ch): State<ClickHouseClient>,
    State(world_helper): State<Arc<WorldHelper>>,
    Path(world_name): Path<String>,
    Query(q): Query<MoversQuery>,
) -> Result<impl IntoResponse, WebError> {
    let world = world_helper
        .lookup_world_by_name(&world_name)
        .ok_or(WebError::NotFound)?;
    let world_id = match AnySelector::from(&world) {
        AnySelector::World(id) => id,
        _ => return Err(WebError::BadRequest),
    };

    // Parse direction with a default. Reject unknown values rather than
    // silently coercing to rising, so the frontend can rely on round-trip
    // fidelity.
    let direction_str = q
        .direction
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "rising".to_string());
    let direction = match direction_str.as_str() {
        "rising" => MoverDirection::Rising,
        "falling" => MoverDirection::Falling,
        "volume" => MoverDirection::Volume,
        _ => return Err(WebError::BadRequest),
    };
    let limit = q.limit.unwrap_or(10).clamp(1, 50);

    let rows = ultros_clickhouse::queries::top_movers(&ch, world_id, direction, limit)
        .await
        .map_err(|e| {
            tracing::warn!(error = ?e, world_id, "top_movers CH query failed");
            anyhow::anyhow!("ClickHouse top_movers query failed: {e}")
        })?;

    // For each mover, fetch the 24h sparkline so the response is one
    // round trip from the frontend's perspective.
    let scan_req: Vec<(i32, u8, i32)> =
        rows.iter().map(|m| (m.item_id, m.hq, m.world_id)).collect();
    let sparkline_rows = ultros_clickhouse::queries::sparklines_batch(&ch, &scan_req, 24)
        .await
        .unwrap_or_default();
    let mut spark_by_key: std::collections::HashMap<(i32, u8, i32), Vec<u32>> = sparkline_rows
        .into_iter()
        .map(|s| ((s.item_id, s.hq, s.world_id), s.points))
        .collect();

    let items: Vec<MoverItem> = rows
        .into_iter()
        .map(|r| MoverItem {
            item_id: r.item_id,
            hq: r.hq != 0,
            world_id: r.world_id,
            price_now: r.price_now,
            pct_change_24h: r.pct_change_24h,
            volume_24h: r.volume_24h,
            sparkline: spark_by_key
                .remove(&(r.item_id, r.hq, r.world_id))
                .unwrap_or_else(|| vec![0; 24]),
        })
        .collect();

    let mut response = Json(MoversResponse {
        world_id,
        direction: direction_str,
        items,
    })
    .into_response();
    // Sales_hourly refreshes every 15 min; a 60s browser cache stays
    // ahead of the data without paying CH every page-load.
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(60)));
    Ok(response)
}

/// POST /api/v1/sparklines/{world} — bulk sparkline fetch by item id list.
pub(crate) async fn post_sparklines(
    State(ch): State<ClickHouseClient>,
    State(world_helper): State<Arc<WorldHelper>>,
    Path(world_name): Path<String>,
    Json(req): Json<SparklinesRequest>,
) -> Result<impl IntoResponse, WebError> {
    let world = world_helper
        .lookup_world_by_name(&world_name)
        .ok_or(WebError::NotFound)?;
    let world_id = match AnySelector::from(&world) {
        AnySelector::World(id) => id,
        _ => return Err(WebError::BadRequest),
    };
    if req.items.is_empty() {
        return Ok(Json(SparklinesResponse {
            world_id,
            series: vec![],
        })
        .into_response());
    }
    if req.items.len() > 200 {
        // The tuple-IN clause is fine up to ~thousands, but 200 covers
        // every reasonable page-of-rows use case and keeps the response
        // payload bounded.
        return Err(WebError::BadRequest);
    }
    let hours = req.hours.unwrap_or(24).clamp(6, 168);
    let scan_req: Vec<(i32, u8, i32)> = req
        .items
        .iter()
        .map(|(item, hq)| (*item, *hq as u8, world_id))
        .collect();

    let rows = ultros_clickhouse::queries::sparklines_batch(&ch, &scan_req, hours)
        .await
        .map_err(|e| {
            tracing::warn!(error = ?e, world_id, "sparklines_batch CH query failed");
            anyhow::anyhow!("ClickHouse sparklines query failed: {e}")
        })?;

    let series: Vec<SparklineSeries> = rows
        .into_iter()
        .map(|r| SparklineSeries {
            item_id: r.item_id,
            hq: r.hq != 0,
            world_id: r.world_id,
            points: r.points,
            first_price: r.first_price,
            last_price: r.last_price,
        })
        .collect();

    let mut response = Json(SparklinesResponse { world_id, series }).into_response();
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(60)));
    Ok(response)
}
