//! `GET /api/v1/sale_stats/{worldDcOrRegion}` — bulk sale-history statistics.
//!
//! Returns min / median / mean per-unit sale price plus sample count for
//! every `(item_id, hq)` with sales in the trailing window, aggregated
//! across all worlds in the selector's scope (world, datacenter, or
//! region — same name resolution as `/api/v1/cheapest/{world}`).
//!
//! Consumed by the recipe analyzer's selectable cost basis (#1202): sale
//! statistics are a far more robust ingredient/revenue estimate than the
//! single cheapest current listing. Also carries the stats-column fields
//! (last sold, unit volume, vwap, sales/day) and — for single-world scopes
//! only — the per-world confidence band. The response is cached in-process
//! with single-flight refresh and stale fallback, then shared-cacheable for 5
//! minutes. Request volume therefore does not multiply ClickHouse work.

use std::{collections::HashMap, sync::Arc};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use serde::Deserialize;
use ultros_api_types::sale_stats::{BulkSaleStats, ItemSaleStats};
use ultros_api_types::trends::ConfidenceBand;
use ultros_clickhouse::ClickHouseClient;
use ultros_db::world_data::world_cache::{AnySelector, WorldCache};

use crate::web::{
    error::{ClickHouseQueryError, WebError},
    sale_stats_cache::{CacheDisposition, CacheKey, SaleStatsCache},
};

const DEFAULT_WINDOW_DAYS: u16 = 7;
const SUPPORTED_WINDOWS: [u16; 4] = [1, 7, 30, 90];

#[derive(Debug, Deserialize)]
pub(crate) struct SaleStatsQuery {
    /// Trailing rollup window in days. Supported: 1, 7, 30, 90; defaults to 7.
    window: Option<u16>,
}

pub(crate) async fn get_sale_stats(
    State(ch): State<ClickHouseClient>,
    State(world_cache): State<Arc<WorldCache>>,
    State(cache): State<SaleStatsCache>,
    Path(world): Path<String>,
    Query(query): Query<SaleStatsQuery>,
) -> Result<impl IntoResponse, WebError> {
    let value = world_cache.lookup_value_by_name(&world)?;
    let selector = AnySelector::from(&value);
    let world_ids = world_cache
        .get_all_worlds_in(&value)
        .ok_or(WebError::NotFound)?;
    let window_days = query.window.unwrap_or(DEFAULT_WINDOW_DAYS);
    if !SUPPORTED_WINDOWS.contains(&window_days) {
        return Err(WebError::BadRequest);
    }

    let cached = cache
        .get_or_load(
            CacheKey {
                selector,
                window_days,
            },
            move || async move { load_sale_stats(&ch, world_ids, window_days).await },
        )
        .await?;
    let disposition = match cached.disposition {
        CacheDisposition::Fresh => "fresh",
        CacheDisposition::Loaded => "loaded",
        CacheDisposition::Stale => "stale",
    };
    metrics::counter!(
        "ultros_sale_stats_cache_total",
        "disposition" => disposition
    )
    .increment(1);
    Ok(cached_response(cached.body, disposition))
}

async fn load_sale_stats(
    ch: &ClickHouseClient,
    world_ids: Vec<i32>,
    window_days: u16,
) -> Result<Bytes, WebError> {
    let rows = ultros_clickhouse::queries::bulk_sale_stats(ch, &world_ids, window_days)
        .await
        .map_err(|e| ClickHouseQueryError::new("bulk_sale_stats", e))?;
    // A new deployment creates the table before the elected scheduler has
    // finished its first seed. Signal a transient failure so the analyzer can
    // use its recent-sales failover instead of caching an empty market.
    if rows.is_empty() {
        return Err(WebError::TemporarilyUnavailable);
    }

    // Confidence bands are stored per world and don't compose across
    // worlds, so only a single-world scope carries them; datacenter and
    // region scopes report `Unknown`.
    let confidence: HashMap<(i32, bool), ConfidenceBand> = match world_ids.as_slice() {
        [only] => ultros_clickhouse::queries::bulk_confidence(ch, *only)
            .await
            .map_err(|e| ClickHouseQueryError::new("bulk_confidence", e))?
            .into_iter()
            .map(|r| ((r.item_id, r.hq != 0), r.confidence_band()))
            .collect(),
        _ => HashMap::new(),
    };

    let stats = rows
        .into_iter()
        .map(|r| ItemSaleStats {
            item_id: r.item_id,
            hq: r.hq != 0,
            min_price: r.min_price,
            median_price: r.median_price,
            avg_price: r.avg_price,
            num_sold: r.num_sold,
            last_sold_unix: r.last_sold_unix,
            units_sold: r.units_sold,
            vwap: r.vwap,
            sales_per_day: r.num_sold as f32 / window_days as f32,
            confidence: confidence
                .get(&(r.item_id, r.hq != 0))
                .copied()
                .unwrap_or_default(),
        })
        .collect();

    serde_json::to_vec(&BulkSaleStats { stats })
        .map(Bytes::from)
        .map_err(anyhow::Error::from)
        .map_err(Into::into)
}

fn cached_response(body: Bytes, disposition: &'static str) -> axum::response::Response {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/json".to_string(),
            ),
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=300, s-maxage=300, stale-while-revalidate=1800".to_string(),
            ),
            (
                axum::http::header::HeaderName::from_static("x-ultros-cache"),
                disposition.to_string(),
            ),
        ],
        body,
    )
        .into_response()
}
