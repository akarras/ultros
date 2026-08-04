//! `/api/v1/item_stats/{world}/{item_id}` — per-item analyzer stats for
//! the item view's confidence chip.
//!
//! `{world}` may name a world, a datacenter, or a region; a multi-world scope
//! is folded together by
//! [`ultros_clickhouse::queries::aggregate_item_stats_variants`], which
//! documents how each field combines.
//!
//! Returns deep-scan rollup data for both HQ and NQ variants in one request.
//! The frontend renders a ConfidenceBadge that summarises sample size +
//! launder suspicion for the user.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use ultros_api_types::{
    item_stats::ItemStatsResponse,
    world_helper::{AnySelector, WorldHelper},
};
use ultros_clickhouse::ClickHouseClient;

use crate::web::error::WebError;

pub(crate) async fn get_item_stats(
    State(ch): State<ClickHouseClient>,
    State(world_helper): State<Arc<WorldHelper>>,
    Path((world_name, item_id)): Path<(String, i32)>,
) -> Result<impl IntoResponse, WebError> {
    let scope = world_helper
        .lookup_world_by_name(&world_name)
        .ok_or(WebError::NotFound)?;
    // The rollup is keyed by world, so a datacenter or region has to fan out
    // to its member worlds and fold the rows back together. This used to
    // reject anything but a single world, which 400'd the request the item
    // view fires on *every* datacenter- and region-scoped page load — the
    // confidence badge soft-fails to nothing, so the whole chip was silently
    // dead at those scopes even though the chart beside it (`price_series`)
    // has always accepted them.
    let world_ids: Vec<i32> = scope.all_worlds().map(|w| w.id).collect();
    if world_ids.is_empty() {
        return Err(WebError::NotFound);
    }

    // Ask CH for both quality variants of every world in one round trip.
    // Missing variants (one of NQ/HQ doesn't exist, or a world has no sales)
    // yield zero rows, not an error.
    let requests: Vec<(i32, u8, i32)> = world_ids
        .iter()
        .flat_map(|world_id| [(item_id, 0u8, *world_id), (item_id, 1u8, *world_id)])
        .collect();
    let scans = ultros_clickhouse::queries::deep_scan_batch(&ch, 30, &requests)
        .await
        .map_err(|e| {
            tracing::warn!(error = ?e, item_id, world_name, "item_stats CH query failed");
            anyhow::anyhow!("ClickHouse item_stats query failed: {e}")
        })?;

    let variants = ultros_clickhouse::queries::aggregate_item_stats_variants(&scans);

    let mut response = Json(ItemStatsResponse {
        world_id: AnySelector::from(&scope).as_world_id(),
        item_id,
        variants,
    })
    .into_response();
    // Rollup refreshes every ~5 min worst case (1d window); a 60s browser
    // cache is comfortable and matches the cadence the user sees.
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(60)));
    Ok(response)
}
