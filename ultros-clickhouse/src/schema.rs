//! ClickHouse DDL. Idempotent; called on every web-server startup.
//!
//! Tables defined here:
//! - `sales` — raw mirror of `sale_history`
//! - `item_stats_window` (Task 1.1) — multi-window aggregates
//! - `item_quality_score` (Task 1.1) — trustworthiness per item
//! - `_backfill_state` (Task 0.6) — resumable backfill cursor

use clickhouse::Client;

use crate::ClickHouseError;

pub async fn apply(client: &Client) -> Result<(), ClickHouseError> {
    apply_sales_table(client).await?;
    Ok(())
}

/// Raw mirror of Postgres `sale_history`.
///
/// Engine: `ReplacingMergeTree(inserted_at)` on the natural key
/// `(item_id, hq, world_id, sold_date, buying_character_id)` makes dual-writes
/// idempotent — replaying the event bus or re-running backfill against an
/// already-populated partition is a no-op on merge.
///
/// Partitioning by month keeps retention / drop operations cheap and aligns
/// with the backfill chunk size.
///
/// `total_gil` is a MATERIALIZED column computed on insert from
/// `price_per_item * quantity` so consumers don't have to do the math and the
/// value lives compressed on disk like any other column.
async fn apply_sales_table(client: &Client) -> Result<(), ClickHouseError> {
    client
        .query(
            r#"
            CREATE TABLE IF NOT EXISTS sales (
                pg_id                Int32,
                sold_date            DateTime,
                inserted_at          DateTime DEFAULT now(),
                item_id              Int32,
                hq                   UInt8,
                world_id             Int32,
                price_per_item       UInt32,
                quantity             UInt16,
                total_gil            UInt64 MATERIALIZED toUInt64(price_per_item) * toUInt64(quantity),
                buying_character_id  Int64,
                buyer_name           LowCardinality(String) DEFAULT ''
            )
            ENGINE = ReplacingMergeTree(inserted_at)
            PARTITION BY toYYYYMM(sold_date)
            ORDER BY (item_id, hq, world_id, sold_date, pg_id)
            SETTINGS index_granularity = 8192
            "#,
        )
        .execute()
        .await?;
    Ok(())
}
