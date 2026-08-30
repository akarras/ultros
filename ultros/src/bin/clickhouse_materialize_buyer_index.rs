//! One-shot binary that materializes the `idx_sales_buyer` skip index over the
//! parts of `sales` that predate it.
//!
//! Usage:
//!   cargo run --bin clickhouse_materialize_buyer_index
//!
//! Startup DDL (`ClickHouseClient::migrate`) adds the index, but a skip index
//! in ClickHouse only covers parts written after it exists — every part already
//! on disk carries no bloom filter and is read in full. That makes the
//! owned-character purchase history fast for recent data and a whole-table scan
//! for everything older, which is the opposite of what a "look back at what I
//! paid" feature needs.
//!
//! `MATERIALIZE INDEX` fixes that, but it is a mutation across the entire table
//! — far too expensive to fire from web-server startup, and it would re-fire on
//! every deploy. Hence a binary an operator runs once, deliberately, after
//! shipping the index.
//!
//! It returns as soon as ClickHouse accepts the mutation; the work continues in
//! the background. Watch it finish with:
//!
//! ```sql
//! SELECT is_done, parts_to_do, latest_fail_reason
//! FROM system.mutations
//! WHERE table = 'sales' AND command LIKE '%idx_sales_buyer%'
//! ```

use ultros_clickhouse::ClickHouseClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let ch = ClickHouseClient::from_env();
    // Ensures the index exists before we ask for it to be materialized, so
    // running this against a deployment that hasn't restarted yet still works.
    ch.migrate().await?;

    tracing::info!("submitting MATERIALIZE INDEX idx_sales_buyer on sales");
    ch.client()
        .query("ALTER TABLE sales MATERIALIZE INDEX idx_sales_buyer")
        .execute()
        .await?;
    tracing::info!(
        "mutation submitted; it runs in the background. Track it in system.mutations \
         (table = 'sales')"
    );
    Ok(())
}
