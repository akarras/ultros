//! One-shot backfill from Postgres `sale_history` to ClickHouse `sales`.
//!
//! Chunks by `(world_id, year-month)` for resumability — re-running this
//! after a partial completion skips sealed past months. The active month is
//! always replayed; explicit replay can repair any previously completed chunk.
//!
//! Idempotent: the `sales` table is a `ReplacingMergeTree` keyed on
//! `(item_id, hq, world_id, sold_date, pg_id)`, so re-streaming the same rows
//! just merges into no-ops. Dual-writes overlapping the backfill window are
//! safe *because* both paths key on the same Postgres `sale_history.id` — see
//! [`SaleRow::from_db_model`] (this path) and [`SaleRow::from_api_sale`] (the
//! live path). If either ever writes a placeholder id, the two paths stop
//! agreeing on row identity and the overlap double-counts instead of merging.

use std::time::Instant;

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use futures::TryStreamExt;
use tracing::info;
use ultros_db::UltrosDb;

use crate::{ClickHouseClient, ClickHouseError, rows::SaleRow};

/// A bounded month range, including both endpoints. Resume skips sealed past
/// months; replay ignores their markers to repair dropped or late-arriving sales.
#[derive(Debug, Clone)]
pub struct BackfillOptions {
    pub from: NaiveDate,
    pub through: NaiveDate,
    /// Empty selects every known world. Unknown explicit IDs are rejected.
    pub worlds: Vec<i32>,
    pub replay: bool,
}

impl BackfillOptions {
    pub fn since(start_year: i32) -> Result<Self, ClickHouseError> {
        Ok(Self {
            from: NaiveDate::from_ymd_opt(start_year, 1, 1)
                .ok_or_else(|| ClickHouseError::Backfill("invalid start year".into()))?,
            through: chrono::Utc::now().date_naive().with_day(1).unwrap(),
            worlds: vec![],
            replay: false,
        })
    }

    fn validate(&self, current_month: NaiveDate) -> Result<(), ClickHouseError> {
        if self.from.day() != 1
            || self.through.day() != 1
            || self.from > self.through
            || self.from.year() < 1970
            || self.through > current_month
        {
            return Err(ClickHouseError::Backfill(
                "expected an ordered month range from 1970 through the current month".into(),
            ));
        }
        Ok(())
    }
}

/// Resume historical backfill, always revisiting the active month. Existing
/// markers for that month (written by older versions) are deliberately ignored.
pub async fn backfill_sales(
    pg: &UltrosDb,
    ch: &ClickHouseClient,
    start_year: i32,
) -> Result<BackfillStats, ClickHouseError> {
    backfill_sales_with_options(pg, ch, &BackfillOptions::since(start_year)?).await
}

pub async fn backfill_sales_with_options(
    pg: &UltrosDb,
    ch: &ClickHouseClient,
    options: &BackfillOptions,
) -> Result<BackfillStats, ClickHouseError> {
    let current_month = chrono::Utc::now().date_naive().with_day(1).unwrap();
    options.validate(current_month)?;
    let worlds = pg
        .list_worlds()
        .await
        .map_err(|e| ClickHouseError::Backfill(e.to_string()))?;
    for id in &options.worlds {
        if !worlds.iter().any(|world| world.id == *id) {
            return Err(ClickHouseError::Backfill(format!("unknown world {id}")));
        }
    }
    ensure_state_table(ch).await?;
    let mut stats = BackfillStats::default();
    info!(?options, "starting ClickHouse backfill");
    for world in worlds
        .iter()
        .filter(|world| options.worlds.is_empty() || options.worlds.contains(&world.id))
    {
        let mut month = options.from;
        while month <= options.through {
            let ym = month.year() as u32 * 100 + month.month();
            let sealed = month < current_month;
            if !options.replay && sealed && chunk_already_done(ch, world.id, ym).await? {
                stats.chunks_skipped += 1;
            } else {
                let n = stream_chunk(pg, ch, world.id, month.year(), month.month()).await?;
                // An active month can receive more rows immediately after this
                // stream ends. Never claim it is complete.
                if sealed {
                    mark_chunk_done(ch, world.id, ym, n).await?;
                }
                stats.chunks_streamed += 1;
                stats.rows_streamed += n;
            }
            month = month
                .checked_add_months(chrono::Months::new(1))
                .ok_or_else(|| ClickHouseError::Backfill("month range overflow".into()))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_or_future_ranges() {
        let current = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let mut options = BackfillOptions::since(2026).unwrap();
        options.through = current;
        assert!(options.validate(current).is_ok());
        for (from, through) in [
            (current, current.pred_opt().unwrap()),
            (current.with_day(2).unwrap(), current),
            (current, current.succ_opt().unwrap()),
            (current, NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()),
        ] {
            options.from = from;
            options.through = through;
            assert!(options.validate(current).is_err());
        }
    }

    /// Isolated schema in an explicitly opted-in disposable Postgres database,
    /// plus an explicitly named ClickHouse audit database. Never loads .env.
    #[tokio::test]
    #[ignore = "requires disposable PostgreSQL and ClickHouse; see ingest-observability.md"]
    async fn replay_repairs_sealed_months_and_resume_revisits_active_months() {
        use sea_orm::{ConnectOptions, ConnectionTrait, Database};
        let url = std::env::var("ULTROS_PLUGIN_TEST_DATABASE_URL").unwrap();
        let suffix = url
            .strip_prefix("postgres://ultros_plugin@127.0.0.1:")
            .unwrap();
        let (port, database) = suffix.split_once('/').unwrap();
        assert!(port.parse::<u16>().is_ok_and(|p| p > 0));
        let name = database.strip_prefix("ultros_plugin_").unwrap();
        assert!(!name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'));
        assert!(
            std::env::var("CLICKHOUSE_URL")
                .unwrap()
                .starts_with("http://127.0.0.1:")
        );
        assert_eq!(
            std::env::var("CLICKHOUSE_DATABASE").unwrap(),
            "ultros_audit"
        );
        let mut connection_options = ConnectOptions::new(url);
        connection_options
            .max_connections(2)
            .min_connections(1)
            .set_schema_search_path("audit_ch_backfill")
            .sqlx_logging(false);
        let connection = Database::connect(connection_options).await.unwrap();
        connection.execute_unprepared(
            "DROP SCHEMA IF EXISTS audit_ch_backfill CASCADE;
             CREATE SCHEMA audit_ch_backfill;
             CREATE TABLE audit_ch_backfill.world(id integer PRIMARY KEY, name text NOT NULL, datacenter_id integer NOT NULL);
             INSERT INTO audit_ch_backfill.world VALUES(30002,'RepairWorld',1),(30003,'OtherWorld',1);
             CREATE TABLE audit_ch_backfill.sale_history(id integer PRIMARY KEY,quantity integer NOT NULL,price_per_item integer NOT NULL,buying_character_id integer NOT NULL,hq boolean NOT NULL,sold_item_id integer NOT NULL,sold_date timestamp NOT NULL,world_id integer NOT NULL);
             INSERT INTO audit_ch_backfill.sale_history VALUES(-8001,2,100,1,false,9004,'2020-01-02',30002),(-8002,9,100,1,false,9004,'2020-01-02',30003);"
        ).await.unwrap();
        let pg = UltrosDb::from_connection(connection);
        let ch = ClickHouseClient::from_env();
        ch.migrate().await.unwrap();
        ch.client().query("ALTER TABLE sales DELETE WHERE item_id = 9004 AND world_id IN (30002,30003) SETTINGS mutations_sync = 1").execute().await.unwrap();
        ensure_state_table(&ch).await.unwrap();
        ch.client().query("ALTER TABLE _backfill_state DELETE WHERE world_id IN (30002,30003) SETTINGS mutations_sync = 1").execute().await.unwrap();
        let january = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        let mut options = BackfillOptions {
            from: january,
            through: january,
            worlds: vec![30002],
            replay: false,
        };
        let first = backfill_sales_with_options(&pg, &ch, &options)
            .await
            .unwrap();
        assert_eq!((first.chunks_streamed, first.rows_streamed), (1, 1));
        pg.get_connection()
            .execute_unprepared(
                "INSERT INTO sale_history VALUES(-8003,3,100,1,false,9004,'2020-01-03',30002)",
            )
            .await
            .unwrap();
        let skipped = backfill_sales_with_options(&pg, &ch, &options)
            .await
            .unwrap();
        assert_eq!(skipped.chunks_skipped, 1);
        options.replay = true;
        for _ in 0..2 {
            let repaired = backfill_sales_with_options(&pg, &ch, &options)
                .await
                .unwrap();
            assert_eq!(repaired.rows_streamed, 2);
        }
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct Totals {
            n: u64,
            qty: u64,
        }
        let totals: Totals = ch.client().query("SELECT count() AS n, sum(quantity) AS qty FROM sales FINAL WHERE item_id=9004 AND world_id IN (30002,30003)").fetch_one().await.unwrap();
        assert_eq!(
            (totals.n, totals.qty),
            (2, 5),
            "replay must deduplicate and respect world scope"
        );

        let current = chrono::Utc::now().date_naive().with_day(1).unwrap();
        options.from = current;
        options.through = current;
        options.replay = false;
        // Simulate the legacy bug: an active month was marked complete.
        mark_chunk_done(&ch, 30002, current.year() as u32 * 100 + current.month(), 0)
            .await
            .unwrap();
        pg.get_connection()
            .execute_unprepared(&format!(
                "INSERT INTO sale_history VALUES(-8004,1,100,1,false,9004,'{current}',30002)"
            ))
            .await
            .unwrap();
        assert_eq!(
            backfill_sales_with_options(&pg, &ch, &options)
                .await
                .unwrap()
                .rows_streamed,
            1
        );
        pg.get_connection()
            .execute_unprepared(&format!(
                "INSERT INTO sale_history VALUES(-8005,1,100,1,false,9004,'{current}',30002)"
            ))
            .await
            .unwrap();
        assert_eq!(
            backfill_sales_with_options(&pg, &ch, &options)
                .await
                .unwrap()
                .rows_streamed,
            2
        );
        options.worlds.push(123456789);
        assert!(
            backfill_sales_with_options(&pg, &ch, &options)
                .await
                .is_err()
        );
        pg.get_connection()
            .execute_unprepared("DROP SCHEMA audit_ch_backfill CASCADE")
            .await
            .unwrap();
    }
}
