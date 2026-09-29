//! Real ClickHouse publication and semantic regressions; opt-in disposable DB.
use ultros_clickhouse::{ClickHouseClient, listing_snapshots};

#[tokio::test]
async fn exact_snapshots_reconcile_late_evidence_and_publish_only_complete_generations() {
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
    let a = 910001 + (std::process::id() % 1000) as i32 * 3;
    let b = a + 1;
    let empty = a + 2;
    let item = 1_910_001;
    let from = to - 86400;
    let removal = to - 2000;

    assert!(
        listing_snapshots::read(&ch, &[a, b], 1, to).await.is_err(),
        "unbuilt is not empty"
    );
    listing_snapshots::refresh(&ch, &[empty], 1, to)
        .await
        .unwrap();
    assert!(
        listing_snapshots::read(&ch, &[empty], 1, to)
            .await
            .unwrap()
            .1
            .is_empty(),
        "committed empty is valid"
    );

    ch.client()
        .query(&format!(
            "INSERT INTO listing_events VALUES
        ({},'added','websocket',{item},0,{a},'first',1,11,100,2,0,0,{}),
        ({removal},'removed','websocket',{item},0,{a},'first',1,11,100,2,0,0,{}),
        ({},'removed','websocket',{},0,{a},'frontier',2,12,200,2,0,0,{})",
            removal - 4000,
            removal - 60,
            removal - 60,
            to - 600,
            item + 1,
            removal - 60
        ))
        .execute()
        .await
        .unwrap();
    // Scope is never empty: the worlds take turns being empty. The minimum
    // of each world's empty duration would incorrectly claim half a day.
    ch.client()
        .query(&format!(
            "INSERT INTO floor_changes VALUES
        ({from},{item},0,{a},0,'listing'),({from},{item},0,{b},100,'listing'),
        ({},{item},0,{a},100,'listing'),({},{item},0,{b},0,'listing'),
        ({from},{},0,{a},10,'listing'),({},{},0,{a},100,'listing'),
        ({},{},0,{b},50,'listing'),({from},{},0,{a},80,'listing')",
            from + 43200,
            from + 43200,
            item + 2,
            from + 43200,
            item + 2,
            from + 43200,
            item + 2,
            item + 3
        ))
        .execute()
        .await
        .unwrap();
    // Sales-only keys must be discovered; no stale sale rollup substitutes for
    // the authoritative sale/receipt reconciliation.
    ch.client().query(&format!("INSERT INTO sales (pg_id,sold_date,item_id,hq,world_id,price_per_item,quantity,buying_character_id)
        VALUES (1910001,{removal},{},0,{a},500,2,1)",item+4)).execute().await.unwrap();
    listing_snapshots::refresh(&ch, &[a, b], 1, to)
        .await
        .unwrap();
    let (_, first) = listing_snapshots::read(&ch, &[b, a, a], 1, to)
        .await
        .unwrap();
    assert_eq!(first[&(item, false)].matches.unmatched, 1);
    assert_eq!(
        first[&(item + 1, false)].matches.pending,
        1,
        "exclusive settled frontier"
    );
    assert_eq!(first[&(item, false)].floor_empty_secs, 0);
    assert_eq!(first[&(item, false)].floor_max, Some(100));
    assert_eq!(
        first[&(item + 2, false)].floor_min,
        Some(50),
        "ignore low prices before the common baseline"
    );
    assert_eq!(first[&(item + 2, false)].floor_unknown_secs, 43200);
    assert_eq!(
        first[&(item + 3, false)].floor_unknown_secs,
        86400,
        "missing requested world"
    );
    assert_eq!(first[&(item + 3, false)].floor_min, None);
    assert_eq!(first[&(item + 4, false)].matches.sales_without_receipt, 1);
    // A receipt after this generation's observation endpoint is not evidence
    // available to that generation, even when its game sale time is earlier.
    ch.client()
        .query(&format!(
            "INSERT INTO sale_receipts VALUES
        (1910001,{}, {removal},{},0,{a},500,2)",
            to + 2000,
            item + 4
        ))
        .execute()
        .await
        .unwrap();

    // Evidence written after a previous generation must repair earlier outcomes.
    ch.client()
        .query(&format!(
            "INSERT INTO sale_receipts VALUES
        (1910002,{}, {removal},{item},0,{a},100,2)",
            removal + 1
        ))
        .execute()
        .await
        .unwrap();
    ch.client().query(&format!("INSERT INTO listing_events SELECT * FROM listing_events WHERE item_id={item} AND world_id={a}"))
        .execute().await.unwrap();
    listing_snapshots::refresh(&ch, &[a, b], 1, to + 1)
        .await
        .unwrap();
    let (_, second) = listing_snapshots::read(&ch, &[a, b], 1, to + 1)
        .await
        .unwrap();
    assert_eq!(second[&(item, false)].additions, 1);
    assert_eq!(second[&(item, false)].removals, 1);
    assert_eq!(second[&(item, false)].matches.matched, 1);
    assert_eq!(second[&(item + 4, false)].matches.sales_without_receipt, 1);
    assert_eq!(
        second[&(item, false)].matches.median_time_to_sell_secs,
        Some(60)
    );
    ch.client()
        .query(&format!(
            "INSERT INTO listing_events VALUES
        ({},'removed','websocket',{item},0,{a},'late-competing',3,13,100,2,0,0,{})",
            removal + 1,
            removal - 60
        ))
        .execute()
        .await
        .unwrap();
    listing_snapshots::refresh(&ch, &[a, b], 1, to + 2)
        .await
        .unwrap();
    let (_, third) = listing_snapshots::read(&ch, &[a, b], 1, to + 2)
        .await
        .unwrap();
    let stats = third[&(item, false)];
    assert_eq!(
        stats.matches.matched, 0,
        "a late competitor revokes the old match"
    );
    assert_eq!(stats.matches.ambiguous, 2);
    assert_eq!(
        stats.removals,
        stats.matches.matched
            + stats.matches.ambiguous
            + stats.matches.repriced
            + stats.matches.unmatched
            + stats.matches.pending
    );

    // A partially written candidate has no manifest and must not shadow the old
    // committed generation. A corrupt/incomplete manifest must fail closed.
    let scope = format!("{a},{b}");
    let orphan = "00000000-0000-0000-0000-000000000141";
    ch.client()
        .query("INSERT INTO listing_window_snapshot_rows VALUES (?,1,?,999,0,'{}',now())")
        .bind(&scope)
        .bind(orphan)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        listing_snapshots::read(&ch, &[a, b], 1, to + 2)
            .await
            .unwrap()
            .1,
        third
    );
    ch.client()
        .query("INSERT INTO listing_window_snapshot_manifests VALUES (?,1,?,?,2,now())")
        .bind(&scope)
        .bind(orphan)
        .bind(to + 3)
        .execute()
        .await
        .unwrap();
    assert!(
        listing_snapshots::read(&ch, &[a, b], 1, to + 3)
            .await
            .is_err()
    );
    listing_snapshots::refresh(&ch, &[a, b], 1, to + 4)
        .await
        .unwrap();
    assert!(
        listing_snapshots::read(&ch, &[a, b], 1, to + 4)
            .await
            .is_ok()
    );
    assert!(
        listing_snapshots::read(&ch, &[a, b], 1, to + 3)
            .await
            .is_err(),
        "future manifest"
    );
    assert!(
        listing_snapshots::read(&ch, &[a, b], 1, to + 905)
            .await
            .is_err(),
        "expired manifest"
    );
    listing_snapshots::refresh(&ch, &[a, b], 1, to + 2001)
        .await
        .unwrap();
    let (_, after_receipt) = listing_snapshots::read(&ch, &[a, b], 1, to + 2001)
        .await
        .unwrap();
    assert_eq!(
        after_receipt[&(item + 4, false)]
            .matches
            .sales_without_receipt,
        0
    );

    // Old event keys disappear without per-item tombstones; floor baselines
    // persist. The later receipt remains in this observation window even
    // though its game sale time has expired.
    listing_snapshots::refresh(&ch, &[a, b], 1, to + 86410)
        .await
        .unwrap();
    let (_, expired) = listing_snapshots::read(&ch, &[a, b], 1, to + 86410)
        .await
        .unwrap();
    assert!(!expired.contains_key(&(item + 1, false)));
    assert_eq!(expired[&(item + 4, false)].matches.received_sales, 1);
    assert_eq!(expired[&(item + 4, false)].matches.sales_without_receipt, 0);

    // Advance beyond the later receipt and its 600-second matching context.
    listing_snapshots::refresh(&ch, &[a, b], 1, to + 90001)
        .await
        .unwrap();
    let (_, expired) = listing_snapshots::read(&ch, &[a, b], 1, to + 90001)
        .await
        .unwrap();
    assert!(
        !expired.contains_key(&(item + 4, false)),
        "expired sale-only key must vanish"
    );
}
