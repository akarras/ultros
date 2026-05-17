//! Integration test that exercises the schema against a live ClickHouse.
//!
//! Reads `CLICKHOUSE_URL` (etc) from env so it runs against whatever the dev
//! `docker-compose.dev.yml` brought up. Skipped automatically when the env var
//! `ULTROS_CH_INTEGRATION` isn't set, so `cargo test` in CI without ClickHouse
//! is still green.
//!
//! Run locally with:
//!   ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test schema_smoke -- --nocapture

use chrono::{NaiveDate, Utc};
use ultros_clickhouse::{ClickHouseClient, rows::SaleRow};

fn integration_enabled() -> bool {
    std::env::var("ULTROS_CH_INTEGRATION").is_ok()
}

#[tokio::test]
async fn migrate_then_insert_then_read_round_trip() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }

    // Load .env from the workspace root so CLICKHOUSE_* vars are available
    // even when running `cargo test` directly (which doesn't auto-load .env).
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();

    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");

    // Use a sentinel item_id well outside the real game's range so re-running
    // the test is idempotent and never collides with real data.
    let item_id = -424242;
    let sold_date = chrono::DateTime::from_naive_utc_and_offset(
        NaiveDate::from_ymd_opt(2026, 5, 15)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        Utc,
    );
    let row = SaleRow {
        pg_id: -42,
        sold_date,
        item_id,
        hq: 1,
        world_id: 40,
        price_per_item: 12345,
        quantity: 7,
        buying_character_id: 99,
        buyer_name: "smoke-test".to_string(),
    };

    // Clean up any prior run. `mutations_sync = 1` blocks until the
    // mutation finishes — without it, ALTER ... DELETE returns immediately
    // and our subsequent count assertions race against leftover rows.
    ch.client()
        .query("ALTER TABLE sales DELETE WHERE item_id = ? SETTINGS mutations_sync = 1")
        .bind(item_id)
        .execute()
        .await
        .expect("cleanup");

    // Insert.
    let mut insert = ch
        .client()
        .insert::<SaleRow>("sales")
        .await
        .expect("insert");
    insert.write(&row).await.expect("write");
    insert.end().await.expect("end");

    // Read back. The client requires the full row type to deserialize tuples.
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Aggregates {
        count: u64,
        sum_qty: u64,
        sum_gil: u64,
    }
    let agg: Aggregates = ch
        .client()
        .query(
            "SELECT count() AS count, sum(quantity) AS sum_qty, sum(total_gil) AS sum_gil \
             FROM sales WHERE item_id = ?",
        )
        .bind(item_id)
        .fetch_one()
        .await
        .expect("fetch");
    assert_eq!(agg.count, 1);
    assert_eq!(agg.sum_qty, 7);
    // total_gil is a MATERIALIZED column = price_per_item * quantity = 12345 * 7.
    assert_eq!(agg.sum_gil, 12345 * 7);
}
