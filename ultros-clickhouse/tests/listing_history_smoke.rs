//! Deterministic fixtures in an explicitly disposable, loopback-only database.
//! ULTROS_CH_INTEGRATION=1 CLICKHOUSE_DATABASE=ultros_t12_* cargo test ...
use ultros_api_types::floor_history::{FloorHistoryRequest, FloorInterval};
use ultros_clickhouse::{ClickHouseClient, floor_history, listing_history};

#[tokio::test]
async fn window_history_receipts_floors_stock_and_bounds() {
    if std::env::var("ULTROS_CH_INTEGRATION").is_err() {
        return;
    }
    let url = std::env::var("CLICKHOUSE_URL").unwrap();
    assert!(
        url.starts_with("http://127.0.0.1:"),
        "disposable loopback fixture required"
    );
    assert!(
        std::env::var("CLICKHOUSE_DATABASE")
            .unwrap()
            .starts_with("ultros_t12_")
    );
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.unwrap();
    ch.migrate().await.unwrap();
    let to = chrono::Utc::now().timestamp();
    let from = to - 86400;
    let removal = to - 2000;
    let item = 700_000 + (std::process::id() % 100000) as i32;
    // All fixture IDs are isolated by the process-specific database and item.
    let sql = format!(
        "INSERT INTO listing_events VALUES
        ({},'added','websocket',{item},0,1,'normal',1,1,100,2,0,0,{}),
        ({removal},'removed','websocket',{item},0,1,'normal',1,1,100,2,0,0,{}),
        ({removal},'removed','websocket',{item},0,1,'ambiguous-a',2,2,200,2,0,0,{}),
        ({removal},'removed','websocket',{item},0,1,'ambiguous-b',3,3,200,2,0,0,{}),
        ({removal},'removed','websocket',{item},0,1,'reprice',4,4,300,2,0,0,{}),
        ({},'added','websocket',{item},0,1,'reprice-new',5,4,250,2,0,0,{}),
        ({},'added','snapshot',{item},0,1,'seed',6,6,400,2,0,0,{}),
        ({},'added','websocket',{item},0,1,'old',7,7,500,2,0,0,{}),
        ({},'updated','websocket',{item},0,1,'update',8,8,50,3,60,3,{}),
        ({},'removed','websocket',{item},0,2,'pending',9,9,800,2,0,0,{})",
        from + 100,
        removal - 3600,
        removal - 3600,
        removal - 3600,
        removal - 3600,
        removal - 3600,
        removal + 1,
        removal,
        from + 50,
        from + 50,
        to - 2 * 86400,
        to - 2 * 86400,
        removal + 2,
        removal,
        to - 100,
        removal
    );
    ch.client().query(&sql).execute().await.unwrap();
    ch.client()
        .query(&format!(
            "INSERT INTO sale_receipts VALUES
        (1,{}, {},{item},0,1,100,2),(1,{}, {},{item},0,1,100,2),
        (2,{}, {},{item},0,1,200,2),(3,{}, {},{item},0,1,300,2)",
            removal + 1,
            removal,
            removal + 1,
            removal,
            removal + 1,
            removal,
            removal + 1,
            removal
        ))
        .execute()
        .await
        .unwrap();
    ch.client().query(&format!("INSERT INTO sales (pg_id,sold_date,item_id,hq,world_id,price_per_item,quantity,buying_character_id) VALUES (99,{removal},{item},0,1,900,2,1)")).execute().await.unwrap();
    ch.client()
        .query(&format!(
            "INSERT INTO floor_changes VALUES
        ({},{item},0,1,100,'listing'),({},{item},0,2,80,'listing'),
        ({},{item},0,2,0,'refill'),({},{item},0,1,120,'listing'),
        ({},{item},0,1,0,'refill'),
        ({},{item},1,1,200,'listing')",
            from - 100,
            from - 100,
            from + 100,
            from + 200,
            from + 300,
            from + 50
        ))
        .execute()
        .await
        .unwrap();
    let rows = listing_history::window(&ch, &[1, 2], 1, to).await.unwrap();
    let stats = &rows[&(item, false)];
    assert_eq!(stats.additions, 2);
    assert_eq!(stats.removals, 5);
    assert_eq!(stats.matches.matched, 1);
    assert_eq!(stats.matches.ambiguous, 2);
    assert_eq!(stats.matches.repriced, 1);
    assert_eq!(stats.matches.pending, 1);
    assert_eq!(stats.matches.median_time_to_sell_secs, Some(3600));
    assert_eq!(stats.matches.sales_without_receipt, 1);
    assert_eq!((stats.floor_min, stats.floor_max), (Some(80), Some(120)));
    assert_eq!(stats.floor_empty_secs, 86400 - 300);
    assert_eq!(stats.floor_unknown_secs, 0);
    assert!(!stats.listing_coverage.continuity_verified);
    assert!(stats.listing_coverage.observed_span_secs < 86400);
    for days in [7, 30, 90] {
        let longer = listing_history::window(&ch, &[1, 2], days, to)
            .await
            .unwrap();
        assert_eq!(longer[&(item, false)].additions, 3);
        assert!(
            longer[&(item, false)].listing_coverage.observed_span_secs < u64::from(days) * 86400
        );
    }
    let mut req = FloorHistoryRequest {
        item_ids: vec![item, item + 1],
        from,
        to,
        interval: FloorInterval::Hourly,
        hq: None,
    };
    let batch = floor_history::batch(&ch, &[1, 2], &req).await.unwrap();
    let nq = &batch.series[0];
    assert_eq!(nq.bounds.max, Some(120));
    assert_eq!(nq.history.points[0].price, Some(80));
    assert_eq!(nq.history.points[1].price, None);
    assert!(nq.unknown_timestamps.is_empty());
    let hq = &batch.series[1];
    assert_eq!(hq.bounds.unknown_secs, 86400);
    assert!(!hq.unknown_timestamps.is_empty());
    assert!(
        batch.series[2]
            .history
            .points
            .iter()
            .all(|p| p.price.is_none())
    );
    req.interval = FloorInterval::Daily;
    assert_eq!(
        floor_history::batch(&ch, &[1, 2], &req)
            .await
            .unwrap()
            .series[0]
            .history
            .points
            .len(),
        2
    );
    req.item_ids = vec![item; 21];
    assert!(floor_history::batch(&ch, &[1, 2], &req).await.is_err());
    assert!(
        listing_history::stock(&ch, &[1, 2], 1)
            .await
            .unwrap()
            .is_empty()
    );
    for world in [1, 2] {
        ch.client().query(&format!("INSERT INTO listing_alive SELECT {world},{item},toUInt8(0),now(),toUInt32(1),toUInt64(100),toUInt32(1),now(),quantileTDigestState(0.5)(toUInt32(0)),toUInt32(100)")).execute().await.unwrap();
        ch.client().query(&format!("INSERT INTO sale_stats_window SELECT {world},toUInt16(1),{item},toUInt8(0),now(),toUInt32(100),quantileTDigestState(0.5)(toUInt32(100)),toUInt64(100),toUInt64(1),toInt64(now()),toUInt64({}),toUInt64(100)",if world==1 { 0 } else { 50 })).execute().await.unwrap();
    }
    assert_eq!(
        listing_history::stock(&ch, &[1], 1).await.unwrap()[&(item, false)],
        Some(0)
    );
    assert_eq!(
        listing_history::stock(&ch, &[1, 2], 1).await.unwrap()[&(item, false)],
        Some(50)
    );
    assert_eq!(
        listing_history::stock(&ch, &[1, 2, 3], 1).await.unwrap()[&(item, false)],
        None
    );
}
