//! One-shot binary that backfills Postgres `sale_history` into ClickHouse.
//!
//! Usage:
//!   cargo run --bin clickhouse_backfill           # defaults to start_year=2022
//!   cargo run --bin clickhouse_backfill 2025      # start from 2025
//!
//! Resumable: tracks per-chunk progress in the `_backfill_state` table on
//! ClickHouse. Safe to re-run after a partial completion — completed chunks
//! are skipped.

use std::env;

use ultros_clickhouse::{ClickHouseClient, backfill::backfill_sales};
use ultros_db::UltrosDb;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let start_year: i32 = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2022);

    let pg = UltrosDb::connect().await?;
    let ch = ClickHouseClient::from_env();
    ch.migrate().await?;

    let stats = backfill_sales(&pg, &ch, start_year).await?;
    tracing::info!(?stats, "backfill complete");
    Ok(())
}
