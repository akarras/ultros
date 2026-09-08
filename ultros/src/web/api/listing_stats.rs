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
//! That difference is why every response carries the rollup's own timestamp,
//! as `x-ultros-listing-stats-computed-at` and as `computed_at_unix` on the
//! body: an empty market and a rollup that has been failing all day are
//! otherwise the same `200 {"stats":[]}`, which this cache would then serve for
//! five minutes at a time with `stale-while-revalidate` behind it.
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

/// When the rollup behind this response last ran, for a monitor that cannot
/// see the process's metrics.
const COMPUTED_AT_HEADER: &str = "x-ultros-listing-stats-computed-at";

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
    let computed_at = computed_at_from_body(&cached.body);
    let mut response = cached_response(cached.body, disposition);
    response.headers_mut().insert(
        axum::http::header::HeaderName::from_static(COMPUTED_AT_HEADER),
        axum::http::HeaderValue::from(computed_at),
    );
    Ok(response)
}

async fn load_listing_stats(ch: &ClickHouseClient, world_ids: Vec<i32>) -> Result<Bytes, WebError> {
    let computed_at_unix = ultros_clickhouse::queries::listing_alive_computed_at(ch, &world_ids)
        .await
        .map_err(|e| ClickHouseQueryError::new("listing_alive_computed_at", e))?;
    let rows = ultros_clickhouse::queries::bulk_listing_alive(ch, &world_ids)
        .await
        .map_err(|e| ClickHouseQueryError::new("bulk_listing_alive", e))?;
    let stats = rows.into_iter().map(to_wire).collect();
    serde_json::to_vec(&BulkListingStats {
        computed_at_unix,
        stats,
    })
    .map(Bytes::from)
    .map_err(anyhow::Error::from)
    .map_err(Into::into)
}

/// Read `computed_at_unix` back off a serialized body.
///
/// The cache stores bodies, not the values they were built from, so the header
/// has to come from the bytes on a hit as well as on a load. `BulkListingStats`
/// declares the field first (with a test in `ultros-api-types` pinning that),
/// which makes this a fixed-size prefix scan rather than a serde parse of a
/// multi-megabyte whole-market payload on every cache hit. A body that does not
/// start with it reports `0` — the same "never computed" value the query
/// itself returns — rather than lying about freshness.
fn computed_at_from_body(body: &[u8]) -> i64 {
    const NEEDLE: &[u8] = br#""computed_at_unix":"#;
    let head = &body[..body.len().min(64)];
    let Some(start) = head
        .windows(NEEDLE.len())
        .position(|window| window == NEEDLE)
    else {
        return 0;
    };
    let digits = &body[start + NEEDLE.len()..];
    let end = digits
        .iter()
        .position(|b| !b.is_ascii_digit() && *b != b'-')
        .unwrap_or(digits.len());
    std::str::from_utf8(&digits[..end])
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
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

    #[test]
    fn computed_at_is_read_off_a_real_body() {
        let body = serde_json::to_vec(&BulkListingStats {
            computed_at_unix: 1_757_000_000,
            stats: vec![to_wire(row(0, 950))],
        })
        .unwrap();
        assert_eq!(computed_at_from_body(&body), 1_757_000_000);
    }

    #[test]
    fn computed_at_reads_zero_from_an_empty_rollup() {
        let body = serde_json::to_vec(&BulkListingStats::default()).unwrap();
        assert_eq!(computed_at_from_body(&body), 0);
    }

    /// Never guess a freshness the body does not carry — a short, truncated or
    /// unrecognised body reports "never computed" rather than a stale number.
    #[test]
    fn computed_at_of_an_unrecognised_body_is_zero() {
        assert_eq!(computed_at_from_body(b""), 0);
        assert_eq!(computed_at_from_body(b"{}"), 0);
        assert_eq!(computed_at_from_body(br#"{"stats":[]}"#), 0);
        assert_eq!(computed_at_from_body(br#"{"computed_at_unix":"#), 0);
        // Far enough in that the prefix scan will not reach it.
        let buried = format!(r#"{{"padding":"{}","computed_at_unix":5}}"#, "x".repeat(80));
        assert_eq!(computed_at_from_body(buried.as_bytes()), 0);
    }
}
