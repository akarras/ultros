//! Integration tests for sales_hourly + sparklines + top_movers.

use ultros_clickhouse::{
    ClickHouseClient,
    queries::{self, MoverDirection},
    rollups,
};

fn integration_enabled() -> bool {
    std::env::var("ULTROS_CH_INTEGRATION").is_ok()
}

fn load_env() {
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();
}

#[tokio::test]
async fn refresh_sales_hourly_populates() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");

    let n = rollups::refresh_sales_hourly(&ch).await.expect("refresh");
    eprintln!("  sales_hourly rows: {n}");
    assert!(n > 0, "expected non-empty rollup on backfilled data");
}

#[tokio::test]
async fn sparklines_batch_returns_aligned_arrays() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    rollups::refresh_sales_hourly(&ch).await.expect("refresh");

    // Pick a few real items with recent activity so the array isn't all zeros.
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Pick {
        item_id: i32,
        hq: u8,
        world_id: i32,
    }
    let picks: Vec<Pick> = ch
        .client()
        .query(
            "SELECT item_id, hq, world_id FROM sales_hourly FINAL \
             WHERE bucket > now() - INTERVAL 24 HOUR \
             GROUP BY item_id, hq, world_id \
             ORDER BY sum(unit_volume) DESC LIMIT 3",
        )
        .fetch_all()
        .await
        .expect("pick");
    assert!(
        !picks.is_empty(),
        "expected at least one item with 24h activity"
    );

    let req: Vec<(i32, u8, i32)> = picks
        .iter()
        .map(|p| (p.item_id, p.hq, p.world_id))
        .collect();
    let rows = queries::sparklines_batch(&ch, &req, 24)
        .await
        .expect("sparkline");

    assert_eq!(rows.len(), req.len());
    for r in &rows {
        eprintln!(
            "  item={} hq={} world={} points={} first={} last={} pct={:.1}%",
            r.item_id,
            r.hq,
            r.world_id,
            r.points.len(),
            r.first_price,
            r.last_price,
            r.pct_change(),
        );
        assert_eq!(r.points.len(), 24, "expected exactly 24 hourly buckets");
        // At least one bucket should be non-zero for an active item.
        assert!(r.points.iter().any(|&p| p > 0));
    }
}

#[tokio::test]
async fn top_movers_returns_rising_in_descending_order() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    rollups::refresh_sales_hourly(&ch).await.expect("refresh");

    // Find the world with the most volume to maximize the chance of having
    // mover candidates that pass the >=3 sales filter.
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Top {
        world_id: i32,
    }
    let top: Option<Top> = ch
        .client()
        .query(
            "SELECT world_id FROM sales_hourly FINAL \
             WHERE bucket > now() - INTERVAL 24 HOUR \
             GROUP BY world_id \
             ORDER BY sum(unit_volume) DESC LIMIT 1",
        )
        .fetch_optional()
        .await
        .expect("top world");
    let Some(top) = top else {
        eprintln!("no worlds with activity — skipping");
        return;
    };

    let risers = queries::top_movers(&ch, top.world_id, MoverDirection::Rising, 5)
        .await
        .expect("rising");
    eprintln!("  world={} top risers:", top.world_id);
    for r in &risers {
        eprintln!(
            "    item={} hq={} price_now={} pct={:.1}% vol={}",
            r.item_id, r.hq, r.price_now, r.pct_change_24h, r.volume_24h
        );
    }
    if risers.len() >= 2 {
        // Descending: each row's pct should be <= the previous.
        for w in risers.windows(2) {
            assert!(
                w[0].pct_change_24h >= w[1].pct_change_24h,
                "rising movers must be sorted desc by pct_change_24h"
            );
        }
    }

    let fallers = queries::top_movers(&ch, top.world_id, MoverDirection::Falling, 5)
        .await
        .expect("falling");
    if fallers.len() >= 2 {
        for w in fallers.windows(2) {
            assert!(
                w[0].pct_change_24h <= w[1].pct_change_24h,
                "falling movers must be sorted asc by pct_change_24h"
            );
        }
    }

    let by_vol = queries::top_movers(&ch, top.world_id, MoverDirection::Volume, 5)
        .await
        .expect("volume");
    if by_vol.len() >= 2 {
        for w in by_vol.windows(2) {
            assert!(
                w[0].volume_24h >= w[1].volume_24h,
                "volume movers must be sorted desc by volume_24h"
            );
        }
    }
}
