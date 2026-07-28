//! Integration tests for the price_series aggregate.
//!
//! Run with a throwaway ClickHouse:
//!   docker run --rm -d -p 8123:8123 -e CLICKHOUSE_DB=ultros \
//!     -e CLICKHOUSE_USER=ultros -e CLICKHOUSE_PASSWORD= \
//!     --name ch-test clickhouse/clickhouse-server
//!   ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test price_series_smoke

use ultros_api_types::price_series::{HqFilter, SeriesGroup};
use ultros_clickhouse::{ClickHouseClient, queries, rows::SaleRow};

fn integration_enabled() -> bool {
    std::env::var("ULTROS_CH_INTEGRATION").is_ok()
}

fn load_env() {
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();
}

/// Item id far outside the real range so fixtures never collide with
/// backfilled production data in a shared dev ClickHouse.
const FIXTURE_ITEM: i32 = 999_000_001;

fn ts(secs: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(secs, 0).unwrap()
}

/// Base timestamp aligned to a day boundary, so bucket assignment in the
/// assertions is unambiguous.
const T0: i64 = 1_700_006_400; // 2023-11-15 00:00:00 UTC

async fn seed(ch: &ClickHouseClient) {
    // `ALTER TABLE ... DELETE` (a mutation) rather than the lightweight
    // `DELETE FROM`: the rest of this crate's smoke tests (schema_smoke,
    // writer_smoke, rollups_smoke, vendor_filter_smoke) all clear fixtures
    // this way, and lightweight deletes need
    // `allow_experimental_lightweight_delete` enabled on older/unconfigured
    // servers, which we can't assume for an arbitrary dev ClickHouse.
    // `mutations_sync = 1` blocks until the mutation finishes — without it
    // the call returns immediately and the subsequent insert+query would
    // race against not-yet-deleted leftover rows from a prior run.
    ch.client()
        .query("ALTER TABLE sales DELETE WHERE item_id = ? SETTINGS mutations_sync = 1")
        .bind(FIXTURE_ITEM)
        .execute()
        .await
        .expect("clear fixtures");

    // Deliberately inserted out of chronological order so argMin/argMax are
    // proven to key on sold_date rather than on insertion order.
    let rows = [
        // (pg_id, offset_secs, world_id, price, qty, hq)
        (1, 3_600, 1, 300u32, 2u16, 0u8),
        (2, 0, 1, 100, 1, 0),
        (3, 7_200, 1, 200, 1, 1),
        (4, 1_800, 2, 500, 4, 0),
    ];
    let mut insert = ch
        .client()
        .insert::<SaleRow>("sales")
        .await
        .expect("insert");
    for (pg_id, offset, world_id, price, quantity, hq) in rows {
        insert
            .write(&SaleRow {
                pg_id,
                sold_date: ts(T0 + offset),
                item_id: FIXTURE_ITEM,
                hq,
                world_id,
                price_per_item: price,
                quantity,
                buying_character_id: 0,
                buyer_name: String::new(),
            })
            .await
            .expect("write");
    }
    insert.end().await.expect("end insert");
}

#[tokio::test]
async fn ohlc_keys_on_sold_date_not_insertion_order() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    seed(&ch).await;

    let rows = queries::price_series(
        &ch,
        FIXTURE_ITEM,
        &[(1, 10), (2, 10)],
        SeriesGroup::World,
        HqFilter::Any,
        ts(T0),
        ts(T0 + 86_400),
        86_400,
    )
    .await
    .expect("query");

    let world1 = rows.iter().find(|r| r.series_id == 1).expect("world 1");
    assert_eq!(world1.open, 100, "earliest sale by sold_date");
    assert_eq!(world1.close, 200, "latest sale by sold_date");
    assert_eq!(world1.high, 300);
    assert_eq!(world1.low, 100);
    assert_eq!(world1.sales, 3);
}

#[tokio::test]
async fn gil_and_units_reproduce_vwap() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    seed(&ch).await;

    let rows = queries::price_series(
        &ch,
        FIXTURE_ITEM,
        &[(1, 10), (2, 10)],
        SeriesGroup::World,
        HqFilter::Any,
        ts(T0),
        ts(T0 + 86_400),
        86_400,
    )
    .await
    .expect("query");

    // World 1: 100*1 + 300*2 + 200*1 = 900 gil over 4 units = 225.
    let world1 = rows.iter().find(|r| r.series_id == 1).expect("world 1");
    assert_eq!(world1.gil, 900);
    assert_eq!(world1.units, 4);
    assert_eq!(world1.gil as f64 / world1.units as f64, 225.0);
}

#[tokio::test]
async fn datacenter_grouping_merges_worlds_and_recomputes_quantiles() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    seed(&ch).await;

    let rows = queries::price_series(
        &ch,
        FIXTURE_ITEM,
        &[(1, 10), (2, 10)],
        SeriesGroup::Datacenter,
        HqFilter::Any,
        ts(T0),
        ts(T0 + 86_400),
        86_400,
    )
    .await
    .expect("query");

    assert_eq!(rows.len(), 1, "both worlds collapse into one datacenter");
    let dc = &rows[0];
    assert_eq!(dc.series_id, 10);
    assert_eq!(dc.high, 500, "max across both worlds");
    assert_eq!(dc.sales, 4);
    // p50 over [100,200,300,500] is 300 — not derivable from either world's
    // own median (200 and 500). This is the regression guard for grouping
    // being a server-side concern.
    assert_eq!(dc.p50, 300);
}

#[tokio::test]
async fn hq_filter_narrows_the_result() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    seed(&ch).await;

    let rows = queries::price_series(
        &ch,
        FIXTURE_ITEM,
        &[(1, 10), (2, 10)],
        SeriesGroup::World,
        HqFilter::Hq,
        ts(T0),
        ts(T0 + 86_400),
        86_400,
    )
    .await
    .expect("query");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].sales, 1);
    assert_eq!(rows[0].open, 200);
}

/// Regression guard for the bug where the price_series web handler fetched
/// raw sales from Postgres (no date bound, filtered client-side) instead of
/// from ClickHouse: a window that doesn't reach "now" silently came back
/// empty even though sales existed in range. `raw_sales` sources from the
/// same `sales` table as `price_series`, with the same `WHERE` shape, so
/// this asserts it actually respects `[from, to)` instead of returning
/// everything (or nothing).
#[tokio::test]
async fn raw_sales_respects_the_requested_window() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    seed(&ch).await;

    // Fixture rows are at offsets 0, 1_800, 3_600, 7_200. A window of
    // [T0 + 1_800, T0 + 7_200) should include the offset-1_800 and
    // offset-3_600 rows but exclude offset-0 (before `from`) and offset-7_200
    // (at `to`, which is exclusive).
    let rows = queries::raw_sales(
        &ch,
        FIXTURE_ITEM,
        &[1, 2],
        HqFilter::Any,
        ts(T0 + 1_800),
        ts(T0 + 7_200),
        2_000,
    )
    .await
    .expect("query");

    assert_eq!(rows.len(), 2, "only the two in-window rows come back");
    assert!(
        rows.iter()
            .all(|r| r.sold_date >= ts(T0 + 1_800) && r.sold_date < ts(T0 + 7_200)),
        "every row falls inside [from, to)"
    );
    let mut prices: Vec<u32> = rows.iter().map(|r| r.price_per_item).collect();
    prices.sort_unstable();
    assert_eq!(
        prices,
        vec![300, 500],
        "the offset-0 and offset-7_200 rows are excluded"
    );
}
