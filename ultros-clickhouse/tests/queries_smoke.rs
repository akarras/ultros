//! Integration test for [`queries::deep_scan_batch`].
//!
//! Run with:
//!   ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test queries_smoke -- --nocapture

use ultros_clickhouse::{ClickHouseClient, queries, rollups};

fn integration_enabled() -> bool {
    std::env::var("ULTROS_CH_INTEGRATION").is_ok()
}

fn load_env() {
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();
}

#[tokio::test]
async fn deep_scan_batch_returns_rows_for_real_items() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();

    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");

    // Make sure rollups exist so the test is independent of test ordering.
    rollups::refresh_window(&ch, 30).await.expect("refresh 30d");
    rollups::refresh_quality_scores(&ch).await.expect("scores");

    // Pick the top three real (item, world) tuples by sample_size — these
    // are guaranteed to have rollup data.
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Pick {
        item_id: i32,
        hq: u8,
        world_id: i32,
    }
    let picks: Vec<Pick> = ch
        .client()
        .query(
            "SELECT item_id, hq, world_id FROM item_stats_window FINAL \
             WHERE window_days = 30 ORDER BY sample_size DESC LIMIT 3",
        )
        .fetch_all()
        .await
        .expect("pick");
    assert!(!picks.is_empty(), "expected at least one rollup row");

    let req: Vec<(i32, u8, i32)> = picks
        .iter()
        .map(|p| (p.item_id, p.hq, p.world_id))
        .collect();
    let scans = queries::deep_scan_batch(&ch, 30, &req).await.expect("scan");

    assert_eq!(scans.len(), req.len(), "expected one row per request");
    for scan in &scans {
        eprintln!(
            "  item={} world={} n={} clean={} band={} qs={} vwap={} p50={}",
            scan.item_id,
            scan.world_id,
            scan.sample_size,
            scan.cleaned_sample_size,
            scan.confidence_band_raw,
            scan.quality_score,
            scan.vwap,
            scan.p50,
        );
        assert!(scan.sample_size > 0);
        assert!(scan.cleaned_sample_size <= scan.sample_size);
        // After a quality_score refresh, every rollup row should have a
        // matching score row.
        assert_ne!(scan.confidence_band_raw, "unknown");
    }
}

#[tokio::test]
async fn deep_scan_one_returns_none_for_missing_item() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();

    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");

    // Item id well outside any FFXIV range and not present in fixtures.
    let scan = queries::deep_scan_one(&ch, -999_999, false, 40, 30)
        .await
        .expect("query");
    assert!(scan.is_none());
}

#[tokio::test]
async fn deep_scan_batch_empty_request_returns_empty() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();

    let ch = ClickHouseClient::from_env();
    let scans = queries::deep_scan_batch(&ch, 30, &[]).await.expect("scan");
    assert!(scans.is_empty());
}
