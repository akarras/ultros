//! Integration test for the vendor-anchored noise filter (Phase 4).
//!
//! Verifies:
//! 1. `refresh_vendor_prices` populates the lookup table from xiv-gen.
//! 2. A synthetic launder fixture (quantity=1, sale 1000× vendor price) is
//!    flagged by Layer 2's vendor-anchored rule even when the item's own
//!    sale history would otherwise look "normal" (no relative-price
//!    outlier signal).
//!
//! Run with:
//!   ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test vendor_filter_smoke -- --nocapture

use ultros_clickhouse::{ClickHouseClient, rollups};

fn integration_enabled() -> bool {
    std::env::var("ULTROS_CH_INTEGRATION").is_ok()
}

fn load_env() {
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();
}

#[tokio::test]
async fn refresh_vendor_prices_populates_lookup() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();

    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");

    let n = rollups::refresh_vendor_prices(&ch)
        .await
        .expect("refresh vendor");
    eprintln!("  vendor-price rows loaded: {n}");
    assert!(
        n > 1000,
        "expected several thousand vendor-priced items, got {n}"
    );

    // Spot-check item 4422: a real launder-prone item with a low NPC
    // vendor price. The test doesn't hard-code the price (game patches
    // change them) but does assert it's < 10,000 — the launders on this
    // item routinely show 7-figure sale prices, far above any reasonable
    // multiplier of vendor.
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct VendorLookup {
        vendor_price: u32,
    }
    let row: Option<VendorLookup> = ch
        .client()
        .query("SELECT vendor_price FROM item_vendor_price FINAL WHERE item_id = ?")
        .bind(4422i32)
        .fetch_optional()
        .await
        .expect("lookup item 4422");
    match row {
        Some(r) => {
            eprintln!("  item 4422 vendor_price = {}", r.vendor_price);
            assert!(
                r.vendor_price > 0 && r.vendor_price < 10_000,
                "item 4422 should have a small positive vendor price; got {}",
                r.vendor_price
            );
        }
        None => panic!("item 4422 has no vendor price; xiv-gen schema may have shifted"),
    }
}

#[tokio::test]
async fn vendor_anchored_rule_catches_high_multiplier_launder() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();

    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");

    // Use the real item 4422 — vendor price is small (typically 170 gil).
    // Insert 30 "legitimate" sales at 10x vendor (~1700 gil) plus 3 launder
    // rows at 1000x vendor (~170k gil). Pre-Phase-4 the launders wouldn't
    // be flagged by relative-price checks since the *cleaned* p50 would
    // still be ~1700 and the launders are only 100x that — under the
    // existing 10x p50 threshold? Wait, 100x > 10x p50, so the existing
    // L2 catches them. Better test: launders that fall *within* the
    // existing p50 thresholds but are still suspicious vs vendor.
    //
    // Construct: legitimate sales at 50x vendor (~8500 gil), launders at
    // 200x vendor (~34k gil). 34k / 8.5k = 4x p50 — under the existing
    // 10x p50 threshold. Vendor rule (200x > 100x threshold) catches them
    // where the relative rule wouldn't.
    let item_id = 4422;
    let world_id = -777_777; // sentinel world so we don't pollute real rollups

    ch.client()
        .query(
            "ALTER TABLE sales DELETE WHERE item_id = ? AND world_id = ? \
             SETTINGS mutations_sync = 1",
        )
        .bind(item_id)
        .bind(world_id)
        .execute()
        .await
        .expect("cleanup");

    // Look up the real vendor price so the multipliers below are correct.
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct V {
        vendor_price: u32,
    }
    rollups::refresh_vendor_prices(&ch).await.expect("vendor");
    let v: V = ch
        .client()
        .query("SELECT vendor_price FROM item_vendor_price FINAL WHERE item_id = ?")
        .bind(item_id)
        .fetch_one()
        .await
        .expect("lookup");
    let legit_price = v.vendor_price * 50;
    let launder_price = v.vendor_price * 200;
    eprintln!(
        "  vendor={} legit_price={} launder_price={} ratio_to_p50={:.1}x",
        v.vendor_price,
        legit_price,
        launder_price,
        launder_price as f32 / legit_price as f32,
    );

    let mut insert = ch
        .client()
        .insert::<ultros_clickhouse::rows::SaleRow>("sales")
        .await
        .expect("insert");
    let now = chrono::Utc::now();
    for i in 0..30 {
        let row = ultros_clickhouse::rows::SaleRow {
            pg_id: -1000 - i,
            sold_date: now - chrono::Duration::hours(i as i64),
            item_id,
            hq: 0,
            world_id,
            price_per_item: legit_price,
            quantity: 1,
            buying_character_id: 10 + i as i64,
            buyer_name: format!("legit_{i}"),
        };
        insert.write(&row).await.expect("write");
    }
    for i in 0..3 {
        let row = ultros_clickhouse::rows::SaleRow {
            pg_id: -2000 - i,
            sold_date: now - chrono::Duration::hours(i as i64),
            item_id,
            hq: 0,
            world_id,
            price_per_item: launder_price,
            quantity: 1,
            buying_character_id: 9000 + i as i64,
            buyer_name: format!("launder_{i}"),
        };
        insert.write(&row).await.expect("write");
    }
    insert.end().await.expect("end");

    rollups::refresh_window(&ch, 30).await.expect("refresh");

    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Stat {
        sample_size: u32,
        cleaned_sample_size: u32,
        excluded_count: u32,
        p50: u32,
    }
    let stat: Stat = ch
        .client()
        .query(
            "SELECT sample_size, cleaned_sample_size, excluded_count, p50 \
             FROM item_stats_window FINAL \
             WHERE item_id = ? AND world_id = ? AND window_days = 30",
        )
        .bind(item_id)
        .bind(world_id)
        .fetch_one()
        .await
        .expect("stat");

    eprintln!(
        "  sample={} clean={} excluded={} p50={}",
        stat.sample_size, stat.cleaned_sample_size, stat.excluded_count, stat.p50
    );
    assert_eq!(stat.sample_size, 33, "expected all 33 rows pre-filter");
    assert_eq!(
        stat.excluded_count,
        3,
        "vendor-anchored rule should flag all 3 launder rows even though \
         launder_price / legit_price = {:.1}x is under the 10x p50 threshold",
        launder_price as f32 / legit_price as f32
    );
    assert_eq!(stat.cleaned_sample_size, 30);
    assert_eq!(
        stat.p50, legit_price,
        "cleaned p50 should equal legit price"
    );

    // Cleanup.
    ch.client()
        .query(
            "ALTER TABLE sales DELETE WHERE item_id = ? AND world_id = ? \
             SETTINGS mutations_sync = 1",
        )
        .bind(item_id)
        .bind(world_id)
        .execute()
        .await
        .expect("post-cleanup");
}
