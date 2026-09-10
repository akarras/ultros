//! Opt-in synthetic workload, never a deployed-data probe. See the QA document.
use std::time::{Duration, Instant};

use clickhouse::Row;
use serde::Deserialize;
use ultros_clickhouse::{
    ClickHouseClient, ClickHouseError, ClickHouseErrorKind, listing_history, queries,
};

#[derive(Row, Deserialize)]
struct QueryEvidence {
    query: String,
    query_duration_ms: u64,
    read_rows: u64,
    result_rows: u64,
    memory_usage: u64,
    exception_code: i32,
}

async fn sql(ch: &ClickHouseClient, query: &str) {
    ch.client().query(query).execute().await.unwrap();
}

fn limit_error(error: &ClickHouseError) -> bool {
    if error.kind() == ClickHouseErrorKind::Timeout {
        return true;
    }
    let text = error.to_string();
    [
        "TOO_MANY_ROWS_OR_BYTES",
        "MEMORY_LIMIT_EXCEEDED",
        "TIMEOUT_EXCEEDED",
    ]
    .iter()
    .any(|kind| text.contains(kind))
}

#[tokio::test]
#[ignore = "writes millions of synthetic rows; requires a fresh owned loopback database and heavy-check lock"]
async fn listing_history_scope_workload() {
    let url = std::env::var("CLICKHOUSE_URL").unwrap();
    let database = std::env::var("CLICKHOUSE_DATABASE").unwrap();
    // Exact authority parsing: a misleading userinfo or hostname cannot pass.
    let authority = url.strip_prefix("http://127.0.0.1:").unwrap();
    assert!(authority.parse::<u16>().is_ok(), "loopback port only");
    assert!(database.starts_with("ultros_t12_workload_"));
    assert!(
        database
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    );
    let rows: u64 = std::env::var("T12_ROWS_PER_WORLD")
        .unwrap_or_else(|_| "24000".into())
        .parse()
        .unwrap();
    assert!(
        [24_000, 240_000].contains(&rows),
        "bounded fixture sizes only"
    );
    let ch = ClickHouseClient::from_env();
    let existing_tables = ch
        .client()
        .query("SELECT count() FROM system.tables WHERE database = currentDatabase()")
        .fetch_one::<u64>()
        .await
        .unwrap();
    assert_eq!(
        existing_tables, 0,
        "refuse to migrate a database that already contains tables"
    );
    ch.migrate().await.unwrap();
    for table in [
        "listing_events",
        "sale_receipts",
        "sales",
        "floor_changes",
        "listing_alive",
        "sale_stats_window",
    ] {
        let count = ch
            .client()
            .query(&format!("SELECT count() FROM {table}"))
            .fetch_one::<u64>()
            .await
            .unwrap();
        assert_eq!(count, 0, "fresh disposable database required: {table}");
    }
    let to = ch
        .client()
        .query("SELECT toInt64(now())")
        .fetch_one::<i64>()
        .await
        .unwrap()
        - 120;
    let cycles = rows / 3;
    let total = cycles * 32;
    // Spread triplets through 90 days. 20% share a hot item; the remainder
    // span 1,000 items and both qualities. Retainer identities are per cycle.
    let source = format!(
        "SELECT number + 1 AS id, toInt32(intDiv(number,{cycles})+1) AS world,
        number % {cycles} AS c, toUInt8(intDiv(c,1000)%2) AS quality,
        toInt32(700000 + if(c%5=0,0,c%1000)) AS item,
        toUInt32(100 + c%97) AS price,
        toUInt32({to}-90*86400+3600+intDiv(c*(90*86400-7200),{cycles})) AS t
        FROM numbers({total})"
    );
    let seed_start = Instant::now();
    sql(&ch, &format!("INSERT INTO listing_events SELECT
        toDateTime(t-multiIf(k=0,1200,k=1,700,0)),
        CAST(multiIf(k=0,'added',k=1,'updated','removed'),'Enum8(\\'added\\'=1,\\'updated\\'=2,\\'removed\\'=3)'),
        'websocket',item,quality,world,toString(id),toInt32(id),toInt32(id),price,toUInt16(2),price,toUInt16(2),toDateTime(t-3600)
        FROM ({source}) AS cycles CROSS JOIN (SELECT number AS k FROM numbers(3)) AS kinds")).await;
    sql(&ch, &format!("INSERT INTO sale_receipts SELECT toInt32(id),toDateTime(t+1),toDateTime(t),item,quality,world,price,toUInt16(2) FROM ({source}) WHERE id%10 != 0")).await;
    // A small retry population exercises deduplication without inventing time.
    sql(&ch, &format!("INSERT INTO sale_receipts SELECT toInt32(id),toDateTime(t+1),toDateTime(t),item,quality,world,price,toUInt16(2) FROM ({source}) WHERE id%101=0 AND id%10 != 0")).await;
    sql(&ch, &format!("INSERT INTO sales (pg_id,sold_date,item_id,hq,world_id,price_per_item,quantity,buying_character_id)
        SELECT toInt32(id),toDateTime(t),item,quality,world,price,toUInt16(2),toInt64(id) FROM ({source})")).await;
    sql(&ch, &format!("INSERT INTO floor_changes SELECT toDateTime(t),item,quality,world,if(c%11=0,0,price),'listing' FROM ({source})")).await;
    let boards = "SELECT toInt32(intDiv(number,2000)+1) AS world,toInt32(700000+number%1000) AS item,toUInt8(intDiv(number%2000,1000)) AS quality FROM numbers(64000)";
    sql(&ch, &format!("INSERT INTO floor_changes SELECT toDateTime({to}-91*86400),item,quality,world,toUInt32(100),'resync' FROM ({boards})")).await;
    sql(&ch, &format!("INSERT INTO listing_alive SELECT world,item,quality,toDateTime({to}),toUInt32(50),toUInt64(100),toUInt32(50),toDateTime({to}-3600),quantileTDigestState(0.5)(toUInt32(3600)),toUInt32(100) FROM ({boards}) GROUP BY world,item,quality")).await;
    for days in [30, 90] {
        sql(&ch, &format!("INSERT INTO sale_stats_window SELECT world,toUInt16({days}),item,quality,toDateTime({to}),toUInt32(100),quantileTDigestState(0.5)(toUInt32(100)),toUInt64(100),toUInt64(1),toInt64({to}-1),toUInt64(200),toUInt64(100) FROM ({boards}) GROUP BY world,item,quality")).await;
    }
    println!(
        "FIXTURE database={database} worlds=32 items=1000 qualities=2 events={} receipt_rows={} sales={} floor_rows={} seed_seconds={:.3} to={to}",
        rows * 32,
        ch.client()
            .query("SELECT count() FROM sale_receipts")
            .fetch_one::<u64>()
            .await
            .unwrap(),
        total,
        total + 64000,
        seed_start.elapsed().as_secs_f64()
    );
    for (scope, n) in [("world", 1), ("dc", 8), ("region", 32)] {
        let worlds = (1..=n).collect::<Vec<_>>();
        for days in [30, 90] {
            for repeat in 1..=3 {
                let start_us = chrono::Utc::now().timestamp_micros();
                let start = Instant::now();
                // Match StatsCache's full-load deadline, not just each SQL's
                // ten-second limit. Timing includes local decode and matching.
                let result = tokio::time::timeout(Duration::from_secs(12), async {
                    let alive = queries::bulk_listing_alive(&ch, &worlds).await?;
                    let mut history = listing_history::window(&ch, &worlds, days, to).await?;
                    let stock = listing_history::stock(&ch, &worlds, days, to).await?;
                    for row in &alive {
                        let key = (row.item_id, row.hq != 0);
                        listing_history::set_stock(
                            history.get_mut(&key).unwrap(),
                            row.alive_units,
                            stock.get(&key).copied().flatten(),
                        );
                    }
                    Ok::<_, ClickHouseError>(history)
                })
                .await
                .unwrap_or_else(|_| Err(clickhouse::error::Error::TimedOut.into()));
                let elapsed = start.elapsed().as_secs_f64();
                let rss = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
                println!(
                    "PROCESS scope={scope} days={days} repeat={repeat} cumulative_{}",
                    rss.lines()
                        .find(|line| line.starts_with("VmHWM:"))
                        .unwrap_or("VmHWM: unavailable")
                );
                match result {
                    Ok(history) => {
                        assert!(
                            elapsed < 12.5,
                            "successful history must also meet the cache deadline"
                        );
                        assert_eq!(history.len(), 2000);
                        assert!(history.values().all(|r| r.floor_unknown_secs == 0));
                        assert!(history.values().all(|r| r.days_of_stock.is_some()));
                        let removals: u64 = history.values().map(|r| r.removals).sum();
                        let classified: u64 = history
                            .values()
                            .map(|r| {
                                r.matches.matched
                                    + r.matches.ambiguous
                                    + r.matches.repriced
                                    + r.matches.unmatched
                                    + r.matches.pending
                            })
                            .sum();
                        assert_eq!(removals, classified);
                        let expected = |offset: i64| {
                            (0..cycles)
                                .filter(|c| {
                                    let t = to - 90 * 86400
                                        + 3600
                                        + (c * (90 * 86400 - 7200) / cycles) as i64
                                        - offset;
                                    t >= to - i64::from(days) * 86400
                                })
                                .count() as u64
                                * n as u64
                        };
                        assert_eq!(removals, expected(0));
                        assert_eq!(
                            history.values().map(|r| r.additions).sum::<u64>(),
                            expected(1200)
                        );
                        println!(
                            "CASE rows_per_world={rows} scope={scope} worlds={n} days={days} repeat={repeat} seconds={elapsed:.3} status=ok output_keys={} removals={removals}",
                            history.len()
                        );
                    }
                    Err(error) => {
                        println!(
                            "CASE rows_per_world={rows} scope={scope} worlds={n} days={days} repeat={repeat} seconds={elapsed:.3} status=unavailable error={error}"
                        );
                        assert!(limit_error(&error), "unexpected query failure");
                        assert!(
                            cfg!(debug_assertions) && error.kind() == ClickHouseErrorKind::Timeout,
                            "all supported fixture scopes must complete in the optimized profile"
                        );
                        assert!(rows == 240000 && n >= 8, "small fixtures must succeed");
                        assert!(
                            elapsed < 14.0,
                            "local processing must not starve the 12-second deadline"
                        );
                    }
                }
                // Query logs belong to this disposable server only. No global
                // cache flush, merge, or production introspection is performed.
                sql(&ch, "SYSTEM FLUSH LOGS").await;
                let evidence = ch.client().query(&format!("SELECT query,query_duration_ms,read_rows,result_rows,memory_usage,exception_code
                    FROM system.query_log WHERE current_database = currentDatabase()
                    AND event_time_microseconds >= fromUnixTimestamp64Micro({start_us})
                    AND type IN ('QueryFinish','ExceptionWhileProcessing','ExceptionBeforeStart')
                    AND query_kind='Select' AND NOT has(tables,'system.query_log') ORDER BY event_time_microseconds")).fetch_all::<QueryEvidence>().await.unwrap();
                assert!(!evidence.is_empty(), "enable query_log on the owned server");
                for q in evidence {
                    let stage = if q.query.contains("/* listing_history_items */") {
                        "item_keys"
                    } else if q.query.contains("FROM listing_events") {
                        "events"
                    } else if q.query.contains("FROM floor_changes") {
                        "floors"
                    } else if q.query.contains("FROM sales FINAL") {
                        "missing_receipts"
                    } else if q.query.contains("FROM sale_receipts") {
                        "receipts"
                    } else if q.query.contains("FROM sale_stats_window") {
                        "stock"
                    } else {
                        "alive"
                    };
                    println!(
                        "QUERY scope={scope} days={days} repeat={repeat} stage={stage} milliseconds={} read_rows={} result_rows={} peak_bytes={} exception_code={}",
                        q.query_duration_ms,
                        q.read_rows,
                        q.result_rows,
                        q.memory_usage,
                        q.exception_code
                    );
                }
            }
        }
    }
    if rows == 240000 {
        sql(&ch, &format!("INSERT INTO listing_events SELECT toDateTime({to}-2000),'updated','websocket',toInt32(900000),toUInt8(0),toInt32(1),toString(number),toInt32(number),toInt32(number),toUInt32(100),toUInt16(2),toUInt32(100),toUInt16(2),toDateTime({to}-3600) FROM numbers(2000001)")).await;
        let start = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(12),
            listing_history::window(&ch, &[1], 90, to),
        )
        .await;
        let error = result
            .expect("oversized-item guard must return within the cache deadline")
            .expect_err("a failed later item batch must not return earlier partial history");
        assert!(error.to_string().contains("TOO_MANY_ROWS_OR_BYTES"));
        println!(
            "CAP_GUARD item=900000 rows=2000001 status=unavailable seconds={:.3} error={error}",
            start.elapsed().as_secs_f64()
        );
    }
}
