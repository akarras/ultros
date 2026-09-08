//! `GET /api/v1/listing_stats/{worldDcOrRegion}` — the alive listing set.
//!
//! For every `(item_id, hq)` with at least one listing on the board: how many
//! listings and units, how many retainers, how long they have sat there, and
//! the floor among them — replayed from ClickHouse `listing_events` by the
//! `listing_alive` rollup and aggregated across all worlds in the selector's
//! scope (world, datacenter, or region — same name resolution as
//! `/api/v1/cheapest/{world}`). Part I1 of #1342.
//!
//! Same cache contract as `sale_stats`: in-process, single-flight, stale
//! fallback, shared-cacheable for 5 minutes. One deliberate difference: an
//! empty result is a `200` with an empty `stats`, not a `503`. Sale stats
//! treats "no rows" as "not yet rolled up" and asks the client to fail over;
//! here the rollup writes zero rows for emptied boards and there is no
//! failover, so "nothing alive" is a real answer worth caching.
//!
//! `?window=` is accepted and ignored — no `Query` extractor, so axum drops
//! it unread — which lets I2 add the windowed metrics without changing the
//! URL shape.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, State},
    response::IntoResponse,
};
use ultros_api_types::listing_stats::{BulkListingStats, ItemListingStats};
use ultros_clickhouse::{ClickHouseClient, queries::BulkListingAliveRow};
use ultros_db::world_data::world_cache::{AnySelector, WorldCache};

use crate::web::{
    error::{ClickHouseQueryError, WebError},
    stats_cache::{CacheKey, ListingStatsCache, cached_response},
};

/// I1 has no window; every scope shares one key until I2 keys on the real one.
const NO_WINDOW: u16 = 0;

pub(crate) async fn get_listing_stats(
    State(ch): State<ClickHouseClient>,
    State(world_cache): State<Arc<WorldCache>>,
    State(cache): State<ListingStatsCache>,
    Path(world): Path<String>,
) -> Result<impl IntoResponse, WebError> {
    let value = world_cache.lookup_value_by_name(&world)?;
    let selector = AnySelector::from(&value);
    let world_ids = world_cache
        .get_all_worlds_in(&value)
        .ok_or(WebError::NotFound)?;

    let cached = cache
        .get_or_load(
            CacheKey {
                selector,
                window_days: NO_WINDOW,
            },
            move || async move { load_listing_stats(&ch, world_ids).await },
        )
        .await?;
    let disposition = cached.disposition.as_str();
    metrics::counter!(
        "ultros_listing_stats_cache_total",
        "disposition" => disposition
    )
    .increment(1);
    Ok(cached_response(cached.body, disposition))
}

async fn load_listing_stats(ch: &ClickHouseClient, world_ids: Vec<i32>) -> Result<Bytes, WebError> {
    let rows = ultros_clickhouse::queries::bulk_listing_alive(ch, &world_ids)
        .await
        .map_err(|e| ClickHouseQueryError::new("bulk_listing_alive", e))?;
    let stats = rows.into_iter().map(to_wire).collect();
    serde_json::to_vec(&BulkListingStats { stats })
        .map(Bytes::from)
        .map_err(anyhow::Error::from)
        .map_err(Into::into)
}

/// ClickHouse row to wire row. Counts and ages already have the wire's
/// unsigned types; the floor is `UInt32` in ClickHouse but `i32` on the wire
/// like every other gil figure, so it saturates instead of wrapping.
fn to_wire(row: BulkListingAliveRow) -> ItemListingStats {
    ItemListingStats {
        item_id: row.item_id,
        hq: row.hq != 0,
        alive_count: row.alive_count,
        alive_units: row.alive_units,
        distinct_retainers: row.distinct_retainers,
        oldest_reviewed_unix: row.oldest_reviewed_unix,
        median_age_secs: row.median_age_secs,
        floor_alive: i32::try_from(row.floor_alive).unwrap_or(i32::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(hq: u8, floor_alive: u32) -> BulkListingAliveRow {
        BulkListingAliveRow {
            item_id: 7,
            hq,
            alive_count: 3,
            alive_units: 12,
            distinct_retainers: 2,
            oldest_reviewed_unix: 1_700_000_000,
            median_age_secs: 86_400,
            floor_alive,
        }
    }

    #[test]
    fn wire_row_maps_fields_directly() {
        let wire = to_wire(row(1, 950));
        assert_eq!(
            wire,
            ItemListingStats {
                item_id: 7,
                hq: true,
                alive_count: 3,
                alive_units: 12,
                distinct_retainers: 2,
                oldest_reviewed_unix: 1_700_000_000,
                median_age_secs: 86_400,
                floor_alive: 950,
            }
        );
    }

    #[test]
    fn hq_is_any_nonzero_byte() {
        assert!(!to_wire(row(0, 1)).hq);
        assert!(to_wire(row(2, 1)).hq);
    }

    #[test]
    fn floor_saturates_at_i32_max() {
        assert_eq!(to_wire(row(0, i32::MAX as u32)).floor_alive, i32::MAX);
        assert_eq!(to_wire(row(0, u32::MAX)).floor_alive, i32::MAX);
    }
}
