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
    // Use the producer's clock as well as a fixed endpoint for every assertion.
    // A host/ClickHouse clock offset must not move the expired-sale fixture
    // back inside the real refresh query's rolling window.
    let to = ch
        .client()
        .query("SELECT toInt64(now())")
        .fetch_one::<i64>()
        .await
        .unwrap();
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
    // Simulate an acknowledged-late append-only insert: the writer retries the
    // entire batch, so every event is physically present twice. Turnover and
    // one-to-one sale attribution must still describe the original observations.
    ch.client()
        .query(&format!(
            "INSERT INTO listing_events SELECT * FROM listing_events WHERE item_id = {item}"
        ))
        .execute()
        .await
        .unwrap();
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
        listing_history::stock(&ch, &[1, 2], 1, to)
            .await
            .unwrap()
            .is_empty()
    );
    for world in [1, 2] {
        ch.client().query(&format!("INSERT INTO listing_alive SELECT {world},{item},toUInt8(0),toDateTime({to}),toUInt32(1),toUInt64(100),toUInt32(1),toDateTime({to}),quantileTDigestState(0.5)(toUInt32(0)),toUInt32(100)")).execute().await.unwrap();
        ch.client().query(&format!("INSERT INTO sale_stats_window SELECT {world},toUInt16(1),{item},toUInt8(0),toDateTime({to}),toUInt32(100),quantileTDigestState(0.5)(toUInt32(100)),toUInt64(100),toUInt64(1),toInt64({to}-1),toUInt64({}),toUInt64(100)",if world==1 { 0 } else { 50 })).execute().await.unwrap();
    }
    assert_eq!(
        listing_history::stock(&ch, &[1], 1, to).await.unwrap()[&(item, false)],
        Some(0)
    );
    assert_eq!(
        listing_history::stock(&ch, &[1, 2], 1, to).await.unwrap()[&(item, false)],
        Some(50)
    );
    assert_eq!(
        listing_history::stock(&ch, &[1, 2, 3], 1, to)
            .await
            .unwrap()[&(item, false)],
        None
    );

    // Evaluate at a fixed window end so exact cutoffs do not depend on how
    // quickly ClickHouse executes the inserts. Each case has a separate key.
    for (index, (days, cadence)) in [(1, 900), (7, 3600), (30, 21600), (90, 21600)]
        .into_iter()
        .enumerate()
    {
        let first = item + 100 + index as i32 * 20;
        let start = to - i64::from(days) * 86400;
        let cases = [
            (to - cadence, to - 900, start, 50, Some(50)),
            (to - cadence - 1, to, to - 1, 50, None),
            (to, to - 901, to - 1, 50, None),
            (to + 1, to, to - 1, 50, None),
            (to, to + 1, to - 1, 50, None),
            (to, to, start - 1, 50, None),
            (to, to, to, 50, None),
            (to, to, 0, 0, Some(0)),
            (to - cadence - 1, to, 0, 0, None),
        ];
        for (offset, &(sale_time, alive_time, sold_at, units, _)) in cases.iter().enumerate() {
            stock_snapshot(
                &ch,
                first + offset as i32,
                1,
                days,
                (sale_time, sold_at, units),
                alive_time,
            )
            .await;
        }
        let actual = listing_history::stock(&ch, &[1], days, to).await.unwrap();
        for (offset, &(_, _, _, _, expected)) in cases.iter().enumerate() {
            assert_eq!(
                actual[&(first + offset as i32, false)],
                expected,
                "window {days}, boundary case {offset}"
            );
        }
        let mixed = first + 10;
        stock_snapshot(&ch, mixed, 1, days, (to, to - 1, 50), to).await;
        stock_snapshot(&ch, mixed, 2, days, (to - cadence - 1, to - 1, 50), to).await;
        let actual = listing_history::stock(&ch, &[1, 2], days, to)
            .await
            .unwrap();
        assert_eq!(
            actual[&(mixed, false)],
            None,
            "one stale world invalidates the scope"
        );
        assert_eq!(
            actual[&(first, false)],
            None,
            "one missing world invalidates the scope"
        );
    }

    // Exercise the real producer's vanished-group behavior: once its final
    // sale is outside the window, a refresh leaves the older positive row in
    // ReplacingMergeTree. The consumer must not treat it as current stock data.
    let expired = item + 1000;
    stock_snapshot(&ch, expired, 1, 1, (to - 900, to - 86401, 50), to).await;
    ch.client().query(&format!("INSERT INTO sales (pg_id,sold_date,item_id,hq,world_id,price_per_item,quantity,buying_character_id) VALUES (100,{},{expired},0,1,100,50,1)",to-86401)).execute().await.unwrap();
    ultros_clickhouse::rollups::refresh_sale_stats_window(&ch, 1)
        .await
        .unwrap();
    let retained = ch.client().query(&format!("SELECT sum(units_sold) FROM sale_stats_window FINAL WHERE item_id={expired} AND world_id=1 AND window_days=1")).fetch_one::<u64>().await.unwrap();
    assert_eq!(
        retained, 50,
        "the real producer retains the obsolete positive row"
    );
    assert_eq!(
        listing_history::stock(&ch, &[1], 1, to).await.unwrap()[&(expired, false)],
        None
    );
}

async fn stock_snapshot(
    ch: &ClickHouseClient,
    item: i32,
    world: i32,
    days: u16,
    sale: (i64, i64, u64),
    alive_time: i64,
) {
    let (computed_at, sold_at, units) = sale;
    ch.client().query(&format!("INSERT INTO listing_alive SELECT {world},{item},toUInt8(0),toDateTime({alive_time}),toUInt32(1),toUInt64(100),toUInt32(1),toDateTime({alive_time}),quantileTDigestState(0.5)(toUInt32(0)),toUInt32(100)")).execute().await.unwrap();
    ch.client().query(&format!("INSERT INTO sale_stats_window SELECT {world},toUInt16({days}),{item},toUInt8(0),toDateTime({computed_at}),toUInt32(100),quantileTDigestState(0.5)(toUInt32(100)),toUInt64(100),toUInt64(1),toInt64({sold_at}),toUInt64({units}),toUInt64(100)")).execute().await.unwrap();
}

/// A retry is duplicate evidence; two distinct listings are still two events.
#[tokio::test]
async fn event_dedup_preserves_distinct_listing_identities() {
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
    let to = ch
        .client()
        .query("SELECT toInt64(now())")
        .fetch_one::<i64>()
        .await
        .unwrap();
    let item = 1_800_000 + (std::process::id() % 100000) as i32;
    let added = to - 8000;
    let removed = to - 2000;
    // Every compact WindowEvent field is identical between A and B. Only the
    // stored listing identities distinguish these genuinely separate stacks.
    for (listing, pg_id) in [("distinct-a", 1001), ("distinct-b", 1002)] {
        ch.client()
            .query(&format!(
                "INSERT INTO listing_events VALUES
            ({added},'added','websocket',{item},0,1,'{listing}',{pg_id},42,777,2,0,0,{added}),
            ({removed},'removed','websocket',{item},0,1,'{listing}',{pg_id},42,777,2,0,0,{added})"
            ))
            .execute()
            .await
            .unwrap();
    }
    ch.client()
        .query(&format!(
            "INSERT INTO listing_events SELECT * FROM listing_events WHERE item_id={item}"
        ))
        .execute()
        .await
        .unwrap();
    ch.client()
        .query(&format!(
            "INSERT INTO sale_receipts VALUES (1999,{}, {removed},{item},0,1,777,2)",
            removed + 1
        ))
        .execute()
        .await
        .unwrap();
    let rows = listing_history::window(&ch, &[1], 1, to).await.unwrap();
    let stats = &rows[&(item, false)];
    assert_eq!(
        stats.additions, 2,
        "retries must not inflate distinct additions"
    );
    assert_eq!(stats.removals, 2, "distinct stacks must not be collapsed");
    assert_eq!(stats.matches.matched, 0);
    assert_eq!(
        stats.matches.ambiguous, 2,
        "one receipt cannot identify either stack"
    );
}

/// Item partitioning must not partition worlds or average per-world medians.
#[tokio::test]
async fn item_batches_keep_pooled_ages_and_exact_scope_floors() {
    if std::env::var("ULTROS_CH_INTEGRATION").is_err() {
        return;
    }
    let url = std::env::var("CLICKHOUSE_URL").unwrap();
    assert!(url.starts_with("http://127.0.0.1:"));
    assert!(
        std::env::var("CLICKHOUSE_DATABASE")
            .unwrap()
            .starts_with("ultros_t12_")
    );
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.unwrap();
    let to = chrono::Utc::now().timestamp();
    let from = to - 86400;
    let removed = to - 2000;
    let first_item = 1_500_000 + (std::process::id() % 100000) as i32;
    // More than two item batches; each item has ages [10,20] in world 1 and
    // [100] in world 2. The scope median is 20, not the mean of 15 and 100.
    let source = format!(
        "SELECT toInt32({first_item}+intDiv(number,3)) AS item,
        number%3 AS k,toInt32(if(k=2,2,1)) AS world,
        toInt32(800000000+number) AS id,toUInt32(100*(k+1)) AS price,
        toUInt32(multiIf(k=0,10,k=1,20,100)) AS age FROM numbers(810)"
    );
    ch.client().query(&format!("INSERT INTO listing_events SELECT toDateTime({removed}),'removed','websocket',item,toUInt8(0),world,toString(id),id,id,price,toUInt16(2),toUInt32(0),toUInt16(0),toDateTime({removed}-age) FROM ({source})")).execute().await.unwrap();
    ch.client().query(&format!("INSERT INTO sale_receipts SELECT id,toDateTime({removed}+1),toDateTime({removed}),item,toUInt8(0),world,price,toUInt16(2) FROM ({source})")).execute().await.unwrap();
    for (offset, world, price) in [
        (-100, 1, 100),
        (-100, 2, 80),
        (50, 1, 999), // hidden by world 2's cheaper floor
        (90, 1, 100),
        (100, 2, 0),
        (200, 1, 120),
        (300, 1, 0),
    ] {
        ch.client().query(&format!("INSERT INTO floor_changes SELECT toDateTime({from}+({offset})),toInt32({first_item}+number),toUInt8(0),toInt32({world}),toUInt32({price}),'listing' FROM numbers(270)")).execute().await.unwrap();
    }
    let result = listing_history::window(&ch, &[1, 2], 1, to).await.unwrap();
    for item in first_item..first_item + 270 {
        let stats = &result[&(item, false)];
        assert_eq!(stats.removals, 3);
        assert_eq!(stats.matches.matched, 3);
        assert_eq!(stats.matches.median_time_to_sell_secs, Some(20));
        assert_eq!((stats.floor_min, stats.floor_max), (Some(80), Some(120)));
        assert_eq!(stats.floor_empty_secs, 86400 - 300);
        assert_eq!(stats.floor_unknown_secs, 0);
        assert!(!stats.listing_coverage.continuity_verified);
    }
}
