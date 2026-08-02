//! Integration tests for the price_density aggregate.
//!
//! Run with a throwaway ClickHouse:
//!   docker run --rm -d -p 8123:8123 -e CLICKHOUSE_DB=ultros \
//!     -e CLICKHOUSE_USER=ultros -e CLICKHOUSE_PASSWORD= \
//!     --name ch-test clickhouse/clickhouse-server
//!   ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test price_density_smoke

use ultros_api_types::price_series::HqFilter;
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
const FIXTURE_ITEM_DENSITY: i32 = 999_000_006;

fn ts(secs: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(secs, 0).unwrap()
}

const T0: i64 = 1_700_006_400; // day-aligned

async fn seed(ch: &ClickHouseClient, item: i32) {
    // Mutation delete + mutations_sync=1: same rationale as the other smoke
    // tests (lightweight DELETE isn't assumed available; async mutation
    // would race the insert below).
    ch.client()
        .query("ALTER TABLE sales DELETE WHERE item_id = ? SETTINGS mutations_sync = 1")
        .bind(item)
        .execute()
        .await
        .expect("clear fixtures");

    // Prices 100..=400 across two day buckets: with lo=100, bin_width=100,
    // bins=4 the expected non-empty cells are unambiguous. 400 exercises the
    // top-edge clamp (floor((400-100)/100) = 3 = max bin only via least()).
    let rows = [
        // (pg_id, offset_secs, price)
        (1, 0, 100u32),   // day 0, bin 0
        (2, 60, 150),     // day 0, bin 0
        (3, 120, 250),    // day 0, bin 1
        (4, 86_400, 400), // day 1, bin 3 (clamped by least())
        (5, 86_460, 399), // day 1, bin 2
    ];
    let mut insert = ch
        .client()
        .insert::<SaleRow>("sales")
        .await
        .expect("insert");
    for (pg_id, offset, price) in rows {
        insert
            .write(&SaleRow {
                pg_id,
                sold_date: ts(T0 + offset),
                item_id: item,
                hq: 0,
                world_id: 1,
                price_per_item: price,
                quantity: 1,
                buying_character_id: 0,
                buyer_name: String::new(),
            })
            .await
            .expect("write");
    }
    insert.end().await.expect("end insert");
}

#[tokio::test]
async fn density_bins_and_counts_match_the_fixture() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    seed(&ch, FIXTURE_ITEM_DENSITY).await;

    let (lo, hi) = queries::price_min_max(
        &ch,
        FIXTURE_ITEM_DENSITY,
        &[1],
        HqFilter::Any,
        ts(T0),
        ts(T0 + 3 * 86_400),
    )
    .await
    .expect("min_max query")
    .expect("fixture has rows");
    assert_eq!((lo, hi), (100, 400));

    let rows = queries::price_density(
        &ch,
        FIXTURE_ITEM_DENSITY,
        &[1],
        HqFilter::Any,
        ts(T0),
        ts(T0 + 3 * 86_400),
        86_400,
        100,   // lo
        100.0, // bin_width -> bins are [100,200) [200,300) [300,400) [400,..]
        4,
    )
    .await
    .expect("density query");

    // (bucket_offset_days, bin, n)
    let got: Vec<(i64, u16, u64)> = rows
        .iter()
        .map(|r| ((r.bucket.timestamp() - T0) / 86_400, r.price_bin, r.n))
        .collect();
    assert_eq!(got, vec![(0, 0, 2), (0, 1, 1), (1, 2, 1), (1, 3, 1)]);
}

#[tokio::test]
async fn empty_window_returns_no_min_max() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    let none = queries::price_min_max(
        &ch,
        FIXTURE_ITEM_DENSITY,
        &[1],
        HqFilter::Any,
        ts(0),
        ts(60), // 1970: nothing there
    )
    .await
    .expect("query");
    assert!(
        none.is_none(),
        "count()=0 must map to None, not Some((0, 0))"
    );
}
