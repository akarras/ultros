//! One-shot backfill from Postgres `sale_history` to ClickHouse `sales`.
//!
//! Chunks by `(world_id, year-month)` for resumability — re-running this
//! after a partial completion is cheap because completed chunks are tracked
//! in the `_backfill_state` table and skipped.
//!
//! Idempotent: the `sales` table is a `ReplacingMergeTree` keyed on
//! `(item_id, hq, world_id, sold_date, buying_character_id)`, so re-streaming
//! the same rows just merges into no-ops. That means dual-writes overlapping
//! the backfill window are safe too.

use std::time::Instant;

use chrono::{NaiveDate, NaiveDateTime};
use futures::TryStreamExt;
use tracing::{info, warn};
use ultros_db::UltrosDb;

use crate::{ClickHouseClient, ClickHouseError, rows::SaleRow};

/// Run a chunked backfill from `start_year` onward, finishing at the current
/// month. Resumable: completed chunks are skipped on re-runs.
pub async fn backfill_sales(
    pg: &UltrosDb,
    ch: &ClickHouseClient,
    start_year: i32,
) -> Result<BackfillStats, ClickHouseError> {
    ensure_state_table(ch).await?;

    let worlds = pg
        .list_worlds()
        .await
        .map_err(|e| ClickHouseError::Backfill(e.to_string()))?;
    let now = chrono::Utc::now().naive_utc();
    let mut stats = BackfillStats::default();

    info!(
        worlds = worlds.len(),
        start_year, "starting ClickHouse backfill"
    );

    for world in &worlds {
        let mut y = start_year;
        let mut m: u32 = 1;
        loop {
            let chunk_start =
                match NaiveDate::from_ymd_opt(y, m, 1).and_then(|d| d.and_hms_opt(0, 0, 0)) {
                    Some(t) => t,
                    None => {
                        warn!(year = y, month = m, "skipping invalid date");
                        break;
                    }
                };
            if chunk_start > now {
                break;
            }
            let ym = (y as u32) * 100 + m;
            if chunk_already_done(ch, world.id, ym).await? {
                stats.chunks_skipped += 1;
            } else {
                let n = stream_chunk(pg, ch, world.id, y, m).await?;
                mark_chunk_done(ch, world.id, ym, n).await?;
                stats.chunks_streamed += 1;
                stats.rows_streamed += n;
            }
            m += 1;
            if m > 12 {
                m = 1;
                y += 1;
            }
        }
    }
    info!(?stats, "ClickHouse backfill complete");
    Ok(stats)
}

async fn ensure_state_table(ch: &ClickHouseClient) -> Result<(), ClickHouseError> {
    ch.client()
        .query(
            r#"
            CREATE TABLE IF NOT EXISTS _backfill_state (
                world_id     Int32,
                year_month   UInt32,
                completed_at DateTime,
                rows_streamed UInt64
            )
            ENGINE = ReplacingMergeTree(completed_at)
            ORDER BY (world_id, year_month)
            "#,
        )
        .execute()
        .await?;
    Ok(())
}

async fn chunk_already_done(
    ch: &ClickHouseClient,
    world_id: i32,
    ym: u32,
) -> Result<bool, ClickHouseError> {
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Found {
        n: u8,
    }
    let found: Found = ch
        .client()
        .query(
            "SELECT count() > 0 AS n FROM _backfill_state \
             WHERE world_id = ? AND year_month = ?",
        )
        .bind(world_id)
        .bind(ym)
        .fetch_one()
        .await?;
    Ok(found.n != 0)
}

async fn mark_chunk_done(
    ch: &ClickHouseClient,
    world_id: i32,
    ym: u32,
    rows: u64,
) -> Result<(), ClickHouseError> {
    #[derive(serde::Serialize, clickhouse::Row)]
    struct StateRow {
        world_id: i32,
        year_month: u32,
        #[serde(with = "clickhouse::serde::chrono::datetime")]
        completed_at: chrono::DateTime<chrono::Utc>,
        rows_streamed: u64,
    }
    let mut insert = ch.client().insert::<StateRow>("_backfill_state").await?;
    insert
        .write(&StateRow {
            world_id,
            year_month: ym,
            completed_at: chrono::Utc::now(),
            rows_streamed: rows,
        })
        .await?;
    insert.end().await?;
    Ok(())
}

async fn stream_chunk(
    pg: &UltrosDb,
    ch: &ClickHouseClient,
    world_id: i32,
    year: i32,
    month: u32,
) -> Result<u64, ClickHouseError> {
    let start = Instant::now();
    let first = month_start(year, month)
        .ok_or_else(|| ClickHouseError::Backfill(format!("bad start date {year}-{month}")))?;
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1u32)
    } else {
        (year, month + 1)
    };
    let last = month_start(next_year, next_month).ok_or_else(|| {
        ClickHouseError::Backfill(format!("bad end date {next_year}-{next_month}"))
    })?;

    let mut stream = pg
        .stream_sales_in_range(world_id, first, last)
        .await
        .map_err(|e| ClickHouseError::Backfill(e.to_string()))?;
    let mut insert = ch.client().insert::<SaleRow>("sales").await?;
    let mut n: u64 = 0;
    while let Some(row) = stream
        .try_next()
        .await
        .map_err(|e| ClickHouseError::Backfill(e.to_string()))?
    {
        insert
            .write(&SaleRow::from_db_model(&row, String::new()))
            .await?;
        n += 1;
    }
    insert.end().await?;
    info!(
        world_id,
        year,
        month,
        rows = n,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "backfill chunk done"
    );
    Ok(n)
}

fn month_start(year: i32, month: u32) -> Option<NaiveDateTime> {
    NaiveDate::from_ymd_opt(year, month, 1)?.and_hms_opt(0, 0, 0)
}

#[derive(Default, Debug)]
pub struct BackfillStats {
    pub chunks_streamed: u64,
    pub chunks_skipped: u64,
    pub rows_streamed: u64,
}
