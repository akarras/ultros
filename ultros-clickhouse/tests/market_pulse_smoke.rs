//! Integration tests for the Market Pulse rollup + query.
//!
//! Run with:
//!   ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test market_pulse_smoke -- --nocapture

use ultros_clickhouse::{ClickHouseClient, queries, rollups};

fn integration_enabled() -> bool {
    std::env::var("ULTROS_CH_INTEGRATION").is_ok()
}

fn load_env() {
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();
}

#[tokio::test]
async fn refresh_then_query_market_pulse() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();

    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");

    let buckets = rollups::refresh_world_kpi_5min(&ch).await.expect("refresh");
    eprintln!("  populated {buckets} 5min buckets across the 50h window");
    assert!(
        buckets > 0,
        "expected at least one 5min bucket from the backfilled data"
    );

    // Pick the most-active world over the trailing 48h so the test is
    // meaningful even on a quiet day.
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct TopWorld {
        world_id: i32,
    }
    let top: TopWorld = ch
        .client()
        .query(
            "SELECT world_id FROM world_kpi_5min FINAL \
             WHERE bucket > now() - INTERVAL 48 HOUR \
             GROUP BY world_id \
             ORDER BY sum(sale_count) DESC LIMIT 1",
        )
        .fetch_one()
        .await
        .expect("top world");

    let pulse = queries::market_pulse(&ch, top.world_id)
        .await
        .expect("query");

    eprintln!(
        "  world={} sales today={} yesterday={} ({:?}%)  gil today={} yesterday={} ({:?}%)",
        pulse.world_id,
        pulse.sales_today,
        pulse.sales_yesterday,
        pulse.sales_delta_pct(),
        pulse.gil_volume_today,
        pulse.gil_volume_yesterday,
        pulse.gil_volume_delta_pct(),
    );

    // Today's sales should be at least 1 (we picked the most-active world).
    // Yesterday could legitimately be 0 if the backfill window didn't cover
    // it — accept None delta in that case.
    assert!(
        pulse.sales_today > 0,
        "top world should have non-zero today"
    );
    if pulse.sales_yesterday > 0 {
        let pct = pulse.sales_delta_pct();
        assert!(pct.is_some());
    }
}
