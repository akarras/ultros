//! `GET /api/v1/sale_stats/{worldDcOrRegion}` — bulk sale-history statistics.
//!
//! Returns min / median / mean per-unit sale price plus sample count for
//! every `(item_id, hq)` with sales in the trailing window, aggregated
//! across all worlds in the selector's scope (world, datacenter, or
//! region — same name resolution as `/api/v1/cheapest/{world}`).
//!
//! Consumed by the recipe analyzer's selectable cost basis (#1202): sale
//! statistics are a far more robust ingredient/revenue estimate than the
//! single cheapest current listing. No in-process cache — the response is
//! edge/browser cacheable for 5 minutes, which bounds how often the
//! ClickHouse scan reruns per selector.

use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use serde::Deserialize;
use ultros_api_types::sale_stats::{BulkSaleStats, ItemSaleStats};
use ultros_clickhouse::ClickHouseClient;
use ultros_db::world_data::world_cache::WorldCache;

use crate::web::error::{ClickHouseQueryError, WebError};

const MIN_WINDOW_DAYS: u16 = 1;
const MAX_WINDOW_DAYS: u16 = 90;
const DEFAULT_WINDOW_DAYS: u16 = 7;

#[derive(Debug, Deserialize)]
pub(crate) struct SaleStatsQuery {
    /// Trailing window in days. Clamped to `1..=90`; defaults to 7.
    window: Option<u16>,
}

pub(crate) async fn get_sale_stats(
    State(ch): State<ClickHouseClient>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world): Path<String>,
    Query(query): Query<SaleStatsQuery>,
) -> Result<impl IntoResponse, WebError> {
    let value = world_cache.lookup_value_by_name(&world)?;
    let world_ids = world_cache
        .get_all_worlds_in(&value)
        .ok_or(WebError::NotFound)?;
    let window_days = query
        .window
        .unwrap_or(DEFAULT_WINDOW_DAYS)
        .clamp(MIN_WINDOW_DAYS, MAX_WINDOW_DAYS);

    let rows = ultros_clickhouse::queries::bulk_sale_stats(&ch, &world_ids, window_days)
        .await
        .map_err(|e| ClickHouseQueryError::new("bulk_sale_stats", e))?;

    let stats = rows
        .into_iter()
        .map(|r| ItemSaleStats {
            item_id: r.item_id,
            hq: r.hq != 0,
            min_price: r.min_price,
            median_price: r.median_price,
            avg_price: r.avg_price,
            num_sold: r.num_sold,
        })
        .collect();

    let mut response = Json(BulkSaleStats { stats }).into_response();
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(300)));
    Ok(response)
}
