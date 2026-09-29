//! Round-trips the listing-history tables against a live ClickHouse.
//!
//! Run with:
//!   ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test listing_events_smoke -- --nocapture

use chrono::Utc;
use ultros_clickhouse::{
    ClickHouseClient,
    listing_seed::{already_seeded, mark_seeded},
    rows::{
        FloorChangeReason, FloorChangeRow, ListingEventKind, ListingEventRow, ListingEventSource,
    },
    schema::LISTING_EVENTS_SEED_MARKER_TABLE,
};

fn integration_enabled() -> bool {
    std::env::var("ULTROS_CH_INTEGRATION").is_ok()
}

async fn client() -> ClickHouseClient {
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    // Applying twice must be a no-op.
    ch.migrate().await.expect("migrate again");
    ch
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct Count {
    n: u64,
}

async fn count(ch: &ClickHouseClient, table: &str, item_id: i32) -> u64 {
    let c: Count = ch
        .client()
        .query(&format!(
            "SELECT count() AS n FROM {table} WHERE item_id = ?"
        ))
        .bind(item_id)
        .fetch_one()
        .await
        .expect("count");
    c.n
}

async fn cleanup(ch: &ClickHouseClient, table: &str, item_id: i32) {
    ch.client()
        .query(&format!(
            "ALTER TABLE {table} DELETE WHERE item_id = ? SETTINGS mutations_sync = 1"
        ))
        .bind(item_id)
        .execute()
        .await
        .expect("cleanup");
}

#[tokio::test]
async fn listing_events_insert_and_read_round_trip() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    let ch = client().await;
    let item_id = -616161;
    cleanup(&ch, "listing_events", item_id).await;

    let now = Utc::now();
    let row = ListingEventRow {
        event_time: now,
        kind: ListingEventKind::Updated,
        source: ListingEventSource::Websocket,
        item_id,
        hq: 1,
        world_id: 40,
        listing_id: "smoke-1".to_string(),
        pg_listing_id: -1,
        retainer_id: -1,
        price_per_unit: 450,
        quantity: 2,
        prev_price: 500,
        prev_quantity: 3,
        reviewed_at: now,
    };
    let mut insert = ch
        .client()
        .insert::<ListingEventRow>("listing_events")
        .await
        .expect("insert");
    insert.write(&row).await.expect("write");
    insert.end().await.expect("end");

    let back: ListingEventRow = ch
        .client()
        .query("SELECT ?fields FROM listing_events WHERE item_id = ?")
        .bind(item_id)
        .fetch_one()
        .await
        .expect("read back");
    assert_eq!(back.kind, ListingEventKind::Updated);
    assert_eq!(back.source, ListingEventSource::Websocket);
    assert_eq!(back.prev_price, 500);
    assert_eq!(back.listing_id, "smoke-1");
    assert_eq!(count(&ch, "listing_events", item_id).await, 1);
}

#[tokio::test]
async fn floor_changes_insert_and_read_round_trip() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    let ch = client().await;
    let item_id = -616162;
    cleanup(&ch, "floor_changes", item_id).await;

    let row = FloorChangeRow::new(Utc::now(), item_id, false, 40, 0, FloorChangeReason::Refill);
    let mut insert = ch
        .client()
        .insert::<FloorChangeRow>("floor_changes")
        .await
        .expect("insert");
    insert.write(&row).await.expect("write");
    insert.end().await.expect("end");

    let back: FloorChangeRow = ch
        .client()
        .query("SELECT ?fields FROM floor_changes WHERE item_id = ?")
        .bind(item_id)
        .fetch_one()
        .await
        .expect("read back");
    assert_eq!(back.reason, FloorChangeReason::Refill);
    assert_eq!(back.price_per_unit, 0);
}

#[tokio::test]
async fn seed_marker_round_trip() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    let ch = client().await;
    ch.client()
        .query(&format!(
            "TRUNCATE TABLE {LISTING_EVENTS_SEED_MARKER_TABLE}"
        ))
        .execute()
        .await
        .expect("truncate marker");

    assert!(!already_seeded(&ch).await.expect("query"));
    mark_seeded(&ch, 42).await.expect("mark");
    assert!(already_seeded(&ch).await.expect("query"));
    // Leave the marker cleared so a dev box's leader still runs the real seed.
    ch.client()
        .query(&format!(
            "TRUNCATE TABLE {LISTING_EVENTS_SEED_MARKER_TABLE}"
        ))
        .execute()
        .await
        .expect("truncate marker");
}

/// The real seed path against a live Postgres: streams every `active_listing`
/// row, marks the seed, and is a no-op the second time. Needs `DATABASE_URL`
/// on top of the ClickHouse env; cleans up the rows and marker it created.
#[tokio::test]
async fn seed_streams_the_whole_board_once() {
    if !integration_enabled() || std::env::var("DATABASE_URL").is_err() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 and DATABASE_URL to run");
        return;
    }
    use sea_orm::{ConnectOptions, Database, EntityTrait, PaginatorTrait};
    use ultros_clickhouse::listing_seed::{SeedOutcome, seed_listing_events};

    let ch = client().await;
    // Read-only against whatever Postgres is at DATABASE_URL: no migrations,
    // so a shared dev database that a newer branch has migrated still works.
    let mut opt = ConnectOptions::new(std::env::var("DATABASE_URL").unwrap());
    opt.max_connections(4).min_connections(1);
    let pg = ultros_db::UltrosDb::from_connection(Database::connect(opt).await.expect("postgres"));
    let expected = ultros_db::entity::active_listing::Entity::find()
        .count(pg.get_connection())
        .await
        .expect("count active_listing");

    ch.client()
        .query(&format!(
            "TRUNCATE TABLE {LISTING_EVENTS_SEED_MARKER_TABLE}"
        ))
        .execute()
        .await
        .expect("truncate marker");

    let first = seed_listing_events(&ch, &pg).await.expect("seed");
    assert_eq!(first, SeedOutcome::Seeded { rows: expected });

    let snapshot_rows: Count = ch
        .client()
        .query("SELECT count() AS n FROM listing_events WHERE source = 'snapshot'")
        .fetch_one()
        .await
        .expect("count snapshot rows");
    assert_eq!(snapshot_rows.n, expected);

    let second = seed_listing_events(&ch, &pg).await.expect("seed again");
    assert_eq!(second, SeedOutcome::AlreadySeeded);

    // Cleanup: drop what this test wrote so the dev box is as it was.
    ch.client()
        .query(
            "ALTER TABLE listing_events DELETE WHERE source = 'snapshot' SETTINGS mutations_sync = 1",
        )
        .execute()
        .await
        .expect("delete snapshot rows");
    ch.client()
        .query(&format!(
            "TRUNCATE TABLE {LISTING_EVENTS_SEED_MARKER_TABLE}"
        ))
        .execute()
        .await
        .expect("truncate marker");
}
