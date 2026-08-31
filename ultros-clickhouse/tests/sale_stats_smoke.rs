//! Integration tests for the widened bulk_sale_stats + bulk_confidence.
//!
//! Run with a throwaway ClickHouse:
//!   docker run --rm -d -p 8123:8123 -e CLICKHOUSE_DB=ultros \
//!     -e CLICKHOUSE_USER=ultros -e CLICKHOUSE_PASSWORD= \
//!     --name ch-test clickhouse/clickhouse-server
//!   ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test sale_stats_smoke

use ultros_api_types::trends::ConfidenceBand;
use ultros_clickhouse::{ClickHouseClient, queries, rows::SaleRow};

fn integration_enabled() -> bool {
    std::env::var("ULTROS_CH_INTEGRATION").is_ok()
}

fn load_env() {
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();
}

/// Distinct from every fixture id in the sibling smoke tests — cargo runs
/// test files concurrently against the same throwaway server.
const FIXTURE_ITEM_SALE_STATS: i32 = 999_000_007;
const FIXTURE_WORLD: i32 = 999_001;

async fn seed(ch: &ClickHouseClient, item: i32) -> (i64, i64) {
    ch.client()
        .query("ALTER TABLE sales DELETE WHERE item_id = ? SETTINGS mutations_sync = 1")
        .bind(item)
        .execute()
        .await
        .expect("clear sales fixtures");
    ch.client()
        .query("ALTER TABLE item_quality_score DELETE WHERE item_id = ? SETTINGS mutations_sync = 1")
        .bind(item)
        .execute()
        .await
        .expect("clear quality fixtures");

    // bulk_sale_stats windows on `now()`, so the fixture must be recent.
    let now = chrono::Utc::now().timestamp();
    let older = now - 3_600;
    let newer = now - 60;

    // (pg_id, unix, price, qty): VWAP must weight by quantity —
    // (100*1 + 200*3) / 4 = 175, not the flat mean 150.
    let rows = [(1i32, older, 100u32, 1u16), (2, newer, 200, 3)];
    let mut insert = ch
        .client()
        .insert::<SaleRow>("sales")
        .await
        .expect("insert");
    for (pg_id, unix, price, qty) in rows {
        insert
            .write(&SaleRow {
                pg_id,
                sold_date: chrono::DateTime::from_timestamp(unix, 0).unwrap(),
                item_id: item,
                hq: 0,
                world_id: FIXTURE_WORLD,
                price_per_item: price,
                quantity: qty,
                buying_character_id: 0,
                buyer_name: String::new(),
            })
            .await
            .expect("write");
    }
    insert.end().await.expect("end insert");

    ch.client()
        .query(
            "INSERT INTO item_quality_score
             (item_id, hq, world_id, computed_at, quality_score,
              confidence_band, sample_size_30d, launder_suspicion_pct)
             VALUES (?, 0, ?, now(), 80, 'medium', 42, 0.1)",
        )
        .bind(item)
        .bind(FIXTURE_WORLD)
        .execute()
        .await
        .expect("insert quality score");

    (older, newer)
}

#[tokio::test]
async fn widened_columns_match_the_fixture() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    let (_older, newer) = seed(&ch, FIXTURE_ITEM_SALE_STATS).await;

    let rows = queries::bulk_sale_stats(&ch, &[FIXTURE_WORLD], 7)
        .await
        .expect("bulk_sale_stats");
    let row = rows
        .iter()
        .find(|r| r.item_id == FIXTURE_ITEM_SALE_STATS)
        .expect("fixture row present");

    assert_eq!(row.num_sold, 2);
    assert_eq!(row.min_price, 100);
    assert_eq!(row.units_sold, 4);
    // Quantity-weighted, not the flat mean of 150.
    assert_eq!(row.vwap, 175);
    assert_eq!(row.last_sold_unix, newer);

    let bands = queries::bulk_confidence(&ch, FIXTURE_WORLD)
        .await
        .expect("bulk_confidence");
    let band = bands
        .iter()
        .find(|r| r.item_id == FIXTURE_ITEM_SALE_STATS)
        .expect("quality row present");
    assert_eq!(band.confidence_band(), ConfidenceBand::Medium);
}
