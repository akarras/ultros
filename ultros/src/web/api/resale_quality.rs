//! `POST /api/v1/resale_quality/{world}` — batch deep-scan enrichment
//! for the Flip Finder.
//!
//! Mirrors the analyzer's internal `deep_scan_batch` call but exposes a
//! shape friendlier for client-side join. The FE sends the top-N rows
//! from its Pass-1 profit table (already ranked client-side), and we
//! return per-row confidence band, 30d VWAP, sample size, and laundering
//! suspicion. The Flip Finder uses these to render the Quality and VWAP
//! columns, and to drive the show-suspicious toggle.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use ultros_api_types::{
    resale_quality::{ResaleQualityRequest, ResaleQualityResponse, ResaleQualityRow},
    world_helper::{AnySelector, WorldHelper},
};
use ultros_clickhouse::ClickHouseClient;

use crate::web::error::WebError;

/// Hard cap on the batch size. The analyzer page renders ~200 rows at a
/// time after Pass-1 filtering, so 250 leaves comfortable headroom.
const MAX_ITEMS: usize = 250;

pub(crate) async fn post_resale_quality(
    State(ch): State<ClickHouseClient>,
    State(world_helper): State<Arc<WorldHelper>>,
    Path(world_name): Path<String>,
    Json(req): Json<ResaleQualityRequest>,
) -> Result<impl IntoResponse, WebError> {
    let world = world_helper
        .lookup_world_by_name(&world_name)
        .ok_or(WebError::NotFound)?;
    let world_id = match AnySelector::from(&world) {
        AnySelector::World(id) => id,
        _ => return Err(WebError::BadRequest),
    };

    let window_days = match req.window_days.unwrap_or(30) {
        w @ (7 | 30 | 90) => w,
        _ => 30,
    };

    if req.items.is_empty() {
        return Ok(Json(ResaleQualityResponse {
            world_id,
            window_days,
            rows: vec![],
        })
        .into_response());
    }
    if req.items.len() > MAX_ITEMS {
        return Err(WebError::BadRequest);
    }

    let scan_req: Vec<(i32, u8, i32)> = req
        .items
        .iter()
        .map(|(item_id, hq)| (*item_id, *hq as u8, world_id))
        .collect();

    let rows = ultros_clickhouse::queries::deep_scan_batch(&ch, window_days, &scan_req)
        .await
        .map_err(|e| {
            tracing::warn!(error = ?e, world_id, "resale_quality deep_scan_batch failed");
            anyhow::anyhow!("ClickHouse deep_scan failed: {e}")
        })?;

    let window_f32 = (window_days as f32).max(1.0);
    let rows: Vec<ResaleQualityRow> = rows
        .into_iter()
        .map(|d| ResaleQualityRow {
            item_id: d.item_id,
            hq: d.hq != 0,
            world_id: d.world_id,
            window_days,
            vwap: d.vwap as i32,
            sample_size: d.sample_size,
            // Use cleaned_sample_size for sales/day — the noise-filtered
            // count is the honest number for "did this actually sell".
            sales_per_day: d.cleaned_sample_size as f32 / window_f32,
            confidence_band: d.confidence_band(),
            launder_suspicion: d.launder_suspicion_pct,
        })
        .collect();

    let mut response = Json(ResaleQualityResponse {
        world_id,
        window_days,
        rows,
    })
    .into_response();
    // Rollup refreshes hourly for 30d/90d windows, every 15min for 7d —
    // 60s browser cache stays well ahead of staleness while shielding CH
    // from per-keystroke filter chatter on the FE.
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(60)));
    Ok(response)
}
