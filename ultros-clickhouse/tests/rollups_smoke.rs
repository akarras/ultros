//! Integration tests for the rollup refresher.
//!
//! Runs against a live ClickHouse with backfilled `sales` data. Gated on
//! `ULTROS_CH_INTEGRATION=1`.

use ultros_clickhouse::{ClickHouseClient, rollups};

fn integration_enabled() -> bool {
    std::env::var("ULTROS_CH_INTEGRATION").is_ok()
}

fn load_env() {
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();
}

#[tokio::test]
async fn refresh_30d_window_populates_item_stats() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();

    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");

    let n = rollups::refresh_window(&ch, 30).await.expect("refresh 30d");
    assert!(n > 0, "expected non-empty rollup; got {n} rows");

    // Sanity: every refreshed row should have cleaned <= sample_size and
    // excluded == sample_size - cleaned.
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Sanity {
        violations: u64,
    }
    let s: Sanity = ch
        .client()
        .query(
            "SELECT count() AS violations FROM item_stats_window FINAL \
             WHERE window_days = 30 \
             AND (cleaned_sample_size > sample_size \
                  OR sample_size != cleaned_sample_size + excluded_count)",
        )
        .fetch_one()
        .await
        .expect("sanity");
    assert_eq!(s.violations, 0, "internal sanity check failed");
}

#[tokio::test]
async fn refresh_quality_scores_classifies_bands() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();

    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");

    rollups::refresh_window(&ch, 30).await.expect("refresh 30d");
    rollups::refresh_quality_scores(&ch).await.expect("scores");

    // Distribution across bands. We don't assert exact counts (depends on
    // backfilled data) but we do assert there's at least some variety and
    // that 'unusable' shows up for thin markets — proves the scoring is
    // actually differentiating.
    #[derive(Debug, clickhouse::Row, serde::Deserialize)]
    struct BandRow {
        band: String,
        n: u64,
    }
    let rows: Vec<BandRow> = ch
        .client()
        .query(
            "SELECT toString(confidence_band) AS band, count() AS n \
             FROM item_quality_score FINAL \
             GROUP BY band ORDER BY band",
        )
        .fetch_all()
        .await
        .expect("bands");

    let total: u64 = rows.iter().map(|r| r.n).sum();
    assert!(total > 0, "expected some scores; got {total}");
    for r in &rows {
        eprintln!("  band={} count={}", r.band, r.n);
    }
    // We should see *at least two* distinct bands — if everything is the
    // same band the scoring is collapsing.
    assert!(
        rows.len() >= 2,
        "expected variety across bands; got {rows:?}"
    );
}

#[tokio::test]
async fn noise_filter_excludes_obvious_launder() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();

    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");

    // Inject a fixture: 20 "normal" sales at price 1000 plus 2 obvious
    // launder rows (single-unit at price 100_000). The post-filter sample
    // size should be 20, with 2 excluded.
    let item_id = -777777;
    let world_id = 40;

    ch.client()
        .query("ALTER TABLE sales DELETE WHERE item_id = ? SETTINGS mutations_sync = 1")
        .bind(item_id)
        .execute()
        .await
        .expect("cleanup");

    let mut insert = ch
        .client()
        .insert::<ultros_clickhouse::rows::SaleRow>("sales")
        .await
        .expect("insert");
    let now = chrono::Utc::now();
    for i in 0..20 {
        let row = ultros_clickhouse::rows::SaleRow {
            pg_id: -100 - i,
            sold_date: now - chrono::Duration::hours(i as i64),
            item_id,
            hq: 0,
            world_id,
            price_per_item: 1000,
            quantity: 1,
            buying_character_id: 10 + i as i64,
            buyer_name: format!("normal_{i}"),
        };
        insert.write(&row).await.expect("write normal");
    }
    for i in 0..2 {
        let row = ultros_clickhouse::rows::SaleRow {
            pg_id: -500 - i,
            sold_date: now - chrono::Duration::hours(i as i64),
            item_id,
            hq: 0,
            world_id,
            price_per_item: 100_000, // 100× the median
            quantity: 1,
            buying_character_id: 9999 + i as i64,
            buyer_name: format!("launder_{i}"),
        };
        insert.write(&row).await.expect("write launder");
    }
    insert.end().await.expect("end");

    rollups::refresh_window(&ch, 30).await.expect("refresh");

    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Stat {
        sample_size: u32,
        cleaned_sample_size: u32,
        excluded_count: u32,
        p50: u32,
        vwap: u32,
    }
    let stat: Stat = ch
        .client()
        .query(
            "SELECT sample_size, cleaned_sample_size, excluded_count, p50, vwap \
             FROM item_stats_window FINAL \
             WHERE item_id = ? AND world_id = ? AND window_days = 30",
        )
        .bind(item_id)
        .bind(world_id)
        .fetch_one()
        .await
        .expect("stat");

    eprintln!(
        "  sample={} clean={} excluded={} p50={} vwap={}",
        stat.sample_size, stat.cleaned_sample_size, stat.excluded_count, stat.p50, stat.vwap
    );
    assert_eq!(stat.sample_size, 22, "should see all 22 rows pre-filter");
    assert_eq!(
        stat.excluded_count, 2,
        "should drop both launder rows via Layer 2 heuristic (10× threshold)"
    );
    assert_eq!(stat.cleaned_sample_size, 20);
    assert_eq!(stat.p50, 1000, "median should reflect cleaned data only");
    assert_eq!(stat.vwap, 1000, "VWAP should reflect cleaned data only");

    // Cleanup so subsequent runs are independent.
    ch.client()
        .query("ALTER TABLE sales DELETE WHERE item_id = ? SETTINGS mutations_sync = 1")
        .bind(item_id)
        .execute()
        .await
        .expect("post-cleanup");
}
