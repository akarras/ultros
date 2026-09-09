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
