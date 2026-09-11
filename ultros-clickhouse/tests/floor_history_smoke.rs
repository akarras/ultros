//! Run against a disposable database with ULTROS_CH_INTEGRATION=1.
use ultros_api_types::HqFilter;
use ultros_clickhouse::{ClickHouseClient, floor_history::history};

#[tokio::test]
async fn floor_history_carries_state_and_preserves_empty_markets() {
    if std::env::var("ULTROS_CH_INTEGRATION").is_err() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 against a disposable ClickHouse");
        return;
    }
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.unwrap();
    // Negative fixture IDs cannot overlap real game items.
    let item = -(std::process::id() as i32);
    ch.client()
        .query(&format!(
            r#"
        INSERT INTO floor_changes VALUES
        (1000,{item},0,1,100,'listing'), (1000,{item},0,2,150,'listing'),
        (1000,{item},1,1,180,'listing'), (1150,{item},0,1,0,'listing'),
        (1210,{item},0,2,0,'listing'), (1270,{item},1,1,0,'listing'),
        (1330,{item},0,2,220,'listing'), (1390,{item},0,2,190,'listing')
    "#
        ))
        .execute()
        .await
        .unwrap();
    let all = history(&ch, item, &[1, 2], HqFilter::Any, 1120, 1420)
        .await
        .unwrap();
    assert_eq!(all.bucket_seconds, 60);
    assert_eq!(
        all.points.iter().map(|p| p.price).collect::<Vec<_>>(),
        vec![Some(100), Some(150), Some(180), None, Some(220), Some(190)]
    );
    let hq = history(&ch, item, &[1, 2], HqFilter::Hq, 1120, 1420)
        .await
        .unwrap();
    assert_eq!(hq.points[0].price, Some(180));
    assert_eq!(hq.points[3].price, None);
    let quiet = history(&ch, item, &[1], HqFilter::Hq, 1120, 1200)
        .await
        .unwrap();
    assert!(quiet.points.iter().all(|p| p.price == Some(180)));
    let missing = history(&ch, item, &[9999], HqFilter::Any, 1120, 1420)
        .await
        .unwrap();
    assert!(missing.points.is_empty());
    let full = history(&ch, item, &[1, 2], HqFilter::Any, 0, 1420)
        .await
        .unwrap();
    assert_eq!(
        full.from, 1000,
        "never invent history before first observation"
    );
    ch.client()
        .query(&format!(
            "ALTER TABLE floor_changes DELETE WHERE item_id = {item} SETTINGS mutations_sync = 1"
        ))
        .execute()
        .await
        .unwrap();
}

/// The boot resync anchors keys ClickHouse has never seen, so it must read the
/// floor ClickHouse currently believes: latest event wins, the cheaper of two
/// same-second rows wins (matching the history reader), and 0 is kept so the
/// caller can tell an emptied board from a key with no row at all.
#[tokio::test]
async fn latest_floors_reports_the_floor_the_history_reader_would_carry() {
    if std::env::var("ULTROS_CH_INTEGRATION").is_err() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 against a disposable ClickHouse");
        return;
    }
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.unwrap();
    let item = -(std::process::id() as i32) - 1_000_000;
    ch.client()
        .query(&format!(
            r#"
        INSERT INTO floor_changes VALUES
        (1000,{item},0,1,100,'listing'), (1200,{item},0,1,120,'refill'),
        (1200,{item},0,1,115,'listing'),
        (1000,{item},1,1,300,'listing'), (1500,{item},1,1,0,'refill'),
        (1000,{item},0,2,50,'resync')
    "#
        ))
        .execute()
        .await
        .unwrap();
    let mut rows = ultros_clickhouse::floor_history::latest_floors(&ch)
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.item_id == item)
        .map(|r| (r.world_id, r.hq, r.price))
        .collect::<Vec<_>>();
    rows.sort();
    assert_eq!(rows, vec![(1, 0, 115), (1, 1, 0), (2, 0, 50)]);
    ch.client()
        .query(&format!(
            "ALTER TABLE floor_changes DELETE WHERE item_id = {item} SETTINGS mutations_sync = 1"
        ))
        .execute()
        .await
        .unwrap();
}

/// A world that never listed an item has no `floor_changes` row, so on its
/// own it keeps a datacenter's floor unknown. A recorded boot anchor proves
/// the world empty from that instant on; the earliest anchor per world wins.
#[tokio::test]
async fn anchored_world_without_rows_is_known_empty_in_batch_bounds() {
    if std::env::var("ULTROS_CH_INTEGRATION").is_err() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 against a disposable ClickHouse");
        return;
    }
    use chrono::{TimeZone, Utc};
    use ultros_api_types::floor_history::{FloorHistoryRequest, FloorInterval};
    use ultros_clickhouse::{
        floor_history::{anchors, batch},
        rows::FloorAnchorRow,
        writer::insert_all,
    };
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.unwrap();
    // `batch` only accepts real (positive) item ids; stay far above game data.
    let item = i32::MAX - (std::process::id() as i32 % 1_000_000);
    ch.client()
        .query(&format!(
            "INSERT INTO floor_changes VALUES (1000,{item},0,1,100,'listing')"
        ))
        .execute()
        .await
        .unwrap();
    let rows = [
        FloorAnchorRow {
            anchored_at: Utc.timestamp_opt(5000, 0).unwrap(),
            world_id: 2,
        },
        FloorAnchorRow {
            anchored_at: Utc.timestamp_opt(1000, 0).unwrap(),
            world_id: 2,
        },
    ];
    insert_all(&ch, &rows, 10).await.unwrap();
    let found = anchors(&ch, &[1, 2, 3]).await.unwrap();
    assert_eq!(found, std::collections::BTreeMap::from([(2, 1000i64)]));

    let request = |to| FloorHistoryRequest {
        item_ids: vec![item],
        from: 1100,
        to,
        interval: FloorInterval::Hourly,
        hq: Some(false),
    };
    let known = batch(&ch, &[1, 2], &request(4700)).await.unwrap();
    let series = &known.series[0];
    assert_eq!(series.bounds.known_secs, 3600);
    assert_eq!(series.bounds.unknown_secs, 0);
    assert_eq!(series.bounds.min, Some(100));
    assert!(series.unknown_timestamps.is_empty());
    assert!(series.history.points.iter().all(|p| p.price == Some(100)));

    let unanchored = batch(&ch, &[1, 3], &request(4700)).await.unwrap();
    assert_eq!(unanchored.series[0].bounds.unknown_secs, 3600);
    assert_eq!(unanchored.series[0].bounds.min, None);

    for sql in [
        format!(
            "ALTER TABLE floor_changes DELETE WHERE item_id = {item} SETTINGS mutations_sync = 1"
        ),
        "ALTER TABLE floor_anchors DELETE WHERE world_id = 2 SETTINGS mutations_sync = 1"
            .to_string(),
    ] {
        ch.client().query(&sql).execute().await.unwrap();
    }
}
