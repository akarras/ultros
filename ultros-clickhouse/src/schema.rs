//! ClickHouse DDL. Idempotent; called on every web-server startup.
//!
//! Tables defined here:
//! - `sales` (Task 0.3) — raw mirror of `sale_history`
//! - `item_stats_window` (Task 1.1) — multi-window aggregates
//! - `item_quality_score` (Task 1.1) — trustworthiness per item
//! - `_backfill_state` (Task 0.6) — resumable backfill cursor

use clickhouse::Client;

use crate::ClickHouseError;

pub async fn apply(_client: &Client) -> Result<(), ClickHouseError> {
    // Filled in by Task 0.3
    Ok(())
}
