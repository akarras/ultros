//! `/api/v1/item_stats/{world}/{item_id}` — per-item analyzer stats for
//! the item view's confidence chip.
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
    item_stats::{ItemStatsResponse, ItemStatsVariant},
    world_helper::{AnySelector, WorldHelper},
};
use ultros_clickhouse::ClickHouseClient;

use crate::web::error::WebError;

pub(crate) async fn get_item_stats(
    State(ch): State<ClickHouseClient>,
    State(world_helper): State<Arc<WorldHelper>>,
    Path((world_name, item_id)): Path<(String, i32)>,
) -> Result<impl IntoResponse, WebError> {
    let world = world_helper
        .lookup_world_by_name(&world_name)
        .ok_or(WebError::NotFound)?;
    let world_id = match AnySelector::from(&world) {
        AnySelector::World(id) => id,
        // Item stats are per-world; reject DC/Region selectors to keep the
        // semantics tight. The chart they're paired with is also per-world.
        _ => return Err(WebError::BadRequest),
    };

    // Ask CH for both quality variants in one round trip. Missing variants
    // (one of NQ/HQ doesn't exist) yield zero rows, not an error.
    let scans = ultros_clickhouse::queries::deep_scan_batch(
        &ch,
        30,
        &[(item_id, 0u8, world_id), (item_id, 1u8, world_id)],
    )
    .await
    .map_err(|e| {
        tracing::warn!(error = ?e, item_id, world_id, "item_stats CH query failed");
        anyhow::anyhow!("ClickHouse item_stats query failed: {e}")
    })?;

    let variants: Vec<ItemStatsVariant> = scans
        .into_iter()
        .map(|s| ItemStatsVariant {
            hq: s.hq != 0,
            sample_size_30d: s.sample_size,
            cleaned_sample_size_30d: s.cleaned_sample_size,
            vwap_30d: s.vwap,
            p50_30d: s.p50,
            confidence_band: s.confidence_band(),
            launder_suspicion: s.launder_suspicion_pct,
        })
        .collect();

    let mut response = Json(ItemStatsResponse {
        world_id,
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
