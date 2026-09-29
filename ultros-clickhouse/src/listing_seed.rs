//! One-time seed of `listing_events` from the current Postgres board.
//!
//! The table starts empty at deploy, so a listing already on the board that
//! never changes would emit no row and a floor replayed from events would read
//! too high until it was finally removed. Streaming every `active_listing`
//! row once as `kind = 'added', source = 'snapshot'` makes the alive set
//! complete from day one. Guarded by a marker table so it runs exactly once
//! per ClickHouse database.

use chrono::Utc;
use futures::TryStreamExt;
use tracing::{info, warn};
use ultros_db::UltrosDb;

use crate::{
    ClickHouseClient, ClickHouseError, rows::ListingEventRow,
    schema::LISTING_EVENTS_SEED_MARKER_TABLE, writer::insert_all,
};

/// Rows per INSERT while streaming the board.
pub const SEED_CHUNK: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedOutcome {
    AlreadySeeded,
    Seeded { rows: u64 },
}

/// Streams `active_listing` into `listing_events` unless the marker says it
/// already happened. Safe to call repeatedly; safe to retry after a failure
/// (a partial run's rows are discarded before streaming again).
pub async fn seed_listing_events(
    ch: &ClickHouseClient,
    pg: &UltrosDb,
) -> Result<SeedOutcome, ClickHouseError> {
    if already_seeded(ch).await? {
        return Ok(SeedOutcome::AlreadySeeded);
    }
    // A previous attempt may have died mid-stream. Only the seed writes
    // `source = 'snapshot'`, and the marker exists only after a full success,
    // so anything tagged snapshot at this point is a torn run.
    ch.client()
        .query(
            "ALTER TABLE listing_events DELETE WHERE source = 'snapshot' SETTINGS mutations_sync = 1",
        )
        .execute()
        .await?;

    let observed_at = Utc::now();
    let mut stream = pg
        .stream_active_listings()
        .await
        .map_err(|e| ClickHouseError::Backfill(e.to_string()))?;
    let mut buf: Vec<ListingEventRow> = Vec::with_capacity(SEED_CHUNK);
    let mut rows = 0u64;
    while let Some(model) = stream
        .try_next()
        .await
        .map_err(|e| ClickHouseError::Backfill(e.to_string()))?
    {
        buf.push(ListingEventRow::from_snapshot(&model, observed_at));
        if buf.len() == SEED_CHUNK {
            rows += insert_all(ch, &buf, SEED_CHUNK).await?;
            buf.clear();
        }
    }
    if !buf.is_empty() {
        rows += insert_all(ch, &buf, SEED_CHUNK).await?;
    }
    mark_seeded(ch, rows).await?;
    info!(rows, "seeded listing_events from active_listing");
    Ok(SeedOutcome::Seeded { rows })
}

pub async fn already_seeded(ch: &ClickHouseClient) -> Result<bool, ClickHouseError> {
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Found {
        n: u8,
    }
    let found: Found = ch
        .client()
        .query(&format!(
            "SELECT count() > 0 AS n FROM {LISTING_EVENTS_SEED_MARKER_TABLE}"
        ))
        .fetch_one()
        .await?;
    Ok(found.n != 0)
}

pub async fn mark_seeded(ch: &ClickHouseClient, rows: u64) -> Result<(), ClickHouseError> {
    #[derive(serde::Serialize, clickhouse::Row)]
    struct MarkerRow {
        #[serde(with = "clickhouse::serde::chrono::datetime")]
        seeded_at: chrono::DateTime<Utc>,
        rows_streamed: u64,
    }
    let mut insert = ch
        .client()
        .insert::<MarkerRow>(LISTING_EVENTS_SEED_MARKER_TABLE)
        .await?;
    insert
        .write(&MarkerRow {
            seeded_at: Utc::now(),
            rows_streamed: rows,
        })
        .await?;
    insert.end().await?;
    Ok(())
}

/// Leader-side driver: keeps trying until the seed exists or the token fires.
/// Failures are spaced ten minutes apart so a ClickHouse outage at boot does
/// not turn into a hot loop.
///
/// The token is the rollup *lease*: losing it aborts an in-flight stream
/// rather than letting it run on. Otherwise the next leader's attempt would
/// delete the torn `snapshot` rows while this one was still inserting, and
/// the two would leave the table with a partial duplicate.
pub async fn run_until_seeded(
    ch: ClickHouseClient,
    pg: UltrosDb,
    token: tokio_util::sync::CancellationToken,
) {
    loop {
        let outcome = tokio::select! {
            _ = token.cancelled() => return,
            outcome = seed_listing_events(&ch, &pg) => outcome,
        };
        match outcome {
            Ok(SeedOutcome::AlreadySeeded) | Ok(SeedOutcome::Seeded { .. }) => return,
            Err(error) => {
                metrics::counter!("ultros_listing_events_seed_failures_total").increment(1);
                warn!(?error, "listing_events seed failed; retrying in 10 minutes");
            }
        }
        tokio::select! {
            _ = token.cancelled() => return,
            _ = tokio::time::sleep(std::time::Duration::from_secs(600)) => {}
        }
    }
}
