//! Replays a scripted `listing_events` fixture through the `listing_alive`
//! rollup and checks every field the listing-stats endpoint serves.
//!
//! Run against a throwaway ClickHouse — a container, or a scratch database on
//! a dev server (the fixture resets the refresher's cursor, so the run folds
//! that database's whole `listing_events` table from the seed):
//!   docker run --rm -d -p 8123:8123 -e CLICKHOUSE_DB=ultros \
//!     -e CLICKHOUSE_USER=ultros -e CLICKHOUSE_PASSWORD= \
//!     --name ch-test clickhouse/clickhouse-server
//!   ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test listing_alive_smoke
//!
//! Fixture ids — two dedicated world ids and nine dedicated item ids — are
//! distinct from every sibling smoke test's, and [`cleanup`] deletes by them at
//! both ends of the run. Nothing may be left behind: cargo shares one server
//! across the test files, and `listing_events_smoke`'s
//! `seed_streams_the_whole_board_once` asserts an *exact* count of
//! `source = 'snapshot'` rows, which a leftover fixture row would break with a
//! failure that has nothing to do with either test's subject.

use chrono::{DateTime, TimeDelta, Utc};
use ultros_clickhouse::{
    ClickHouseClient, queries, rollups,
    rows::{ListingEventKind, ListingEventRow, ListingEventSource},
    schema::{
        LISTING_ALIVE_STATE_TABLE, LISTING_EVENTS_SEED_MARKER_TABLE, LISTING_LAST_EVENT_TABLE,
    },
    writer::insert_all,
};

fn integration_enabled() -> bool {
    std::env::var("ULTROS_CH_INTEGRATION").is_ok()
}

fn load_env() {
    let _ = dotenvy::from_filename("../.env");
    let _ = dotenvy::dotenv();
}

const WORLD: i32 = 999_002;
const OTHER_WORLD: i32 = 999_003;

/// (a) added then removed: not alive, stored as a zero row.
const ITEM_ADD_REMOVE: i32 = -616_170;
/// (b) a reprice — `removed` + `added` on one `listing_id` in one second —
/// counts once, at the new price.
const ITEM_REPRICE: i32 = -616_171;
/// (c) an `updated` event: alive with the post-state price.
const ITEM_UPDATED: i32 = -616_172;
/// (d) legacy rows with `listing_id = ''` keep their own identities.
const ITEM_LEGACY: i32 = -616_173;
/// (e) two retainers on one item, NQ and HQ as separate keys.
const ITEM_TWO_RETAINERS: i32 = -616_174;
/// `updated` and `removed` on one Postgres row in one second: removed wins.
const ITEM_SAME_ROW_TIE: i32 = -616_175;
/// An `added` from before the seed with no removal on record: not replayed.
const ITEM_PRE_SEED: i32 = -616_176;
/// A snapshot row removed while the seed was still streaming: not alive.
const ITEM_SEED_STREAM: i32 = -616_177;
/// Spread over two worlds to check the datacenter merge.
const ITEM_MERGE: i32 = -616_178;

const ALL_ITEMS: [i32; 9] = [
    ITEM_ADD_REMOVE,
    ITEM_REPRICE,
    ITEM_UPDATED,
    ITEM_LEGACY,
    ITEM_TWO_RETAINERS,
    ITEM_SAME_ROW_TIE,
    ITEM_PRE_SEED,
    ITEM_SEED_STREAM,
    ITEM_MERGE,
];

const RETAINER_A: i32 = -7001;
const RETAINER_B: i32 = -7002;
const RETAINER_C: i32 = -7003;

#[derive(clickhouse::Row, serde::Deserialize)]
struct Count {
    n: u64,
}

fn in_list() -> String {
    ALL_ITEMS
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Delete every trace of the fixture. Called before the run *and* after it:
/// the `source = 'snapshot'` rows this fixture needs (see [`fixture`]) are
/// otherwise indistinguishable from a real seed's to a sibling test that counts
/// them.
///
/// The `_listing_alive_state` cursor goes too. Before the run it forces the
/// refresher to fold from the seed cutoff rather than from wherever this
/// database left off, which is what lets the pre-seed and mid-stream cases mean
/// anything; after it, a truncated cursor only ever costs the next real refresh
/// one extra full fold, which is idempotent.
async fn cleanup(ch: &ClickHouseClient) {
    for table in ["listing_events", "listing_alive", LISTING_LAST_EVENT_TABLE] {
        ch.client()
            .query(&format!(
                "ALTER TABLE {table} DELETE WHERE item_id IN ({}) SETTINGS mutations_sync = 1",
                in_list()
            ))
            .execute()
            .await
            .expect("cleanup");
    }
    ch.client()
        .query(&format!("TRUNCATE TABLE {LISTING_ALIVE_STATE_TABLE}"))
        .execute()
        .await
        .expect("truncate alive cursor");
}

/// A websocket `added` at `at`, reviewed at `at`, on `WORLD`, NQ. Callers
/// override what the case needs.
fn added(
    item_id: i32,
    listing_id: &str,
    pg_listing_id: i32,
    retainer_id: i32,
    price: u32,
    quantity: u16,
    at: DateTime<Utc>,
) -> ListingEventRow {
    ListingEventRow {
        event_time: at,
        kind: ListingEventKind::Added,
        source: ListingEventSource::Websocket,
        item_id,
        hq: 0,
        world_id: WORLD,
        listing_id: listing_id.to_string(),
        pg_listing_id,
        retainer_id,
        price_per_unit: price,
        quantity,
        prev_price: 0,
        prev_quantity: 0,
        reviewed_at: at,
    }
}

/// A removal of `row` at `at`. Always `websocket`-sourced, whatever the row it
/// copies: only the one-time seed ever writes `snapshot`, and a removal tagged
/// that way would inflate the snapshot row count a sibling test asserts on.
fn removed(row: &ListingEventRow, at: DateTime<Utc>) -> ListingEventRow {
    ListingEventRow {
        event_time: at,
        kind: ListingEventKind::Removed,
        source: ListingEventSource::Websocket,
        ..row.clone()
    }
}

/// What the refresher will use as its cutoff on this database, seeding a
/// marker when it has none. Returns the cutoff and whether the marker was
/// ours to remove afterwards.
async fn seed_cutoff(ch: &ClickHouseClient, now: DateTime<Utc>) -> (DateTime<Utc>, bool) {
    // Plain aggregates, one table per query: a scalar subquery in a SELECT
    // list comes back `Nullable`, which the row decoder refuses for these.
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Stamp {
        n: u64,
        #[serde(with = "clickhouse::serde::chrono::datetime")]
        at: DateTime<Utc>,
    }
    let snapshot: Stamp = ch
        .client()
        .query(
            "SELECT count() AS n, min(event_time) AS at \
             FROM listing_events WHERE source = 'snapshot'",
        )
        .fetch_one()
        .await
        .expect("read snapshot rows");
    if snapshot.n > 0 {
        return (snapshot.at, false);
    }
    let marker: Stamp = ch
        .client()
        .query(&format!(
            "SELECT count() AS n, max(seeded_at) AS at FROM {LISTING_EVENTS_SEED_MARKER_TABLE}"
        ))
        .fetch_one()
        .await
        .expect("read seed marker");
    if marker.n > 0 {
        return (marker.at, false);
    }
    // Never seeded: plant a marker so the pre-seed and mid-stream cases have
    // somewhere to land, and take it out again at the end so a dev box's
    // leader still runs the real seed. As in production the marker lands
    // *after* the snapshot rows (here by 150 s): a refresher that cut off at
    // the marker instead of the snapshot time would miss the mid-stream
    // removal, and this gap is what lets the fixture catch that.
    let snapshot_at = now - TimeDelta::seconds(450);
    let marker_at = now - TimeDelta::seconds(300);
    ch.client()
        .query(&format!(
            "INSERT INTO {LISTING_EVENTS_SEED_MARKER_TABLE} (seeded_at, rows_streamed) \
             VALUES (toDateTime({}), 0)",
            marker_at.timestamp()
        ))
        .execute()
        .await
        .expect("plant marker");
    (snapshot_at, true)
}

async fn clear_marker(ch: &ClickHouseClient) {
    ch.client()
        .query(&format!(
            "TRUNCATE TABLE {LISTING_EVENTS_SEED_MARKER_TABLE}"
        ))
        .execute()
        .await
        .expect("truncate marker");
}

fn fixture(now: DateTime<Utc>, cutoff: DateTime<Utc>) -> Vec<ListingEventRow> {
    let t0 = now - TimeDelta::seconds(120);
    let t1 = now - TimeDelta::seconds(60);
    let mut rows = Vec::new();

    // (a) added, then removed.
    let a1 = added(ITEM_ADD_REMOVE, "a1", 1001, RETAINER_A, 100, 1, t0);
    rows.push(removed(&a1, t1));
    rows.push(a1);

    // (b) reprice: remove(old state, old row) + add(new state, new row) in
    // the same second. Inserted with the `added` first so the tie-break, not
    // insertion order, decides.
    let b1 = added(ITEM_REPRICE, "b1", 1002, RETAINER_A, 500, 2, t0);
    rows.push(added(ITEM_REPRICE, "b1", 1003, RETAINER_A, 450, 2, t1));
    rows.push(removed(&b1, t1));
    rows.push(b1);

    // (c) updated: the post-state price wins, and the retainer's touch moves
    // reviewed_at.
    let c1 = added(ITEM_UPDATED, "c1", 1005, RETAINER_A, 300, 1, t0);
    rows.push(ListingEventRow {
        event_time: t1,
        kind: ListingEventKind::Updated,
        price_per_unit: 280,
        prev_price: 300,
        prev_quantity: 1,
        reviewed_at: t1,
        ..c1.clone()
    });
    rows.push(c1);

    // (d) legacy rows: no listing_id, identity from the Postgres row id. Two
    // stay, one is removed by the legacy content diff.
    rows.push(added(ITEM_LEGACY, "", 2001, RETAINER_A, 100, 5, t0));
    rows.push(added(ITEM_LEGACY, "", 2002, RETAINER_B, 120, 1, t0));
    let d3 = added(ITEM_LEGACY, "", 2003, RETAINER_A, 90, 1, t0);
    rows.push(removed(&d3, t1));
    rows.push(d3);

    // (e) two retainers, three NQ listings with distinct ages, one HQ.
    let mut e1 = added(ITEM_TWO_RETAINERS, "e1", 3001, RETAINER_A, 1000, 1, t0);
    e1.reviewed_at = now - TimeDelta::seconds(3600);
    let mut e2 = added(ITEM_TWO_RETAINERS, "e2", 3002, RETAINER_B, 900, 3, t0);
    e2.reviewed_at = now - TimeDelta::seconds(600);
    let mut e3 = added(ITEM_TWO_RETAINERS, "e3", 3003, RETAINER_A, 950, 1, t0);
    e3.reviewed_at = now - TimeDelta::seconds(1800);
    let mut e4 = added(ITEM_TWO_RETAINERS, "e4", 3004, RETAINER_B, 5000, 1, t0);
    e4.hq = 1;
    rows.extend([e1, e2, e3, e4]);

    // Same row, same second: `updated` then `removed`. The row is gone.
    let s1 = added(ITEM_SAME_ROW_TIE, "s1", 1004, RETAINER_A, 700, 1, t0);
    rows.push(ListingEventRow {
        event_time: t1,
        kind: ListingEventKind::Updated,
        price_per_unit: 650,
        prev_price: 700,
        prev_quantity: 1,
        ..s1.clone()
    });
    rows.push(ListingEventRow {
        event_time: t1,
        kind: ListingEventKind::Removed,
        price_per_unit: 650,
        ..s1.clone()
    });
    rows.push(s1);

    // Before the seed: an `added` whose removal predates recording. Must not
    // be replayed.
    rows.push(added(
        ITEM_PRE_SEED,
        "p1",
        4001,
        RETAINER_A,
        10,
        1,
        cutoff - TimeDelta::seconds(3600),
    ));

    // Seed stream: two snapshot rows at the snapshot time; one is removed
    // five seconds later, while the marker is still unwritten.
    let mut g1 = added(ITEM_SEED_STREAM, "g1", 4002, RETAINER_A, 150, 1, cutoff);
    g1.source = ListingEventSource::Snapshot;
    rows.push(removed(&g1, cutoff + TimeDelta::seconds(5)));
    rows.push(g1);
    let mut g2 = added(ITEM_SEED_STREAM, "g2", 4003, RETAINER_B, 200, 2, cutoff);
    g2.source = ListingEventSource::Snapshot;
    rows.push(g2);

    // Across two worlds.
    let mut f1 = added(ITEM_MERGE, "f1", 5001, RETAINER_A, 100, 1, t0);
    f1.reviewed_at = now - TimeDelta::seconds(1000);
    let mut f2 = added(ITEM_MERGE, "f2", 5002, RETAINER_B, 300, 2, t0);
    f2.reviewed_at = now - TimeDelta::seconds(3000);
    // Odd counts on each side and merged (3 and 5): ClickHouse's t-digest
    // returns the lower middle of an even-count set, which would make the
    // expected median depend on the approximation rather than the merge.
    let mut f3 = added(ITEM_MERGE, "f3", 5003, RETAINER_C, 200, 4, t0);
    f3.world_id = OTHER_WORLD;
    f3.reviewed_at = now - TimeDelta::seconds(1500);
    let mut f4 = added(ITEM_MERGE, "f4", 5004, RETAINER_A, 250, 1, t0);
    f4.reviewed_at = now - TimeDelta::seconds(2000);
    let mut f5 = added(ITEM_MERGE, "f5", 5005, RETAINER_C, 220, 1, t0);
    f5.world_id = OTHER_WORLD;
    f5.reviewed_at = now - TimeDelta::seconds(2500);
    rows.extend([f1, f2, f3, f4, f5]);

    rows
}

async fn stored_alive_count(ch: &ClickHouseClient, item_id: i32) -> Option<u32> {
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Stored {
        alive_count: u32,
    }
    let rows: Vec<Stored> = ch
        .client()
        .query("SELECT alive_count FROM listing_alive FINAL WHERE world_id = ? AND item_id = ? AND hq = 0")
        .bind(WORLD)
        .bind(item_id)
        .fetch_all()
        .await
        .expect("read stored row");
    assert!(rows.len() <= 1, "one row per key after FINAL");
    rows.first().map(|r| r.alive_count)
}

#[tokio::test]
async fn alive_set_matches_the_scripted_fixture() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    load_env();
    let ch = ClickHouseClient::from_env();
    ch.migrate().await.expect("migrate");
    cleanup(&ch).await;

    // ClickHouse DateTime is whole seconds; keep every fixture time exact.
    let now = DateTime::from_timestamp(Utc::now().timestamp(), 0).unwrap();
    let (cutoff, marker_is_ours) = seed_cutoff(&ch, now).await;
    let rows = fixture(now, cutoff);
    insert_all(&ch, &rows, 100).await.expect("insert fixture");
    let inserted: Count = ch
        .client()
        .query(&format!(
            "SELECT count() AS n FROM listing_events WHERE item_id IN ({})",
            in_list()
        ))
        .fetch_one()
        .await
        .expect("count fixture");
    assert_eq!(inserted.n, rows.len() as u64);

    let refreshed = rollups::refresh_listing_alive(&ch).await;
    if marker_is_ours {
        clear_marker(&ch).await;
    }
    refreshed.expect("refresh listing_alive");

    let by_key = |rows: &[queries::BulkListingAliveRow], item: i32, hq: u8| {
        rows.iter()
            .find(|r| r.item_id == item && r.hq == hq)
            .cloned()
    };
    let world = queries::bulk_listing_alive(&ch, &[WORLD])
        .await
        .expect("bulk_listing_alive");
    // Seconds between the fixture's `now` and the read, for the age checks.
    let drift = Utc::now().timestamp() - now.timestamp();
    let age_matches = |actual: u32, expected: i64| {
        let actual = i64::from(actual);
        (actual - expected - drift).abs() <= 5
    };
    let dc = queries::bulk_listing_alive(&ch, &[WORLD, OTHER_WORLD])
        .await
        .expect("bulk_listing_alive across worlds");
    let elsewhere = queries::bulk_listing_alive(&ch, &[OTHER_WORLD])
        .await
        .expect("bulk_listing_alive, other world");
    let computed_at = queries::listing_alive_computed_at(&ch, &[WORLD])
        .await
        .expect("listing_alive_computed_at");
    let stored_add_remove = stored_alive_count(&ch, ITEM_ADD_REMOVE).await;
    let stored_same_row_tie = stored_alive_count(&ch, ITEM_SAME_ROW_TIE).await;
    let stored_pre_seed = stored_alive_count(&ch, ITEM_PRE_SEED).await;

    // Every read is done; take the fixture back out before asserting, so a
    // failure here cannot leave rows behind for a sibling test to trip over.
    cleanup(&ch).await;

    // The rollup timestamp the endpoint serves as its freshness header.
    assert!(computed_at >= now.timestamp(), "{computed_at}");

    // (a) not alive: absent from the read, present as a zero row.
    assert!(by_key(&world, ITEM_ADD_REMOVE, 0).is_none());
    assert_eq!(stored_add_remove, Some(0));

    // (b) the reprice counts once, at the new price.
    let b = by_key(&world, ITEM_REPRICE, 0).expect("reprice row");
    assert_eq!(b.alive_count, 1);
    assert_eq!(b.alive_units, 2);
    assert_eq!(b.distinct_retainers, 1);
    assert_eq!(b.floor_alive, 450);

    // (c) updated: post-state price, reviewed when the retainer touched it.
    let c = by_key(&world, ITEM_UPDATED, 0).expect("updated row");
    assert_eq!(c.alive_count, 1);
    assert_eq!(c.alive_units, 1);
    assert_eq!(c.floor_alive, 280);
    assert_eq!(
        c.oldest_reviewed_unix,
        (now - TimeDelta::seconds(60)).timestamp()
    );
    assert!(age_matches(c.median_age_secs, 60), "{}", c.median_age_secs);

    // (d) legacy identities do not collapse.
    let d = by_key(&world, ITEM_LEGACY, 0).expect("legacy row");
    assert_eq!(d.alive_count, 2);
    assert_eq!(d.alive_units, 6);
    assert_eq!(d.distinct_retainers, 2);
    assert_eq!(d.floor_alive, 100);

    // (e) NQ and HQ are separate keys; ages come from reviewed_at.
    let e = by_key(&world, ITEM_TWO_RETAINERS, 0).expect("two-retainer NQ row");
    assert_eq!(e.alive_count, 3);
    assert_eq!(e.alive_units, 5);
    assert_eq!(e.distinct_retainers, 2);
    assert_eq!(e.floor_alive, 900);
    assert_eq!(
        e.oldest_reviewed_unix,
        (now - TimeDelta::seconds(3600)).timestamp()
    );
    assert!(
        age_matches(e.median_age_secs, 1800),
        "{}",
        e.median_age_secs
    );
    let e_hq = by_key(&world, ITEM_TWO_RETAINERS, 1).expect("two-retainer HQ row");
    assert_eq!(e_hq.alive_count, 1);
    assert_eq!(e_hq.alive_units, 1);
    assert_eq!(e_hq.distinct_retainers, 1);
    assert_eq!(e_hq.floor_alive, 5000);

    // Same row, same second: the removal is the row's final state.
    assert!(by_key(&world, ITEM_SAME_ROW_TIE, 0).is_none());
    assert_eq!(stored_same_row_tie, Some(0));

    // Pre-seed: never replayed, so not even a zero row.
    assert!(by_key(&world, ITEM_PRE_SEED, 0).is_none());
    assert_eq!(stored_pre_seed, None);

    // Mid-stream removal is honoured; the other snapshot row is alive.
    let g = by_key(&world, ITEM_SEED_STREAM, 0).expect("seed-stream row");
    assert_eq!(g.alive_count, 1);
    assert_eq!(g.alive_units, 2);
    assert_eq!(g.floor_alive, 200);

    // Datacenter merge: sums add, minima take the min, the median merges.
    let f_world = by_key(&world, ITEM_MERGE, 0).expect("merge row, one world");
    assert_eq!(f_world.alive_count, 3);
    assert_eq!(f_world.alive_units, 4);
    assert_eq!(f_world.distinct_retainers, 2);
    assert_eq!(f_world.floor_alive, 100);
    assert_eq!(
        f_world.oldest_reviewed_unix,
        (now - TimeDelta::seconds(3000)).timestamp()
    );
    assert!(
        age_matches(f_world.median_age_secs, 2000),
        "{}",
        f_world.median_age_secs
    );
    let f_dc = by_key(&dc, ITEM_MERGE, 0).expect("merge row, two worlds");
    assert_eq!(f_dc.alive_count, 5);
    assert_eq!(f_dc.alive_units, 9);
    assert_eq!(f_dc.distinct_retainers, 3);
    assert_eq!(f_dc.floor_alive, 100);
    assert_eq!(
        f_dc.oldest_reviewed_unix,
        (now - TimeDelta::seconds(3000)).timestamp()
    );
    assert!(
        age_matches(f_dc.median_age_secs, 2000),
        "{}",
        f_dc.median_age_secs
    );

    // Nothing from the fixture leaks into an unrelated world.
    let leaked: Vec<_> = elsewhere
        .iter()
        .filter(|r| ALL_ITEMS.contains(&r.item_id) && r.item_id != ITEM_MERGE)
        .collect();
    assert!(leaked.is_empty(), "{leaked:?}");
}
