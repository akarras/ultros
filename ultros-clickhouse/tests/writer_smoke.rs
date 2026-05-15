//! Integration test for the buffered Writer.
//!
//! Run with:
//!   ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test writer_smoke -- --nocapture

use std::time::Duration;

use chrono::{NaiveDate, Utc};
use tokio_util::sync::CancellationToken;
use ultros_clickhouse::{ClickHouseClient, rows::SaleRow, writer::Writer};

fn integration_enabled() -> bool {
    std::env::var("ULTROS_CH_INTEGRATION").is_ok()
}

fn fixture_row(item_id: i32, offset_minutes: i64) -> SaleRow {
    let base = NaiveDate::from_ymd_opt(2026, 5, 15)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        + chrono::Duration::minutes(offset_minutes);
    SaleRow {
        pg_id: -(1000 + offset_minutes as i32),
        sold_date: chrono::DateTime::from_naive_utc_and_offset(base, Utc),
        item_id,
        hq: 1,
        world_id: 40,
        price_per_item: 1000 + offset_minutes as u32,
        quantity: 1,
        buying_character_id: 1234 + offset_minutes,
        buyer_name: "writer-smoke".to_string(),
    }
}

#[tokio::test]
async fn batch_size_triggers_flush() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();

    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");

    // Sentinel item — well outside real ranges. Clean up first.
    let item_id = -515151;
    ch.client()
        .query("ALTER TABLE sales DELETE WHERE item_id = ?")
        .bind(item_id)
        .execute()
        .await
        .expect("cleanup");

    // Spawn with tiny batch (5) and long interval (60s) so only the batch-size
    // trigger fires within the test window.
    let token = CancellationToken::new();
    let writer = Writer::spawn_with_config(ch.clone(), token.clone(), 5, Duration::from_secs(60));

    // Send exactly batch_size rows. Then sleep briefly to let the flush happen.
    for i in 0..5 {
        writer.send(fixture_row(item_id, i));
    }
    // Yield enough times for the flush task to run.
    tokio::time::sleep(Duration::from_millis(500)).await;

    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Count {
        n: u64,
    }
    let count: Count = ch
        .client()
        .query("SELECT count() AS n FROM sales WHERE item_id = ?")
        .bind(item_id)
        .fetch_one()
        .await
        .expect("count after batch flush");
    assert_eq!(
        count.n, 5,
        "expected batch-size flush to land all 5 rows promptly"
    );

    token.cancel();
}

#[tokio::test]
async fn interval_triggers_partial_flush() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();

    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");

    let item_id = -515152;
    ch.client()
        .query("ALTER TABLE sales DELETE WHERE item_id = ?")
        .bind(item_id)
        .execute()
        .await
        .expect("cleanup");

    // Big batch (1000) so batch-size flush won't fire. Short interval (200ms).
    let token = CancellationToken::new();
    let writer =
        Writer::spawn_with_config(ch.clone(), token.clone(), 1000, Duration::from_millis(200));

    // Send 3 rows — less than batch size; only the interval can flush.
    for i in 100..103 {
        writer.send(fixture_row(item_id, i));
    }
    // Wait two interval ticks to be safe.
    tokio::time::sleep(Duration::from_millis(600)).await;

    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Count {
        n: u64,
    }
    let count: Count = ch
        .client()
        .query("SELECT count() AS n FROM sales WHERE item_id = ?")
        .bind(item_id)
        .fetch_one()
        .await
        .expect("count after interval flush");
    assert_eq!(count.n, 3, "expected interval flush to land partial batch");

    token.cancel();
}

#[tokio::test]
async fn cancellation_drains_remaining_rows() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();

    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");

    let item_id = -515153;
    ch.client()
        .query("ALTER TABLE sales DELETE WHERE item_id = ?")
        .bind(item_id)
        .execute()
        .await
        .expect("cleanup");

    // Big batch, long interval — neither trigger will fire on its own.
    let token = CancellationToken::new();
    let writer =
        Writer::spawn_with_config(ch.clone(), token.clone(), 1000, Duration::from_secs(60));

    for i in 200..207 {
        writer.send(fixture_row(item_id, i));
    }
    // Cancel — the writer's drain path should flush the 7 buffered rows.
    token.cancel();
    // Give the task time to flush and exit cleanly.
    tokio::time::sleep(Duration::from_millis(500)).await;

    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Count {
        n: u64,
    }
    let count: Count = ch
        .client()
        .query("SELECT count() AS n FROM sales WHERE item_id = ?")
        .bind(item_id)
        .fetch_one()
        .await
        .expect("count after cancel-drain");
    assert_eq!(count.n, 7, "expected cancellation to drain buffered rows");
}
