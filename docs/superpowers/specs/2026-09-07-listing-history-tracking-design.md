# Listing history tracking — design

**Date:** 2026-09-07
**Status:** approved for planning
**Scope:** record-only. No rollups, no queries, no endpoints, no UI.

## Problem

ClickHouse holds only `sales` and rollups of it. Listings live solely in
Postgres `active_listing` as the *current* set: a reprice overwrites the row in
place (`ultros-db/src/lib.rs`, `create_listing`'s `ON CONFLICT ... UPDATE`), a
removal is a hard `DELETE`, and nothing anywhere records when a listing
appeared, changed, or vanished. Every feature in the "what could we track"
discussion — lowest-listing-price history, suspicious-sale scoring against the
contemporaneous floor, listing churn, time-to-sell, days-of-inventory, seller
concentration — is blocked by the same missing thing: a listing change log.

This spec adds that log, plus a small companion table that records the lowest
price per item as it changes, so a "floor over the last month" sparkline is a
trivial query rather than an interval replay. Both start recording now so there
is data to surface when the consuming features are built.

## Facts that shaped the design

- The listings event bus payload (`ListingEventData` → `ActiveListing`) does
  **not** carry `listing_id`. A bus consumer cannot tell a new listing from a
  repriced one. The change list therefore has to come from the `ultros-db`
  write paths, which already hold both the old row and the new state.
- Universalis publishes a reprice as `listings/remove(old)` + `listings/add(new)`
  with the same `listingID`, in no guaranteed order. Our ingest collapses the
  pair into an in-place upsert when the add lands first, and into a real
  delete + insert when the remove lands first
  (`ultros-db/src/listings.rs`, `listings_to_remove_with_identity` docs).
- `add_listings` reads the existing rows for the item/world before upserting
  (`ultros-db/src/listings.rs`, `existing_items`), so the previous price of a
  repriced listing is already in memory. `diff_board_with_identity` and
  `remove_listings` likewise know exactly which rows they changed or deleted.
- About 6% of `listings/remove` events are lost (#1178). Phantom listings are
  corrected when the catch-up service next fetches the whole board for that
  item (`update_listings` emits the delete) and, for the analyzer's in-RAM
  floor map, by `rebuild_cheapest_from_db` on boot and on bus lag.
- The analyzer already maintains the current lowest price per
  `(item, hq)` per selector in `AnalyzerService::cheapest_items`
  (`ultros/src/analyzer_service.rs`, `CheapestListings`). This is what the site
  displays as "lowest price".
- The bounded ClickHouse `Writer` (`ultros-clickhouse/src/writer.rs`) is
  hard-wired to `SaleRow` and the `sales` table but its queue/retry/drain logic
  is row-agnostic.
- Prod runs a single `ultros` container. Multi-replica ingest is not a
  deployment that exists today (see Constraints).
- Volume is unmeasured: prod access was blocked during design. Sales run
  roughly 1.3M/day; listing events are expected to be several times that.

## Table 1: `listing_events` (raw change log)

```sql
CREATE TABLE IF NOT EXISTS listing_events (
    event_time      DateTime,                              -- when Ultros observed the change
    kind            Enum8('added' = 1, 'updated' = 2, 'removed' = 3),
    source          Enum8('websocket' = 1, 'catchup' = 2, 'manual' = 3, 'snapshot' = 4),
    item_id         Int32,
    hq              UInt8,
    world_id        Int32,
    listing_id      String,                                -- Universalis id; '' for legacy rows
    pg_listing_id   Int32,                                 -- active_listing.id; pairs legacy add/remove
    retainer_id     Int32,                                 -- Postgres retainer.id
    price_per_unit  UInt32,
    quantity        UInt16,
    prev_price      UInt32,                                -- 0 unless kind = 'updated'
    prev_quantity   UInt16,                                -- 0 unless kind = 'updated'
    reviewed_at     DateTime                               -- Universalis last_review_time
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(event_time)
ORDER BY (item_id, hq, world_id, event_time)
TTL event_time + INTERVAL 365 DAY
SETTINGS index_granularity = 8192
```

Semantics:

- `added`: a listing Ultros did not hold before. Post-state columns.
- `updated`: a `listing_id` Ultros held whose price or quantity changed — the
  same predicate as `view_state_matches_model`. Post-state in
  `price_per_unit`/`quantity`, pre-state in `prev_*`.
- `removed`: a row deleted from `active_listing`. Columns carry the deleted
  row's state.
- `source`: which ingest path produced the change. `websocket` = the socket
  listener in `main.rs`; `catchup` = `UpdateService` (periodic catch-up and
  full sweeps); `manual` = the `/item/refresh/{world}/{item}` route;
  `snapshot` = the one-time seed (below).
- A re-sent listing whose state already matches produces **no row**, because
  the DB diff filters it before any write. Duplicate websocket deliveries are
  therefore absorbed upstream and the table needs no dedup engine.
- **Reprice ordering caveat.** When the add lands before the remove, a reprice
  is one `updated` row. When the remove lands first, it is a `removed` row
  followed by an `added` row with the same `listing_id`. Ingest cannot control
  which; both are correct history and readers can reconcile a
  `removed`/`added` pair on the same `listing_id` within a short window.
- `event_time` is Ultros's observation time (`Utc::now()` when the DB write
  completes), not Universalis's `last_review_time`; the latter is kept in
  `reviewed_at` for analysis.
- Retention: 365 days, monthly partitions. Volume is unmeasured, so the TTL
  is the escape hatch; tightening it is a single `ALTER TABLE ... MODIFY TTL`
  added to `schema::apply`.

### One-time seed

The table starts empty at deploy. A listing already on the board that never
changes emits no row, so a floor replayed from events alone reads too high
until that listing is finally removed — and a cheap listing untouched for
over a month is invisible. To make the alive set complete from day one, seed
once:

- Runs inside `spawn_rollup_scheduler`'s leader task (it already holds the
  Postgres advisory lock, the `ClickHouseClient`, and `UltrosDb`), after
  schema readiness.
- Guarded by a marker table `_listing_events_seed (seeded_at DateTime,
  rows_streamed UInt64) ENGINE = ReplacingMergeTree(seeded_at) ORDER BY
  tuple()`, modelled on `_backfill_state`. If a marker row exists, skip.
- Streams every `active_listing` row from Postgres (sea-orm `stream()`, the
  same shape as `cheapest_listings`) and bulk-inserts them as
  `kind = 'added'`, `source = 'snapshot'`, `event_time = now()`,
  `prev_* = 0`, in chunks of 10k via a direct `client.insert`, not the bounded
  writer. Writes the marker only after the last chunk succeeds.
- A failed seed logs at `warn!`, increments
  `ultros_listing_events_seed_failures_total`, and retries ten minutes later
  for as long as the leader holds the lease. Losing the lease aborts an
  in-flight stream, so two leaders can never seed at once. The marker is
  written only on full success; every attempt first deletes any
  `source = 'snapshot'` rows a torn run left behind, so a retry restarts
  cleanly.

## Table 2: `floor_changes` (lowest price as it moves)

```sql
CREATE TABLE IF NOT EXISTS floor_changes (
    event_time      DateTime,
    item_id         Int32,
    hq              UInt8,
    world_id        Int32,
    price_per_unit  UInt32,                                -- 0 = no listings on the board
    reason          Enum8('listing' = 1, 'refill' = 2, 'resync' = 3)
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(event_time)
ORDER BY (item_id, hq, world_id, event_time)
SETTINGS index_granularity = 8192
```

One row whenever the analyzer's **world-level** lowest price for an
`(item, hq)` changes. Datacenter and region floors are the minimum over their
worlds and are derivable, so only `AnySelector::World` entries emit. No TTL:
the table is small (one row per floor transition) and is the long-term series.

`price_per_unit = 0` means the board emptied. Universalis never reports a
listing below 1 gil, so 0 is unambiguous.

Emission points, all in `ultros/src/analyzer_service.rs`:

| reason    | where                                   | when a row is written                                                                                                                                                                        |
|-----------|-----------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `listing` | `CheapestListings::add_listing`         | the key was absent, or the new price is strictly lower than the stored one. `add_listing` returns the new value when it changed the map.                                                     |
| `refill`  | `remove_from_selector`, phase 3 (apply) | for each stale key: the refilled price if the DB returned one, else 0. Emitted only when that value differs from the price the entry held before phase 1 dropped it.                        |
| `resync`  | `rebuild_cheapest_from_db`              | diff of the fresh world map against the map it replaces; one row per key whose price differs (absent → present, present → absent, or changed). On a cold boot with no restored snapshot this is every key, and serves as the series' anchor. |

The `resync` diff is the self-healing path: a phantom minimum stranded by a
lost remove is corrected here, and the correction is recorded rather than
silently overwritten. Because the map is rebuilt from Postgres on every boot
(`run_worker`), `floor_changes` needs no separate seed.

Transport: `listing` and `refill` rows go through a bounded
`Writer<FloorChangeRow>`. `resync` rows can be millions on a cold boot and
would overflow the 10k queue, so the rebuild collects its diff into a `Vec`
and bulk-inserts it directly in 10k chunks after `wait_ready()`. A failed
bulk insert is counted (`ultros_floor_changes_bulk_failures_total`) and
logged, not retried: the next resync re-derives the same state.

## Producing changes: `ultros-db`

New public types in `ultros-db/src/listings.rs`:

```rust
pub enum ListingChangeKind { Added, Updated, Removed }

pub struct ListingChange {
    pub kind: ListingChangeKind,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    /// Post-state for Added/Updated; the deleted row for Removed.
    pub row: active_listing::Model,
    /// Set only for Updated.
    pub prev_price_per_unit: Option<i32>,
    pub prev_quantity: Option<i32>,
}

pub struct ListingWrite {
    pub added: Vec<(ActiveListing, Retainer)>,
    pub removed: Vec<(ActiveListing, Retainer)>,
    pub changes: Vec<ListingChange>,
}
```

`add_listings`, `remove_listings`, and `update_listings` all return
`Result<ListingWrite>`, replacing today's `Vec<(ActiveListing, Retainer)>`
and the `ListingUpdate` tuple alias. `added`/`removed` keep their current
contents so the three call sites publish to the event bus exactly as before;
`changes` rides alongside.

Where each change is derived:

- `listings_to_upsert` returns `Vec<(ListingView, Option<active_listing::Model>)>`
  — the `Some` is the id-matched row whose state differed. After
  `create_listing`'s `RETURNING` gives the post-state row, a `Some` previous
  becomes `Updated` with `prev_*` from the old model; `None` becomes `Added`.
- `diff_board_with_identity` does the same for its `added` side: the
  `Some(_) | None => added.push(view)` arm keeps the matched model. Its
  `removed` models become `Removed`.
- `remove_listings` maps the rows returned by `exec_with_returning` to
  `Removed`. Superseded removals (state mismatch) are already excluded there
  and correctly emit nothing.

`observed_at` is stamped once per write call, after the DB round-trip.

## Transport: generic `Writer`

`ultros-clickhouse/src/writer.rs`:

```rust
pub trait TableRow: clickhouse::Row + serde::Serialize + Send + Sync + 'static {
    const TABLE: &'static str;
}
pub struct Writer<R: TableRow> { tx: mpsc::Sender<R>, ... }
```

`flush` inserts into `R::TABLE`. `SaleRow` implements `TableRow` with
`"sales"`; the two new rows are `ListingEventRow` (`"listing_events"`) and
`FloorChangeRow` (`"floor_changes"`), both in `rows.rs` with `From`
conversions (`ListingEventRow::from_change(&ListingChange, Source)`,
`FloorChangeRow::new(...)`). Existing writer tests keep using `SaleRow`.

All `ultros_clickhouse_writer_*` metrics gain a `table` label. Nothing in
`docs/grafana-ingest-dashboard.json` references them, so no dashboard breaks;
`docs/ingest-observability.md` gets a line noting the label.

Three writers run side by side, each `spawn_recovering` (the migrate is
idempotent, so concurrent schema application is harmless):

| writer                     | producers                                                                         | shutdown                                                             |
|----------------------------|-----------------------------------------------------------------------------------|----------------------------------------------------------------------|
| `Writer<SaleRow>`          | analyzer history loop (unchanged)                                                 | after analyzer, as today                                             |
| `Writer<ListingEventRow>`  | socket listener (`run_socket_listener`), `UpdateService`, manual refresh route    | after the web task and update service have stopped (they hold the producers) |
| `Writer<FloorChangeRow>`   | analyzer listings loop                                                            | alongside the sale writer, after analyzer shutdown                   |

Wiring changes in `ultros/src/main.rs`: `ClickHouseClient::from_env()` and
the writers are constructed **before** the socket listener is spawned (today
they come after); `run_socket_listener` gains a `Writer<ListingEventRow>`
parameter; `UpdateService` gains a `listing_events` field; the refresh route
reads it from `WebState`; `AnalyzerService::start_analyzer` takes the
`Writer<FloorChangeRow>`.

Every producer uses the existing non-blocking `send` (`try_send`, drop-and-
count on overflow). The ingest path must never back-pressure on analytics;
same contract as sales today.

## Schema application

`schema::apply` gains `apply_listing_events_table`, `apply_floor_changes_table`,
and `apply_listing_events_seed_marker`, each `CREATE TABLE IF NOT EXISTS`,
following the existing per-table function style with a doc comment
explaining the ORDER BY and TTL choices.

## Constraints and known limitations

- **Single ingest process.** With more than one `ultros` replica, each would
  emit its own `listing_events` (mostly deduped by the DB diff, but racy) and
  its own `floor_changes` (fully duplicated, since each replica has its own
  map). Prod is one container today. If replicas arrive, gate both emissions
  on the rollup leader lock; readers should tolerate duplicates in the
  meantime.
- **Lossy by design.** Bus lag, writer overflow, and process crashes drop
  rows, exactly as for `sales`. `listing_events` cannot be backfilled from
  Postgres (the history does not exist there); `floor_changes` self-heals on
  the next resync. Dropped rows are visible via
  `ultros_clickhouse_writer_dropped_rows_total{table=...}`.
- **Floor is "as Ultros believed it."** A phantom listing from a lost remove
  drags the recorded floor down until the next full-board fetch or resync.
  That is the honest thing to chart, since it is what the site displayed.
- **`sales/remove` stays ignored** (`main.rs` logs it). Out of scope; noted
  as a separate correctness gap.
- **Volume unmeasured.** The 365-day TTL and monthly partitions are the
  escape hatch. After deploy, watch `ultros_clickhouse_writer_written_rows_total{table="listing_events"}`
  for a day and revisit the TTL.

## Testing

- `ultros-db` unit tests (pure functions, no DB): `listings_to_upsert` and
  `diff_board_with_identity` return the previous model for a state change,
  `None` for a new listing, and nothing for an identical re-send; the
  `ListingChange` built from each case has the right kind and `prev_*`.
- `ultros-clickhouse` unit tests: `ListingEventRow::from_change` and
  `FloorChangeRow` field mapping, including the clamps (`u32`/`u16`, negative
  → 0) mirrored from `SaleRow`; `prev_*` is 0 for non-updated kinds.
- `ultros-clickhouse` writer unit tests: the generic `run_writer` still
  passes with `SaleRow`; one test instantiates it with `ListingEventRow` to
  prove the type parameter works.
- `ultros/src/analyzer_service.rs` unit tests: `add_listing` reports a change
  only on absent-or-lower; the resync diff yields exactly the changed keys
  (absent→present, present→absent, price change) and nothing for unchanged.
- Smoke tests (need a live ClickHouse, alongside the existing
  `tests/*_smoke.rs`): schema applies idempotently twice; a
  `Writer<ListingEventRow>` and a `Writer<FloorChangeRow>` insert and read
  back; the seed inserts rows and writes the marker, and a second run skips.

## Out of scope

Rollups (`listing_floor_hourly`, churn counts), any query function, any HTTP
endpoint, any UI, sale-to-listing matching, and handling `sales/remove`.
