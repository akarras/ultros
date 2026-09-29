//! Deterministic fixtures in an explicitly disposable, loopback-only database.
//! ULTROS_CH_INTEGRATION=1 CLICKHOUSE_DATABASE=ultros_t12_* cargo test -p ultros-clickhouse --test undercut_pressure_smoke
use ultros_api_types::{price_series::HqFilter, undercut_pressure::PressureState};
use ultros_clickhouse::{ClickHouseClient, undercut_pressure};

#[tokio::test]
async fn pressure_counts_both_reprice_shapes_and_one_episode() {
    if std::env::var("ULTROS_CH_INTEGRATION").is_err() {
        return;
    }
    assert!(
        std::env::var("CLICKHOUSE_URL")
            .unwrap()
            .starts_with("http://127.0.0.1:")
    );
    assert!(
        std::env::var("CLICKHOUSE_DATABASE")
            .unwrap()
            .starts_with("ultros_t12_")
    );
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.unwrap();
    let now = ch
        .client()
        .query("SELECT toInt64(now())")
        .fetch_one::<i64>()
        .await
        .unwrap();
    let hour = now.div_euclid(3600) * 3600 - 3600; // one full hour in the past
    let item = 800_000 + (std::process::id() % 100_000) as i32;
    let world = 9001;
    let t = |s: i64| hour + s;
    // listing_events columns: event_time, kind, source, item_id, hq, world_id,
    // listing_id, pg_listing_id, retainer_id, price_per_unit, quantity,
    // prev_price, prev_quantity, reviewed_at
    let events = format!(
        "INSERT INTO listing_events VALUES
        ({},'updated','websocket',{item},0,{world},'a',1,11,90,1,100,1,{}),
        ({},'removed','websocket',{item},0,{world},'b',2,12,100,1,0,0,{}),
        ({},'added','websocket',{item},0,{world},'b',2,12,80,1,0,0,{}),
        ({},'removed','websocket',{item},0,{world},'c',3,13,100,1,0,0,{}),
        ({},'added','websocket',{item},0,{world},'c',3,13,120,1,0,0,{}),
        ({},'updated','snapshot',{item},0,{world},'d',4,14,50,1,100,1,{}),
        ({},'updated','websocket',{item},1,{world},'e',5,15,50,1,100,1,{})",
        t(100),
        t(100),
        t(200),
        t(200),
        t(230),
        t(230),
        t(300),
        t(300),
        t(330),
        t(330),
        t(400),
        t(400),
        t(500),
        t(500)
    );
    ch.client().query(&events).execute().await.unwrap();
    ch.client().query(&events).execute().await.unwrap(); // retried batch: DISTINCT must dedupe
    ch.client()
        .query(&format!(
            "INSERT INTO floor_changes VALUES ({},{item},0,{world},100,'listing'),({},{item},0,{world},90,'listing'),({},{item},0,{world},80,'listing')",
            t(0), t(100), t(230)
        ))
        .execute()
        .await
        .unwrap();
    let anchor = hour - 7200;
    ch.client()
        .query(&format!(
            "INSERT INTO floor_anchors VALUES ({anchor},{world})"
        ))
        .execute()
        .await
        .unwrap();

    let out = undercut_pressure::load(
        &ch,
        item,
        world,
        HqFilter::Nq,
        undercut_pressure::PressureParams {
            world_id: world,
            from: hour,
            to: hour + 3600,
            bucket_seconds: 3600,
            now,
            anchor: Some(anchor),
        },
    )
    .await
    .unwrap();
    assert_eq!(out.buckets.len(), 1);
    let b = &out.buckets[0];
    // NQ only: updated 100→90 and removed@100/added@80; not the raise, the snapshot or HQ.
    assert_eq!((b.trims, b.cuts, b.sellers), (0, 2, 2));
    // One known bucket → baseline 2 → war needs 4 undercuts → churn.
    assert_eq!(b.state, PressureState::Churn);
    // The carried start is the anchor (empty board). Then 100 at t(0) → 90
    // at t(100) → 80 at t(230): two closed episodes (100 s, 130 s), both
    // ended by an undercut; 80 is still open.
    assert_eq!(
        (out.summary.episodes_left, out.summary.episodes_undercut),
        (0, 2)
    );
    assert_eq!(out.summary.floor_holds_median_secs, Some(115));
}
