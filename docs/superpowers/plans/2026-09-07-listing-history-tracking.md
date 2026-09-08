# Listing History Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Start recording every listing change (`listing_events`) and every lowest-price transition (`floor_changes`) in ClickHouse, seeded from the current board, with no consumer yet.

**Architecture:** The three `ultros-db` listing write paths already know the before/after state of every row they touch; they return a `ListingWrite { added, removed, changes }` so the existing bus publishing is unchanged and a `Vec<ListingChange>` rides alongside. The bounded ClickHouse `Writer` becomes generic over its row type so three writers (`SaleRow`, `ListingEventRow`, `FloorChangeRow`) share one queue/retry/drain implementation. The analyzer's in-RAM cheapest-price map emits a `FloorChangeRow` whenever a world-level floor moves; its boot/lag resync diffs old vs new and bulk-inserts the corrections.

**Tech Stack:** Rust 2024, sea-orm (Postgres), `clickhouse` 0.15 (RowBinaryWithNamesAndTypes with validation on — struct field names MUST match column names, Enum8 needs `serde_repr`), tokio, `metrics`.

**Spec:** `docs/superpowers/specs/2026-09-07-listing-history-tracking-design.md`

## Global Constraints

- Run `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"` before every commit (fmt + clippy `-D warnings` + tests). Never `#[allow]` a clippy lint to silence it.
- No user-facing strings are introduced (record-only), so no locale work.
- Ingest must never back-pressure on analytics: every producer uses the writer's non-blocking `send` (`try_send`, drop-and-count). Bulk inserts run in detached tasks, never on the boot path.
- ClickHouse DDL is idempotent `CREATE TABLE IF NOT EXISTS` applied from `schema::apply` on every startup.
- Smoke tests that need a live ClickHouse are gated on `ULTROS_CH_INTEGRATION=1` and skip otherwise (existing pattern in `ultros-clickhouse/tests/*_smoke.rs`).
- On Windows, prepend `/c/Strawberry/perl/bin:/c/Strawberry/c/bin:` to `PATH` in Git Bash before any `cargo build` of the `ultros` crate (vendored OpenSSL). `cargo test -p ultros-db` and `-p ultros-clickhouse` do not need it.
- Commit after each task. Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## File Structure

| File | Responsibility |
|---|---|
| `ultros-db/src/listings.rs` | (modify) `ListingChange`, `ListingChangeKind`, `ListingWrite` types; diff helpers return the replaced row; the three write paths return `ListingWrite`; `stream_active_listings` for the seed. |
| `ultros-clickhouse/src/rows.rs` | (modify) `TableRow` trait; `ListingEventRow`, `FloorChangeRow` and their Enum8 types with conversions. |
| `ultros-clickhouse/src/schema.rs` | (modify) DDL for `listing_events`, `floor_changes`, `_listing_events_seed`. |
| `ultros-clickhouse/src/writer.rs` | (modify) `Writer<R: TableRow>`; `insert_all` bulk helper; `table` metric label. |
| `ultros-clickhouse/src/listing_seed.rs` | (create) one-time seed of `listing_events` from `active_listing`, guarded by the marker table. |
| `ultros-clickhouse/src/lib.rs` | (modify) `pub mod listing_seed;`. |
| `ultros-clickhouse/Cargo.toml` | (modify) add `serde_repr`. |
| `ultros-clickhouse/tests/listing_events_smoke.rs` | (create) schema + writer + seed-marker round trip against a live ClickHouse. |
| `ultros/src/main.rs` | (modify) construct three writers before the socket spawn; thread `Writer<ListingEventRow>` into the socket listener, `UpdateService`, `WebState`; seed task in the rollup leader; shutdown order. |
| `ultros/src/item_update_service.rs` | (modify) `listing_events` field; emit catch-up rows. |
| `ultros/src/web.rs`, `ultros/src/web/state.rs` | (modify) `listing_events` on `WebState`; manual refresh route emits rows. |
| `ultros/src/analyzer_service.rs` | (modify) `floor_writer` field; `add_listing` reports changes; refill and resync emission; `floor_diff`. |
| `docs/ingest-observability.md` | (modify) new metrics and the `table` label. |

---

### Task 1: `ultros-db` — diff helpers report the replaced row

**Files:**
- Modify: `ultros-db/src/listings.rs` (types near line 114; `ListingsDiff` at ~238; `listings_to_upsert` at ~372; `diff_board_with_identity` at ~447; tests in `mod diff_tests` from ~1085)

**Interfaces:**
- Produces:
  ```rust
  pub enum ListingChangeKind { Added, Updated, Removed }
  pub struct ListingChange {
      pub kind: ListingChangeKind,
      pub observed_at: chrono::DateTime<chrono::Utc>,
      pub row: active_listing::Model,          // post-state for Added/Updated; the deleted row for Removed
      pub prev_price_per_unit: Option<i32>,    // Some only for Updated
      pub prev_quantity: Option<i32>,          // Some only for Updated
  }
  impl ListingChange {
      pub fn added(row: active_listing::Model, observed_at: DateTime<Utc>) -> Self;
      pub fn removed(row: active_listing::Model, observed_at: DateTime<Utc>) -> Self;
      /// `Updated` (with prev_* from `previous`) when `previous` is Some, else `Added`.
      pub fn upserted(row: active_listing::Model, previous: Option<&active_listing::Model>, observed_at: DateTime<Utc>) -> Self;
  }
  fn listings_to_upsert(listings: Vec<ListingView>, existing: RowsWithRetainers) -> Vec<(ListingView, Option<active_listing::Model>)>;
  struct BoardDiff { added: Vec<(ListingView, Option<active_listing::Model>)>, removed: Vec<(active_listing::Model, Option<retainer::Model>)> }
  fn diff_board_with_identity(listings: Vec<ListingView>, existing: RowsWithRetainers) -> BoardDiff;
  ```

- [ ] **Step 1: Add the types**

Below `pub type ListingsWithRetainers = ...` (~line 119) add:

```rust
/// What a listing write did to one `active_listing` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListingChangeKind {
    /// A listing Ultros did not hold before.
    Added,
    /// A `listing_id` Ultros held whose price or quantity changed
    /// (the same predicate as [`view_state_matches_model`]).
    Updated,
    /// A row deleted from `active_listing`.
    Removed,
}

/// One change to the stored board, as observed by a write path. This is the
/// unit the ClickHouse `listing_events` mirror records; the bus payload
/// (`ListingEventData`) cannot serve because it carries no `listing_id` and
/// no previous state.
#[derive(Debug, Clone, PartialEq)]
pub struct ListingChange {
    pub kind: ListingChangeKind,
    /// When Ultros observed the change (stamped after the DB round-trip), not
    /// Universalis' `last_review_time` — that stays on `row.timestamp`.
    pub observed_at: chrono::DateTime<chrono::Utc>,
    /// Post-state for `Added`/`Updated`; the deleted row for `Removed`.
    pub row: active_listing::Model,
    /// Set only for `Updated`.
    pub prev_price_per_unit: Option<i32>,
    /// Set only for `Updated`.
    pub prev_quantity: Option<i32>,
}

impl ListingChange {
    pub fn added(row: active_listing::Model, observed_at: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            kind: ListingChangeKind::Added,
            observed_at,
            row,
            prev_price_per_unit: None,
            prev_quantity: None,
        }
    }

    pub fn removed(row: active_listing::Model, observed_at: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            kind: ListingChangeKind::Removed,
            observed_at,
            row,
            prev_price_per_unit: None,
            prev_quantity: None,
        }
    }

    /// `Updated` when the upsert replaced a stored row, `Added` otherwise.
    pub fn upserted(
        row: active_listing::Model,
        previous: Option<&active_listing::Model>,
        observed_at: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        match previous {
            Some(prev) => Self {
                kind: ListingChangeKind::Updated,
                observed_at,
                row,
                prev_price_per_unit: Some(prev.price_per_unit),
                prev_quantity: Some(prev.quantity),
            },
            None => Self::added(row, observed_at),
        }
    }
}

/// Result of one listing write path. `added`/`removed` are the bus payloads
/// (unchanged shape); `changes` is the ClickHouse mirror's input.
#[derive(Debug, Default)]
pub struct ListingWrite {
    pub added: Vec<(ActiveListing, Retainer)>,
    pub removed: Vec<(ActiveListing, Retainer)>,
    pub changes: Vec<ListingChange>,
}
```

Delete the `pub type ListingUpdate = (...)` alias (line ~114).

- [ ] **Step 2: Write the failing tests**

In `mod diff_tests` (after `upsert_does_not_duplicate_a_legacy_row_matching_by_content`), add:

```rust
    #[test]
    fn upsert_reports_the_replaced_row_for_a_reprice() {
        let world_id = 54;
        let retainer = retainer_model(1, world_id, "Idmus");
        let existing = vec![(
            db_listing_with_id(1, world_id, retainer.id, 500, 3, false, "abc"),
            Some(retainer.clone()),
        )];
        let delta = vec![listing_view_with_id(world_id, &retainer.name, 450, 3, false, "abc")];

        let upserts = listings_to_upsert(delta, existing);

        assert_eq!(upserts.len(), 1);
        let (view, previous) = &upserts[0];
        assert_eq!(view.price_per_unit, Some(450));
        let previous = previous.as_ref().expect("a reprice must carry the row it replaces");
        assert_eq!(previous.price_per_unit, 500);
        assert_eq!(previous.quantity, 3);
    }

    #[test]
    fn upsert_reports_no_previous_row_for_a_new_listing() {
        let world_id = 54;
        let retainer = retainer_model(1, world_id, "Idmus");
        let existing = vec![(
            db_listing_with_id(1, world_id, retainer.id, 500, 1, false, "abc"),
            Some(retainer.clone()),
        )];
        let delta = vec![listing_view_with_id(world_id, &retainer.name, 700, 1, false, "xyz")];

        let upserts = listings_to_upsert(delta, existing);

        assert_eq!(upserts.len(), 1);
        assert!(upserts[0].1.is_none(), "a new id has nothing to replace");
    }

    #[test]
    fn board_diff_pairs_a_reprice_with_its_replaced_row_and_a_new_listing_with_none() {
        let world_id = 54;
        let retainer = retainer_model(1, world_id, "Idmus");
        let existing = vec![
            (
                db_listing_with_id(1, world_id, retainer.id, 500, 1, false, "abc"),
                Some(retainer.clone()),
            ),
            (
                db_listing_with_id(2, world_id, retainer.id, 900, 1, false, "gone"),
                Some(retainer.clone()),
            ),
        ];
        let board = vec![
            listing_view_with_id(world_id, &retainer.name, 450, 1, false, "abc"),
            listing_view_with_id(world_id, &retainer.name, 700, 1, false, "new"),
        ];

        let BoardDiff { added, removed } = diff_board_with_identity(board, existing);

        assert_eq!(added.len(), 2);
        let repriced = added.iter().find(|(v, _)| v.listing_id.as_deref() == Some("abc")).unwrap();
        assert_eq!(repriced.1.as_ref().map(|m| m.price_per_unit), Some(500));
        let fresh = added.iter().find(|(v, _)| v.listing_id.as_deref() == Some("new")).unwrap();
        assert!(fresh.1.is_none());
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].0.listing_id.as_deref(), Some("gone"));
    }

    #[test]
    fn listing_change_upserted_is_updated_only_with_a_previous_row() {
        let now = chrono::Utc::now();
        let previous = db_listing_with_id(1, 54, 1, 500, 3, false, "abc");
        let mut current = previous.clone();
        current.price_per_unit = 450;

        let updated = ListingChange::upserted(current.clone(), Some(&previous), now);
        assert_eq!(updated.kind, ListingChangeKind::Updated);
        assert_eq!(updated.prev_price_per_unit, Some(500));
        assert_eq!(updated.prev_quantity, Some(3));
        assert_eq!(updated.row.price_per_unit, 450);

        let added = ListingChange::upserted(current, None, now);
        assert_eq!(added.kind, ListingChangeKind::Added);
        assert_eq!(added.prev_price_per_unit, None);
        assert_eq!(added.prev_quantity, None);
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p ultros-db diff_tests 2>&1 | tail -20`
Expected: compile errors (`BoardDiff` undefined, tuple pattern on `ListingView`).

- [ ] **Step 4: Change `listings_to_upsert`**

Replace the function body (keep its doc comment, add one sentence: "Each entry pairs the view to write with the stored row it replaces — `Some` for a state change on a known id, `None` for a genuinely new listing."):

```rust
fn listings_to_upsert(
    listings: Vec<ListingView>,
    existing: RowsWithRetainers,
) -> Vec<(ListingView, Option<active_listing::Model>)> {
    let (id_rows, legacy_rows) = split_rows_by_identity(existing);
    let by_id: HashMap<&str, &active_listing::Model> = id_rows
        .iter()
        .filter_map(|(m, _)| m.listing_id.as_deref().map(|id| (id, m)))
        .collect();

    let mut upserts = Vec::new();
    let mut fallback = Vec::new();
    for view in listings {
        match view.listing_id.as_deref().and_then(|id| by_id.get(id)) {
            Some(model) if view_state_matches_model(&view, model) => {}
            Some(model) => upserts.push((view, Some((*model).clone()))),
            None => fallback.push(view),
        }
    }
    upserts.extend(
        listings_to_add(fallback, legacy_rows)
            .into_iter()
            .map(|view| (view, None)),
    );
    upserts
}
```

- [ ] **Step 5: Change `diff_board_with_identity`**

Add a new struct next to `ListingsDiff` (leave `ListingsDiff` alone — `diff_update_listings` still returns it):

```rust
/// Like [`ListingsDiff`], but each added view carries the stored row it
/// replaces when the board re-sent a known `listing_id` with new state.
struct BoardDiff {
    added: Vec<(ListingView, Option<active_listing::Model>)>,
    removed: Vec<(active_listing::Model, Option<retainer::Model>)>,
}
```

Change the signature to `-> BoardDiff` and the loop body:

```rust
    let mut added = Vec::new();
    for view in id_views {
        let id = view.listing_id.as_deref().expect("partitioned on is_some");
        match by_id.remove(id) {
            // board re-sends the same state: entry consumed = row kept as-is
            Some((model, _)) if view_state_matches_model(&view, &model) => {}
            // state changed: upsert, remembering what it replaces
            Some((model, _)) => added.push((view, Some(model))),
            // guard-fallthrough or unknown id: plain insert
            None => added.push((view, None)),
        }
    }
    // ids the board no longer carries
    let mut removed: Vec<_> = by_id.into_values().collect();

    let legacy = diff_update_listings(legacy_views, legacy_rows);
    added.extend(legacy.added.into_iter().map(|view| (view, None)));
    removed.extend(legacy.removed);
    BoardDiff { added, removed }
```

- [ ] **Step 6: Fix the callers inside `listings.rs` just enough to compile**

In `add_listings`, `to_add` is now a `Vec<(ListingView, Option<Model>)>`; temporarily map it: `let to_add = listings_to_upsert(listings, existing_items); let added = fan_out_listing_writes(to_add.into_iter().map(|(m, _previous)| { ... }))`. In `update_listings`, `let BoardDiff { added, removed } = diff_board_with_identity(listings, existing_items);` and `added.into_iter().map(|(m, _previous)| { ... })`. (Task 2 replaces these with the real change plumbing.)

- [ ] **Step 7: Fix the existing tests that index the old shape**

Run `cargo test -p ultros-db diff_tests 2>&1 | grep -E "^error" -A 5`. Every failure is a test reading a field off an element of `listings_to_upsert(...)` or `diff_board_with_identity(...).added`; change `upserts[0].price_per_unit` to `upserts[0].0.price_per_unit`, `ListingsDiff { added, removed } = diff_board_with_identity(...)` to `BoardDiff { added, removed } = ...`, and any `added[i].field` on a board diff to `added[i].0.field`.

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test -p ultros-db 2>&1 | tail -5`
Expected: all pass, including the four new tests.

- [ ] **Step 9: Commit**

```bash
git add ultros-db/src/listings.rs
git commit -m "feat(db): listing diffs report the row each upsert replaces

Introduces ListingChange/ListingWrite so the write paths can hand the
ClickHouse mirror a change list with previous state and listing ids.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: `ultros-db` — write paths return `ListingWrite`; `stream_active_listings`

**Files:**
- Modify: `ultros-db/src/listings.rs` (`add_listings` ~537, `remove_listings` ~600, `update_listings` ~795, `cheapest_listings` ~925)
- Modify: `ultros/src/main.rs:251-283`, `ultros/src/item_update_service.rs:732-750`, `ultros/src/web.rs:1289-1305`

**Interfaces:**
- Consumes: `ListingChange`, `ListingWrite`, `BoardDiff` from Task 1.
- Produces:
  ```rust
  impl UltrosDb {
      pub async fn add_listings(&self, listings: Vec<ListingView>, item_id: ItemId, world_id: WorldId) -> Result<ListingWrite>;
      pub async fn remove_listings(&self, remove_listings: Vec<ListingView>, item_id: ItemId, world_id: WorldId) -> Result<ListingWrite>;
      pub async fn update_listings(&self, listings: Vec<ListingView>, item_id: ItemId, world_id: WorldId) -> Result<ListingWrite>;
      pub async fn stream_active_listings(&self) -> Result<impl Stream<Item = Result<active_listing::Model, DbErr>> + '_, anyhow::Error>;
  }
  ```

- [ ] **Step 1: `add_listings`**

Replace from `let to_add = ...` through `Ok(added)`:

```rust
        let to_add = listings_to_upsert(listings, existing_items);
        let written = fan_out_listing_writes(to_add.into_iter().map(|(m, previous)| {
            let retainer_id = retainers
                .get(&m.retainer_name)
                .expect("Should always have a retainer at this point.")
                .id;
            // Each future owns its `ListingView`. Holding a borrow into `to_add`
            // across the buffered stream instead makes rustc give up on the
            // higher-ranked lifetime and fail every `tokio::spawn` that reaches
            // this call with "implementation of `Send` is not general enough".
            async move {
                let row = self
                    .create_listing(&m, item_id, world_id, Some(retainer_id))
                    .await?;
                Ok((row, previous))
            }
        }))
        .await?;

        let observed_at = chrono::Utc::now();
        let changes: Vec<ListingChange> = written
            .iter()
            .map(|(row, previous)| ListingChange::upserted(row.clone(), previous.as_ref(), observed_at))
            .collect();

        let retainers_by_id: HashMap<i32, &retainer::Model> =
            retainers.values().map(|r| (r.id, r)).collect();
        let added: Vec<_> = written
            .into_iter()
            .map(|(l, _)| {
                let retainer = (*retainers_by_id.get(&l.retainer_id).unwrap())
                    .clone()
                    .into();
                (l.into(), retainer)
            })
            .collect();

        // (keep the existing set_last_updated / counter / histogram lines)
        self.set_last_updated(world_id, item_id).await?;
        counter!("ultros_db_inserted_items", "world_id" => world_id.0.to_string())
            .increment(added.len() as u64);
        histogram!("ultros_db_add_listings_duration_seconds").record(instant.elapsed());
        Ok(ListingWrite {
            added,
            removed: vec![],
            changes,
        })
```

Change the signature's return type to `Result<ListingWrite>`.

- [ ] **Step 2: `remove_listings`**

Return type `Result<ListingWrite>`. The two early `return Ok(vec![])` become `return Ok(ListingWrite::default())`. Replace the final `Ok(items.into_iter()...collect())` with:

```rust
        let observed_at = chrono::Utc::now();
        let changes = items
            .iter()
            .map(|row| ListingChange::removed(row.clone(), observed_at))
            .collect();
        let removed = items
            .into_iter()
            .flat_map(|i| retainers.get(&i.retainer_id).map(|r| (i.into(), r.clone())))
            .collect();
        Ok(ListingWrite {
            added: vec![],
            removed,
            changes,
        })
```

- [ ] **Step 3: `update_listings`**

Return type `Result<ListingWrite>`. Replace from `let ListingsDiff { added, removed } = ...` through `Ok((added, removed))`:

```rust
        let BoardDiff { added, removed } = diff_board_with_identity(listings, existing_items);
        let remove_iter = removed.iter();
        // Each future owns its `ListingView` — see the note in `add_listings`.
        let added = added.into_iter().map(|(m, previous)| {
            let retainer_id = retainers
                .get(&m.retainer_name)
                .expect("Should always have a retainer at this point.")
                .id;
            async move {
                let row = self
                    .create_listing(&m, item_id, world_id, Some(retainer_id))
                    .await?;
                Ok((row, previous))
            }
        });
        let (written, removed_result) =
            futures::future::join(fan_out_listing_writes(added), async move {
                let ids_to_remove: Vec<i32> = remove_iter.map(|(l, _)| l.id).collect();
                if ids_to_remove.is_empty() {
                    return Result::<usize>::Ok(0);
                }
                let res = active_listing::Entity::delete_many()
                    .filter(active_listing::Column::Id.is_in(ids_to_remove))
                    .exec(&self.db)
                    .await?;
                Result::<usize>::Ok(res.rows_affected as usize)
            })
            .await;
        // Writes may have partially succeeded, but a failed insert or delete
        // must not report a successful reconciliation or advance its marker.
        let written = written?;
        removed_result?;

        let observed_at = chrono::Utc::now();
        // Every deleted row is a change even when its retainer is unknown and
        // it therefore never reaches the bus payload below.
        let mut changes: Vec<ListingChange> = removed
            .iter()
            .map(|(m, _)| ListingChange::removed(m.clone(), observed_at))
            .collect();
        changes.extend(
            written
                .iter()
                .map(|(row, previous)| ListingChange::upserted(row.clone(), previous.as_ref(), observed_at)),
        );

        let retainers_by_id: HashMap<i32, &retainer::Model> =
            retainers.values().map(|r| (r.id, r)).collect();
        let added: Vec<_> = written
            .into_iter()
            .map(|(l, _)| {
                let retainer = (*retainers_by_id.get(&l.retainer_id).unwrap())
                    .clone()
                    .into();
                (l.into(), retainer)
            })
            .collect();
        let removed: Vec<_> = removed
            .into_iter()
            .flat_map(|(m, r)| r.map(|r| (m.into(), r.into())))
            .collect();
        self.set_last_updated(world_id, item_id).await?;
        counter!("ultros_db_inserted_items", "world_id" => world_id.0.to_string())
            .increment(added.len() as u64);
        counter!("ultros_db_removed_items", "world_id" => world_id.0.to_string())
            .increment(removed.len() as u64);
        histogram!("ultros_db_update_listings_duration_seconds").record(instant.elapsed());
        Ok(ListingWrite {
            added,
            removed,
            changes,
        })
```

- [ ] **Step 4: `stream_active_listings`**

Directly above `pub async fn cheapest_listings(`:

```rust
    /// Every row of `active_listing`, for the one-time ClickHouse
    /// `listing_events` seed. Streamed, not collected: the table is millions
    /// of rows.
    pub async fn stream_active_listings(
        &self,
    ) -> Result<impl Stream<Item = Result<active_listing::Model, DbErr>> + '_, anyhow::Error> {
        Ok(active_listing::Entity::find().stream(&self.db).await?)
    }
```

- [ ] **Step 5: Build `ultros-db`**

Run: `cargo build -p ultros-db 2>&1 | tail -20`
Expected: success (fix any missed `Ok(vec![])`).

- [ ] **Step 6: Update the three callers to destructure**

`ultros/src/main.rs` ~251:
```rust
                    })) => match db.add_listings(listings.clone(), item, world).await {
                        Ok(write) => {
                            let added = Arc::new(ListingEventData {
                                item_id: item.0,
                                world_id: world.0,
                                listings: write.added,
                            });
```
and ~269:
```rust
                    })) => match db.remove_listings(listings.clone(), item, world).await {
                        Ok(write) => {
                            info!(listings = ?write.removed, ?item, ?world, "Removed listings");
                            if let Err(e) = listings_tx.send(EventType::removed(ListingEventData {
                                item_id: item.0,
                                world_id: world.0,
                                listings: write.removed,
                            })) {
```

`ultros/src/item_update_service.rs` ~732: `Ok((added, removed)) =>` becomes `Ok(ultros_db::listings::ListingWrite { added, removed, changes: _ }) =>` (add `use ultros_db::listings::ListingWrite;` at the top instead if the file already imports from `ultros_db::listings`).

`ultros/src/web.rs` ~1289: `let (added, removed) = db.update_listings(...)` becomes `let ultros_db::listings::ListingWrite { added, removed, .. } = db.update_listings(...)`.

- [ ] **Step 7: Build the workspace**

Run (Git Bash, with Strawberry Perl on PATH): `cargo check -p ultros 2>&1 | tail -20`
Expected: success.

- [ ] **Step 8: Run `ultros-db` tests, fmt, commit**

```bash
cargo test -p ultros-db 2>&1 | tail -5
cargo fmt --all
git add ultros-db/src/listings.rs ultros/src/main.rs ultros/src/item_update_service.rs ultros/src/web.rs
git commit -m "feat(db): listing write paths return ListingWrite with a change list

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: `ultros-clickhouse` rows — `TableRow`, `ListingEventRow`, `FloorChangeRow`

**Files:**
- Modify: `ultros-clickhouse/Cargo.toml` (add `serde_repr = "0.1"` under `[dependencies]`)
- Modify: `ultros-clickhouse/src/rows.rs`

**Interfaces:**
- Consumes: `ListingChange`, `ListingChangeKind` from `ultros_db::listings`.
- Produces:
  ```rust
  pub trait TableRow: clickhouse::Row + serde::Serialize + Send + Sync + 'static { const TABLE: &'static str; }
  impl TableRow for SaleRow          // "sales"
  #[repr(i8)] pub enum ListingEventKind { Added = 1, Updated = 2, Removed = 3 }
  #[repr(i8)] pub enum ListingEventSource { Websocket = 1, Catchup = 2, Manual = 3, Snapshot = 4 }
  pub struct ListingEventRow { event_time, kind, source, item_id, hq, world_id, listing_id, pg_listing_id, retainer_id, price_per_unit, quantity, prev_price, prev_quantity, reviewed_at }
  impl ListingEventRow {
      pub fn from_change(change: &ListingChange, source: ListingEventSource) -> Self;
      pub fn from_snapshot(row: &active_listing::Model, observed_at: DateTime<Utc>) -> Self;
  }
  impl TableRow for ListingEventRow  // "listing_events"
  #[repr(i8)] pub enum FloorChangeReason { Listing = 1, Refill = 2, Resync = 3 }
  pub struct FloorChangeRow { event_time, item_id, hq, world_id, price_per_unit, reason }
  impl FloorChangeRow { pub fn new(event_time: DateTime<Utc>, item_id: i32, hq: bool, world_id: i32, price_per_unit: i32, reason: FloorChangeReason) -> Self; }
  impl TableRow for FloorChangeRow   // "floor_changes"
  ```

- [ ] **Step 1: Add the dependency**

In `ultros-clickhouse/Cargo.toml` under `[dependencies]` add `serde_repr = "0.1"`. Run `cargo check -p ultros-clickhouse` once so `Cargo.lock` picks it up.

- [ ] **Step 2: Write the failing tests**

Append to `mod tests` in `rows.rs`:

```rust
    fn fixture_listing(price: i32, quantity: i32) -> ultros_db::entity::active_listing::Model {
        ultros_db::entity::active_listing::Model {
            id: 77,
            world_id: 40,
            item_id: 7,
            retainer_id: 12,
            price_per_unit: price,
            quantity,
            hq: true,
            timestamp: NaiveDate::from_ymd_opt(2026, 9, 1)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap(),
            listing_id: Some("123456789012345678".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn listing_event_from_updated_change_carries_previous_state() {
        let observed_at = chrono::Utc::now();
        let previous = fixture_listing(500, 3);
        let change = ultros_db::listings::ListingChange::upserted(
            fixture_listing(450, 2),
            Some(&previous),
            observed_at,
        );
        let row = ListingEventRow::from_change(&change, ListingEventSource::Websocket);
        assert_eq!(row.kind, ListingEventKind::Updated);
        assert_eq!(row.source, ListingEventSource::Websocket);
        assert_eq!(row.event_time, observed_at);
        assert_eq!(row.item_id, 7);
        assert_eq!(row.hq, 1);
        assert_eq!(row.world_id, 40);
        assert_eq!(row.listing_id, "123456789012345678");
        assert_eq!(row.pg_listing_id, 77);
        assert_eq!(row.retainer_id, 12);
        assert_eq!(row.price_per_unit, 450);
        assert_eq!(row.quantity, 2);
        assert_eq!(row.prev_price, 500);
        assert_eq!(row.prev_quantity, 3);
        assert_eq!(row.reviewed_at.naive_utc(), change.row.timestamp);
    }

    #[test]
    fn listing_event_from_added_or_removed_change_has_zero_previous_state() {
        let observed_at = chrono::Utc::now();
        let added = ultros_db::listings::ListingChange::added(fixture_listing(100, 1), observed_at);
        let row = ListingEventRow::from_change(&added, ListingEventSource::Catchup);
        assert_eq!(row.kind, ListingEventKind::Added);
        assert_eq!(row.prev_price, 0);
        assert_eq!(row.prev_quantity, 0);

        let removed = ultros_db::listings::ListingChange::removed(fixture_listing(100, 1), observed_at);
        let row = ListingEventRow::from_change(&removed, ListingEventSource::Manual);
        assert_eq!(row.kind, ListingEventKind::Removed);
        assert_eq!(row.price_per_unit, 100);
    }

    #[test]
    fn listing_event_uses_empty_listing_id_for_legacy_rows_and_clamps() {
        let observed_at = chrono::Utc::now();
        let mut legacy = fixture_listing(-5, 70_000);
        legacy.listing_id = None;
        let change = ultros_db::listings::ListingChange::added(legacy, observed_at);
        let row = ListingEventRow::from_change(&change, ListingEventSource::Websocket);
        assert_eq!(row.listing_id, "");
        assert_eq!(row.price_per_unit, 0);
        assert_eq!(row.quantity, u16::MAX);
    }

    #[test]
    fn listing_event_from_snapshot_is_an_added_snapshot_row() {
        let observed_at = chrono::Utc::now();
        let row = ListingEventRow::from_snapshot(&fixture_listing(300, 4), observed_at);
        assert_eq!(row.kind, ListingEventKind::Added);
        assert_eq!(row.source, ListingEventSource::Snapshot);
        assert_eq!(row.event_time, observed_at);
        assert_eq!(row.price_per_unit, 300);
    }

    #[test]
    fn floor_change_row_maps_hq_and_clamps_price() {
        let now = chrono::Utc::now();
        let row = FloorChangeRow::new(now, 7, true, 40, 250, FloorChangeReason::Refill);
        assert_eq!(row.hq, 1);
        assert_eq!(row.price_per_unit, 250);
        assert_eq!(row.reason, FloorChangeReason::Refill);
        let empty = FloorChangeRow::new(now, 7, false, 40, -1, FloorChangeReason::Resync);
        assert_eq!(empty.hq, 0);
        assert_eq!(empty.price_per_unit, 0);
    }

    #[test]
    fn table_names_match_the_schema() {
        assert_eq!(SaleRow::TABLE, "sales");
        assert_eq!(ListingEventRow::TABLE, "listing_events");
        assert_eq!(FloorChangeRow::TABLE, "floor_changes");
    }
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p ultros-clickhouse rows 2>&1 | tail -5`
Expected: compile errors (types undefined).

- [ ] **Step 4: Implement**

At the top of `rows.rs`, extend the imports:

```rust
use chrono::{DateTime, Utc};
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use ultros_db::{
    entity::active_listing,
    listings::{ListingChange, ListingChangeKind},
};
```

After the `SaleRow` impl block add:

```rust
/// A ClickHouse table a [`crate::writer::Writer`] can insert into.
///
/// The client validates each insert against the live table schema
/// (`RowBinaryWithNamesAndTypes`), so a struct's field *names* must match the
/// table's column names exactly, in addition to the types.
pub trait TableRow: Row + Serialize + Send + Sync + 'static {
    const TABLE: &'static str;
}

impl TableRow for SaleRow {
    const TABLE: &'static str = "sales";
}

/// `listing_events.kind`. Values are the Enum8 codes in the DDL.
#[derive(Serialize_repr, Deserialize_repr, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i8)]
pub enum ListingEventKind {
    Added = 1,
    Updated = 2,
    Removed = 3,
}

impl From<ListingChangeKind> for ListingEventKind {
    fn from(kind: ListingChangeKind) -> Self {
        match kind {
            ListingChangeKind::Added => Self::Added,
            ListingChangeKind::Updated => Self::Updated,
            ListingChangeKind::Removed => Self::Removed,
        }
    }
}

/// `listing_events.source`: which ingest path observed the change.
#[derive(Serialize_repr, Deserialize_repr, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i8)]
pub enum ListingEventSource {
    /// The Universalis websocket listener.
    Websocket = 1,
    /// `UpdateService` catch-up and full sweeps (REST boards).
    Catchup = 2,
    /// The `/item/refresh/{world}/{item}` route.
    Manual = 3,
    /// The one-time seed from the current `active_listing` table.
    Snapshot = 4,
}

/// Mirrors the `listing_events` table. Append-only; one row per observed
/// change to `active_listing`. See the 2026-09-07 listing-history spec.
#[derive(Row, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ListingEventRow {
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub event_time: DateTime<Utc>,
    pub kind: ListingEventKind,
    pub source: ListingEventSource,
    pub item_id: i32,
    pub hq: u8,
    pub world_id: i32,
    /// Universalis' listing identity; empty for rows stored before the
    /// identity migration.
    pub listing_id: String,
    /// `active_listing.id`, so a legacy add/remove pair can still be joined.
    pub pg_listing_id: i32,
    pub retainer_id: i32,
    pub price_per_unit: u32,
    pub quantity: u16,
    /// 0 unless `kind == Updated`.
    pub prev_price: u32,
    /// 0 unless `kind == Updated`.
    pub prev_quantity: u16,
    /// Universalis `last_review_time`, distinct from our observation time.
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub reviewed_at: DateTime<Utc>,
}

impl ListingEventRow {
    pub fn from_change(change: &ListingChange, source: ListingEventSource) -> Self {
        let row = &change.row;
        Self {
            event_time: change.observed_at,
            kind: change.kind.into(),
            source,
            item_id: row.item_id,
            hq: row.hq as u8,
            world_id: row.world_id,
            listing_id: row.listing_id.clone().unwrap_or_default(),
            pg_listing_id: row.id,
            retainer_id: row.retainer_id,
            price_per_unit: clamp_price(row.price_per_unit),
            quantity: clamp_qty(row.quantity),
            prev_price: change.prev_price_per_unit.map(clamp_price).unwrap_or(0),
            prev_quantity: change.prev_quantity.map(clamp_qty).unwrap_or(0),
            reviewed_at: DateTime::from_naive_utc_and_offset(row.timestamp, Utc),
        }
    }

    /// A seed row: the listing was on the board when recording started.
    pub fn from_snapshot(row: &active_listing::Model, observed_at: DateTime<Utc>) -> Self {
        Self::from_change(
            &ListingChange::added(row.clone(), observed_at),
            ListingEventSource::Snapshot,
        )
    }
}

impl TableRow for ListingEventRow {
    const TABLE: &'static str = "listing_events";
}

/// `floor_changes.reason`: which analyzer path moved the floor.
#[derive(Serialize_repr, Deserialize_repr, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i8)]
pub enum FloorChangeReason {
    /// A listing event lowered (or created) the floor.
    Listing = 1,
    /// The cheapest listing was removed and the floor was re-read from Postgres.
    Refill = 2,
    /// A full rebuild of the map from Postgres (boot, or after bus lag) found a
    /// different value than the map held.
    Resync = 3,
}

/// Mirrors the `floor_changes` table: one row per transition of the
/// world-level lowest listing price for an `(item, hq)`. `price_per_unit = 0`
/// means the board emptied.
#[derive(Row, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FloorChangeRow {
    #[serde(with = "clickhouse::serde::chrono::datetime")]
    pub event_time: DateTime<Utc>,
    pub item_id: i32,
    pub hq: u8,
    pub world_id: i32,
    pub price_per_unit: u32,
    pub reason: FloorChangeReason,
}

impl FloorChangeRow {
    pub fn new(
        event_time: DateTime<Utc>,
        item_id: i32,
        hq: bool,
        world_id: i32,
        price_per_unit: i32,
        reason: FloorChangeReason,
    ) -> Self {
        Self {
            event_time,
            item_id,
            hq: hq as u8,
            world_id,
            price_per_unit: clamp_price(price_per_unit),
            reason,
        }
    }
}

impl TableRow for FloorChangeRow {
    const TABLE: &'static str = "floor_changes";
}

/// Prices are domain-constrained non-negative; the Postgres column is `i32`.
fn clamp_price(p: i32) -> u32 {
    p.max(0) as u32
}
```

Replace the two existing `.max(0) as u32` price conversions in `SaleRow::from_db_model`/`from_api_sale` with `clamp_price(...)`.

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -p ultros-clickhouse rows 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add ultros-clickhouse/Cargo.toml Cargo.lock ultros-clickhouse/src/rows.rs
git commit -m "feat(clickhouse): ListingEventRow and FloorChangeRow with a TableRow trait

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: `ultros-clickhouse` schema — three tables + smoke test

**Files:**
- Modify: `ultros-clickhouse/src/schema.rs` (`apply` at line 14; append three fns)
- Create: `ultros-clickhouse/tests/listing_events_smoke.rs`

**Interfaces:**
- Consumes: `ListingEventRow`, `FloorChangeRow` (Task 3).
- Produces: tables `listing_events`, `floor_changes`, `_listing_events_seed`; `pub const LISTING_EVENTS_SEED_MARKER_TABLE: &str = "_listing_events_seed";`

- [ ] **Step 1: Write the failing smoke test**

Create `ultros-clickhouse/tests/listing_events_smoke.rs`:

```rust
//! Round-trips the listing-history tables against a live ClickHouse.
//!
//! Run with:
//!   ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test listing_events_smoke -- --nocapture

use chrono::Utc;
use ultros_clickhouse::{
    ClickHouseClient,
    rows::{FloorChangeReason, FloorChangeRow, ListingEventKind, ListingEventRow, ListingEventSource},
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
        .query(&format!("SELECT count() AS n FROM {table} WHERE item_id = ?"))
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
    let mut insert = ch.client().insert::<ListingEventRow>("listing_events").await.expect("insert");
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
    let mut insert = ch.client().insert::<FloorChangeRow>("floor_changes").await.expect("insert");
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
```

- [ ] **Step 2: Run it to verify it fails**

Run: `ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test listing_events_smoke 2>&1 | tail -10`
Expected: FAIL with `Table ultros.listing_events does not exist` (or, with no ClickHouse reachable, a connection error — in that case rely on the unit tests and note it in the PR).

- [ ] **Step 3: Add the DDL**

In `schema::apply`, after `apply_item_category_map(client).await?;`:

```rust
    apply_listing_events_table(client).await?;
    apply_floor_changes_table(client).await?;
    apply_listing_events_seed_marker(client).await?;
```

Append to `schema.rs`:

```rust
/// Name of the one-row marker table that records the `listing_events` seed.
pub const LISTING_EVENTS_SEED_MARKER_TABLE: &str = "_listing_events_seed";

/// Append-only log of every change Ultros observes to `active_listing`.
///
/// Plain `MergeTree`: a re-sent listing whose state already matches is
/// filtered by the Postgres diff before any write, so duplicates never reach
/// this table and no dedup engine is needed. `ORDER BY (item, hq, world,
/// time)` serves the "what happened to this item on this world" reads that
/// every planned consumer starts from; monthly partitions plus the TTL keep
/// retention a one-line change while volume is still unmeasured.
async fn apply_listing_events_table(client: &Client) -> Result<(), ClickHouseError> {
    client
        .query(
            r#"
            CREATE TABLE IF NOT EXISTS listing_events (
                event_time      DateTime,
                kind            Enum8('added' = 1, 'updated' = 2, 'removed' = 3),
                source          Enum8('websocket' = 1, 'catchup' = 2, 'manual' = 3, 'snapshot' = 4),
                item_id         Int32,
                hq              UInt8,
                world_id        Int32,
                listing_id      String,
                pg_listing_id   Int32,
                retainer_id     Int32,
                price_per_unit  UInt32,
                quantity        UInt16,
                prev_price      UInt32,
                prev_quantity   UInt16,
                reviewed_at     DateTime
            )
            ENGINE = MergeTree
            PARTITION BY toYYYYMM(event_time)
            ORDER BY (item_id, hq, world_id, event_time)
            TTL event_time + INTERVAL 365 DAY
            SETTINGS index_granularity = 8192
            "#,
        )
        .execute()
        .await?;
    Ok(())
}

/// One row per transition of the analyzer's world-level lowest listing price
/// for an `(item, hq)`. `price_per_unit = 0` means the board emptied.
/// Tiny (one row per floor move) and the long-term series, so no TTL.
async fn apply_floor_changes_table(client: &Client) -> Result<(), ClickHouseError> {
    client
        .query(
            r#"
            CREATE TABLE IF NOT EXISTS floor_changes (
                event_time      DateTime,
                item_id         Int32,
                hq              UInt8,
                world_id        Int32,
                price_per_unit  UInt32,
                reason          Enum8('listing' = 1, 'refill' = 2, 'resync' = 3)
            )
            ENGINE = MergeTree
            PARTITION BY toYYYYMM(event_time)
            ORDER BY (item_id, hq, world_id, event_time)
            SETTINGS index_granularity = 8192
            "#,
        )
        .execute()
        .await?;
    Ok(())
}

/// Marker written once the `listing_events` seed has streamed every current
/// `active_listing` row. Modelled on `_backfill_state`.
async fn apply_listing_events_seed_marker(client: &Client) -> Result<(), ClickHouseError> {
    client
        .query(&format!(
            r#"
            CREATE TABLE IF NOT EXISTS {LISTING_EVENTS_SEED_MARKER_TABLE} (
                seeded_at     DateTime,
                rows_streamed UInt64
            )
            ENGINE = ReplacingMergeTree(seeded_at)
            ORDER BY tuple()
            "#
        ))
        .execute()
        .await?;
    Ok(())
}
```

- [ ] **Step 4: Run the smoke test**

Run: `ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test listing_events_smoke 2>&1 | tail -10`
Expected: both tests PASS. Also run the existing `schema_smoke` to prove nothing regressed.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-clickhouse/src/schema.rs ultros-clickhouse/tests/listing_events_smoke.rs
git commit -m "feat(clickhouse): listing_events, floor_changes and seed marker tables

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Generic `Writer<R: TableRow>` + `insert_all`

**Files:**
- Modify: `ultros-clickhouse/src/writer.rs` (whole file — every `SaleRow` becomes the type parameter)
- Modify: `ultros/src/analyzer_service.rs:437,463` and the five `Writer::disabled()` test sites (~2755, 2802, 2834, 2851, 2903)
- Modify: `ultros/src/main.rs:88,636`

**Interfaces:**
- Produces:
  ```rust
  pub struct Writer<R: TableRow> { .. }           // Clone, same methods as today
  impl<R: TableRow> Writer<R> { spawn, spawn_recovering, spawn_with_config, wait_ready, send(&self, row: R), shutdown, disabled }
  pub async fn insert_all<R: TableRow>(client: &ClickHouseClient, rows: &[R], chunk: usize) -> Result<u64, ClickHouseError>;
  ```
  Metrics: every `ultros_clickhouse_writer_*` metric gains `"table" => R::TABLE`.

- [ ] **Step 1: Write the failing test**

Append to `mod tests` in `writer.rs`:

```rust
    #[tokio::test]
    async fn run_writer_is_generic_over_the_row_type() {
        use crate::rows::{FloorChangeReason, FloorChangeRow};
        let (tx, rx) = mpsc::channel(4);
        let (batch_tx, mut batches) = mpsc::unbounded_channel();
        let token = CancellationToken::new();
        for i in 0..3 {
            tx.try_send(FloorChangeRow::new(
                chrono::Utc::now(),
                i,
                false,
                40,
                100 + i,
                FloorChangeReason::Listing,
            ))
            .unwrap();
        }
        token.cancel();
        run_writer(rx, token, 2, Duration::from_secs(60), move |rows: &[FloorChangeRow]| {
            batch_tx
                .send(rows.iter().map(|r| r.item_id).collect::<Vec<_>>())
                .unwrap();
            Box::pin(async { Ok(()) })
        })
        .await;
        assert_eq!(batches.recv().await.unwrap(), vec![0, 1]);
        assert_eq!(batches.recv().await.unwrap(), vec![2]);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ultros-clickhouse writer 2>&1 | tail -5`
Expected: type error (`run_writer` expects `SaleRow`).

- [ ] **Step 3: Make the writer generic**

Edit `writer.rs`:

Imports: `use crate::{ClickHouseClient, ClickHouseError, rows::TableRow};` (drop the direct `SaleRow` import; the tests module keeps `use crate::rows::SaleRow;`).

Struct and Clone:
```rust
/// Cheap handle to the bounded writer. Clones share the task and shutdown.
pub struct Writer<R: TableRow> {
    tx: mpsc::Sender<R>,
    token: CancellationToken,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    ready: watch::Receiver<bool>,
}

// Manual impl: a derive would demand `R: Clone`, which rows need not be.
impl<R: TableRow> Clone for Writer<R> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            token: self.token.clone(),
            task: self.task.clone(),
            ready: self.ready.clone(),
        }
    }
}

impl<R: TableRow> Writer<R> {
```
Inside: `send(&self, row: R)`; in `send`, the dropped-rows counter becomes `metrics::counter!("ultros_clickhouse_writer_dropped_rows_total", "reason" => reason, "table" => R::TABLE)`. In `spawn_inner`, the flush closure is `move |rows| { let client = client.clone(); Box::pin(async move { flush::<R>(&client, rows).await }) }` and the early-exit is `record_unflushed(rx.len(), R::TABLE)`.

Free functions:
```rust
async fn run_writer<R, F>(
    mut rx: mpsc::Receiver<R>,
    token: CancellationToken,
    batch_size: usize,
    flush_interval: Duration,
    mut insert: F,
) where
    R: TableRow,
    F: for<'a> FnMut(&'a [R]) -> BoxFuture<'a, Result<(), ClickHouseError>>,
```
Replace the two `metrics::gauge!("ultros_clickhouse_writer_queued_rows")` with `metrics::gauge!("ultros_clickhouse_writer_queued_rows", "table" => R::TABLE)`; `record_unflushed(buf.len() + rx.len(), R::TABLE)`; the exit log becomes `info!(table = R::TABLE, "ClickHouse writer task exiting")`.

```rust
async fn try_flush<R, F>(buf: &mut Vec<R>, insert: &mut F) -> bool
where
    R: TableRow,
    F: for<'a> FnMut(&'a [R]) -> BoxFuture<'a, Result<(), ClickHouseError>>,
```
with `"table" => R::TABLE` on `written_rows_total` and `flush_failures_total`, and `table = R::TABLE` in the warn.

```rust
fn record_unflushed(rows: usize, table: &'static str) {
    if rows != 0 {
        metrics::counter!("ultros_clickhouse_writer_dropped_rows_total", "reason" => "shutdown_unflushed", "table" => table).increment(rows as u64);
        warn!(rows, table, "ClickHouse shutdown left unflushed rows; Postgres backfill required");
    }
}

async fn flush<R: TableRow>(client: &ClickHouseClient, rows: &[R]) -> Result<(), ClickHouseError> {
    let mut insert = client.client().insert::<R>(R::TABLE).await?;
    for row in rows {
        insert.write(row).await?;
    }
    insert.end().await?;
    debug!(rows = rows.len(), table = R::TABLE, "ClickHouse flush");
    Ok(())
}

/// One-shot bulk insert for producers that bypass the queue (the seed, the
/// analyzer's resync diff). Each chunk is one INSERT, so a failure loses at
/// most `chunk` rows and the caller decides whether to retry.
pub async fn insert_all<R: TableRow>(
    client: &ClickHouseClient,
    rows: &[R],
    chunk: usize,
) -> Result<u64, ClickHouseError> {
    let mut written = 0u64;
    for part in rows.chunks(chunk.max(1)) {
        flush(client, part).await?;
        written += part.len() as u64;
    }
    Ok(written)
}
```

In the existing tests: `Writer::disabled()` in `readiness_wait_returns_false_when_initialization_exits` becomes `Writer::<SaleRow>::disabled()`. The struct literal in `shutdown_waits_for_in_flight_acknowledgement` infers `R` from `tx`.

- [ ] **Step 4: Update the two consumers' type references**

`ultros/src/analyzer_service.rs`: field and parameter `ch_writer: ultros_clickhouse::writer::Writer<ultros_clickhouse::rows::SaleRow>`; the five test constructors `ch_writer: ultros_clickhouse::writer::Writer::disabled()` need no change (inferred from the field type).

`ultros/src/main.rs:88`: `writer: ultros_clickhouse::writer::Writer<ultros_clickhouse::rows::SaleRow>`. Line 636's `Writer::spawn_recovering(...)` infers from its use — if it does not, write `Writer::<SaleRow>::spawn_recovering`.

- [ ] **Step 5: Run tests and check**

```bash
cargo test -p ultros-clickhouse writer 2>&1 | tail -5
cargo check -p ultros 2>&1 | tail -5
```
Expected: PASS / success.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add ultros-clickhouse/src/writer.rs ultros/src/analyzer_service.rs ultros/src/main.rs
git commit -m "refactor(clickhouse): make the bounded Writer generic over its row type

Adds a table label to the writer metrics and an insert_all bulk helper.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: `listing_seed` — one-time seed from `active_listing`

**Files:**
- Create: `ultros-clickhouse/src/listing_seed.rs`
- Modify: `ultros-clickhouse/src/lib.rs:16-22` (add `pub mod listing_seed;`)
- Modify: `ultros-clickhouse/tests/listing_events_smoke.rs` (add marker test)

**Interfaces:**
- Consumes: `UltrosDb::stream_active_listings` (Task 2), `ListingEventRow::from_snapshot` (Task 3), `insert_all` (Task 5), `LISTING_EVENTS_SEED_MARKER_TABLE` (Task 4).
- Produces:
  ```rust
  pub const SEED_CHUNK: usize = 10_000;
  pub enum SeedOutcome { AlreadySeeded, Seeded { rows: u64 } }
  pub async fn seed_listing_events(ch: &ClickHouseClient, pg: &UltrosDb) -> Result<SeedOutcome, ClickHouseError>;
  pub async fn already_seeded(ch: &ClickHouseClient) -> Result<bool, ClickHouseError>;
  pub async fn mark_seeded(ch: &ClickHouseClient, rows: u64) -> Result<(), ClickHouseError>;
  ```

- [ ] **Step 1: Write the failing smoke test**

Append to `tests/listing_events_smoke.rs`:

```rust
#[tokio::test]
async fn seed_marker_round_trip() {
    if !integration_enabled() {
        eprintln!("skipped: set ULTROS_CH_INTEGRATION=1 to run");
        return;
    }
    use ultros_clickhouse::listing_seed::{already_seeded, mark_seeded};
    use ultros_clickhouse::schema::LISTING_EVENTS_SEED_MARKER_TABLE;
    let ch = client().await;
    ch.client()
        .query(&format!("TRUNCATE TABLE {LISTING_EVENTS_SEED_MARKER_TABLE}"))
        .execute()
        .await
        .expect("truncate marker");

    assert!(!already_seeded(&ch).await.expect("query"));
    mark_seeded(&ch, 42).await.expect("mark");
    assert!(already_seeded(&ch).await.expect("query"));
    // Leave the marker cleared so a dev box's leader still runs the real seed.
    ch.client()
        .query(&format!("TRUNCATE TABLE {LISTING_EVENTS_SEED_MARKER_TABLE}"))
        .execute()
        .await
        .expect("truncate marker");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test listing_events_smoke seed_marker 2>&1 | tail -5`
Expected: compile error, `listing_seed` module missing.

- [ ] **Step 3: Implement the module**

Create `ultros-clickhouse/src/listing_seed.rs`:

```rust
//! One-time seed of `listing_events` from the current Postgres board.
//!
//! The table starts empty at deploy, so a listing already on the board that
//! never changes would emit no row and a floor replayed from events would read
//! too high until it was finally removed. Streaming every `active_listing`
//! row once as `kind = 'added', source = 'snapshot'` makes the alive set
//! complete from day one. Guarded by a marker table so it runs exactly once
//! per ClickHouse database.

use chrono::Utc;
use futures::TryStreamExt;
use tracing::{info, warn};
use ultros_db::UltrosDb;

use crate::{
    ClickHouseClient, ClickHouseError, rows::ListingEventRow,
    schema::LISTING_EVENTS_SEED_MARKER_TABLE, writer::insert_all,
};

/// Rows per INSERT while streaming the board.
pub const SEED_CHUNK: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedOutcome {
    AlreadySeeded,
    Seeded { rows: u64 },
}

/// Streams `active_listing` into `listing_events` unless the marker says it
/// already happened. Safe to call repeatedly; safe to retry after a failure
/// (a partial run's rows are discarded before streaming again).
pub async fn seed_listing_events(
    ch: &ClickHouseClient,
    pg: &UltrosDb,
) -> Result<SeedOutcome, ClickHouseError> {
    if already_seeded(ch).await? {
        return Ok(SeedOutcome::AlreadySeeded);
    }
    // A previous attempt may have died mid-stream. Only the seed writes
    // `source = 'snapshot'`, and the marker exists only after a full success,
    // so anything tagged snapshot at this point is a torn run.
    ch.client()
        .query("ALTER TABLE listing_events DELETE WHERE source = 'snapshot' SETTINGS mutations_sync = 1")
        .execute()
        .await?;

    let observed_at = Utc::now();
    let mut stream = pg
        .stream_active_listings()
        .await
        .map_err(|e| ClickHouseError::Backfill(e.to_string()))?;
    let mut buf: Vec<ListingEventRow> = Vec::with_capacity(SEED_CHUNK);
    let mut rows = 0u64;
    while let Some(model) = stream
        .try_next()
        .await
        .map_err(|e| ClickHouseError::Backfill(e.to_string()))?
    {
        buf.push(ListingEventRow::from_snapshot(&model, observed_at));
        if buf.len() == SEED_CHUNK {
            rows += insert_all(ch, &buf, SEED_CHUNK).await?;
            buf.clear();
        }
    }
    if !buf.is_empty() {
        rows += insert_all(ch, &buf, SEED_CHUNK).await?;
    }
    mark_seeded(ch, rows).await?;
    info!(rows, "seeded listing_events from active_listing");
    Ok(SeedOutcome::Seeded { rows })
}

pub async fn already_seeded(ch: &ClickHouseClient) -> Result<bool, ClickHouseError> {
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Found {
        n: u8,
    }
    let found: Found = ch
        .client()
        .query(&format!(
            "SELECT count() > 0 AS n FROM {LISTING_EVENTS_SEED_MARKER_TABLE}"
        ))
        .fetch_one()
        .await?;
    Ok(found.n != 0)
}

pub async fn mark_seeded(ch: &ClickHouseClient, rows: u64) -> Result<(), ClickHouseError> {
    #[derive(serde::Serialize, clickhouse::Row)]
    struct MarkerRow {
        #[serde(with = "clickhouse::serde::chrono::datetime")]
        seeded_at: chrono::DateTime<Utc>,
        rows_streamed: u64,
    }
    let mut insert = ch
        .client()
        .insert::<MarkerRow>(LISTING_EVENTS_SEED_MARKER_TABLE)
        .await?;
    insert
        .write(&MarkerRow {
            seeded_at: Utc::now(),
            rows_streamed: rows,
        })
        .await?;
    insert.end().await?;
    Ok(())
}

/// Leader-side driver: keeps trying until the seed exists or the token fires.
/// Failures are spaced ten minutes apart so a ClickHouse outage at boot does
/// not turn into a hot loop.
pub async fn run_until_seeded(
    ch: ClickHouseClient,
    pg: UltrosDb,
    token: tokio_util::sync::CancellationToken,
) {
    loop {
        match seed_listing_events(&ch, &pg).await {
            Ok(SeedOutcome::AlreadySeeded) => return,
            Ok(SeedOutcome::Seeded { .. }) => return,
            Err(error) => {
                metrics::counter!("ultros_listing_events_seed_failures_total").increment(1);
                warn!(?error, "listing_events seed failed; retrying in 10 minutes");
            }
        }
        tokio::select! {
            _ = token.cancelled() => return,
            _ = tokio::time::sleep(std::time::Duration::from_secs(600)) => {}
        }
    }
}
```

Add `pub mod listing_seed;` to `lib.rs` (alphabetical, between `backfill` and `quality_filter`).

- [ ] **Step 4: Run the smoke test and the crate's unit tests**

```bash
ULTROS_CH_INTEGRATION=1 cargo test -p ultros-clickhouse --test listing_events_smoke 2>&1 | tail -8
cargo test -p ultros-clickhouse 2>&1 | tail -5
```
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add ultros-clickhouse/src/listing_seed.rs ultros-clickhouse/src/lib.rs ultros-clickhouse/tests/listing_events_smoke.rs
git commit -m "feat(clickhouse): one-time listing_events seed from active_listing

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Wire `Writer<ListingEventRow>` through ingest

**Files:**
- Modify: `ultros/src/main.rs` (`run_socket_listener` 203-306; writer construction 600-660; `UpdateService {}` 657; `WebState {}` 739; shutdown 775-790; rollup leader 124-131)
- Modify: `ultros/src/item_update_service.rs:135-150` (struct), `:732-750` (emit)
- Modify: `ultros/src/web/state.rs` (field + `FromRef`), `ultros/src/web.rs:1240-1305` (route)

**Interfaces:**
- Consumes: `Writer<ListingEventRow>`, `ListingEventRow::from_change`, `ListingEventSource`, `listing_seed::run_until_seeded`, `ListingWrite.changes`.
- Produces: `UpdateService.listing_events: Writer<ListingEventRow>`; `WebState.listing_events: Writer<ListingEventRow>` with `impl FromRef<WebState> for Writer<ListingEventRow>`.

- [ ] **Step 1: Construct the writers before the socket spawn**

In `main.rs`, move the block

```rust
    let ch_client = ultros_clickhouse::ClickHouseClient::from_env();
    let ch_writer = ultros_clickhouse::writer::Writer::spawn_recovering(
        ch_client.clone(),
        CancellationToken::new(),
    );
```
to directly after `let token = CancellationToken::new();` (~line 606, before `let socket_token = token.clone();`) and extend it:

```rust
    // Migration retries in the writers so an outage at startup does not disable
    // analytics for the lifetime of this process. Keep their cancellation
    // separate so producers can finish sending before the final flush. Three
    // writers, one per table: sales (analyzer), listing changes (every ingest
    // path), and floor moves (analyzer). The migrate each runs is idempotent.
    let ch_client = ultros_clickhouse::ClickHouseClient::from_env();
    let ch_writer = ultros_clickhouse::writer::Writer::<ultros_clickhouse::rows::SaleRow>::spawn_recovering(
        ch_client.clone(),
        CancellationToken::new(),
    );
    let listing_events_writer =
        ultros_clickhouse::writer::Writer::<ultros_clickhouse::rows::ListingEventRow>::spawn_recovering(
            ch_client.clone(),
            CancellationToken::new(),
        );
    let floor_writer =
        ultros_clickhouse::writer::Writer::<ultros_clickhouse::rows::FloorChangeRow>::spawn_recovering(
            ch_client.clone(),
            CancellationToken::new(),
        );
    let socket_listing_events = listing_events_writer.clone();
```

Pass `socket_listing_events` into the socket spawn: `run_socket_listener(init, listings_sender, history_sender, socket_listing_events, socket_token).await;`. (`floor_writer` is consumed in Task 8; until then add `let _ = &floor_writer;` is NOT acceptable — instead do Task 8's `start_analyzer` parameter change in this task's Step 6 to keep the build warning-free.)

- [ ] **Step 2: Socket listener emits websocket rows**

Signature:
```rust
async fn run_socket_listener(
    db: UltrosDb,
    listings_tx: EventProducer<ListingEventData>,
    sales_tx: EventProducer<SaleEventData>,
    listing_events: ultros_clickhouse::writer::Writer<ultros_clickhouse::rows::ListingEventRow>,
    token: CancellationToken,
) {
```
Beside `let sales_tx = sales_tx.clone();` add `let listing_events = listing_events.clone();`. In the `ListingsAdd` arm, right after `Ok(write) => {`:
```rust
                            record_listing_changes(&listing_events, &write.changes, ultros_clickhouse::rows::ListingEventSource::Websocket);
```
Same line in the `ListingsRemove` arm's `Ok(write) => {`.

Add a free function near `run_socket_listener` (it is reused by the other two producers):

```rust
/// Mirror a write path's change list into ClickHouse. Non-blocking: overflow
/// is counted by the writer, never felt by ingest.
pub(crate) fn record_listing_changes(
    writer: &ultros_clickhouse::writer::Writer<ultros_clickhouse::rows::ListingEventRow>,
    changes: &[ultros_db::listings::ListingChange],
    source: ultros_clickhouse::rows::ListingEventSource,
) {
    for change in changes {
        writer.send(ultros_clickhouse::rows::ListingEventRow::from_change(change, source));
    }
}
```

- [ ] **Step 3: `UpdateService` emits catch-up rows**

`item_update_service.rs` struct: add after `sales`:
```rust
    /// ClickHouse `listing_events` mirror for every board this service writes.
    pub(crate) listing_events:
        ultros_clickhouse::writer::Writer<ultros_clickhouse::rows::ListingEventRow>,
```
`main.rs` `UpdateService { ... }` literal: add `listing_events: listing_events_writer.clone(),`.
In `check_items` (~732): `Ok(ListingWrite { added, removed, changes }) => { listings_changed = ...; crate::record_listing_changes(&self.listing_events, &changes, ultros_clickhouse::rows::ListingEventSource::Catchup); ...`. (If `record_listing_changes` lives in `main.rs`, it is reachable as `crate::record_listing_changes`.)

- [ ] **Step 4: Manual refresh route emits rows**

`web/state.rs`: add field
```rust
    /// ClickHouse `listing_events` mirror for the manual refresh route.
    pub(crate) listing_events:
        ultros_clickhouse::writer::Writer<ultros_clickhouse::rows::ListingEventRow>,
```
and
```rust
impl FromRef<WebState> for ultros_clickhouse::writer::Writer<ultros_clickhouse::rows::ListingEventRow> {
    fn from_ref(input: &WebState) -> Self {
        input.listing_events.clone()
    }
}
```
`main.rs` `WebState { ... }` literal: add `listing_events: listing_events_writer.clone(),`.
`web.rs` `refresh_world_item_listings`: add parameter `State(listing_events): State<ultros_clickhouse::writer::Writer<ultros_clickhouse::rows::ListingEventRow>>,` and inside the loop:
```rust
            let ultros_db::listings::ListingWrite { added, removed, changes } = db
                .update_listings(listings, ItemId(item_id), WorldId(world_id as i32))
                .await?;
            crate::record_listing_changes(&listing_events, &changes, ultros_clickhouse::rows::ListingEventSource::Manual);
```
The route body runs inside a spawned future that captures its arguments; clone `listing_events` into it the same way `db`/`senders` are.

- [ ] **Step 5: Seed task in the rollup leader; shutdown order**

In `spawn_rollup_scheduler` (`main.rs`), immediately after `info!("acquired ClickHouse rollup scheduler lease");`:
```rust
                    // One-time listing_events seed, under the same lease so
                    // only one replica ever streams the board.
                    tokio::spawn(ultros_clickhouse::listing_seed::run_until_seeded(
                        ch.clone(),
                        db.clone(),
                        token.child_token(),
                    ));
```
(`db` and `ch` are already parameters of `spawn_rollup_scheduler`. Move this line *below* `let scheduler_token = token.child_token();` and use `scheduler_token.clone()` instead of `token.child_token()` so the seed stops when the lease is lost.)

Shutdown (~line 777): change `drain_analytics` to
```rust
        let drain_analytics = async {
            if let Err(e) = analyzer_shutdown.await {
                error!("Analyzer shutdown failed: {e:?}");
            }
            ch_writer.shutdown().await;
            floor_writer.shutdown().await;
        };
```
and after `tokio::join!(drain_analytics, drain_web);` add
```rust
        // Every listing_events producer (socket task, update service, web) has
        // been cancelled or drained by now.
        listing_events_writer.shutdown().await;
```

- [ ] **Step 6: Thread `floor_writer` into the analyzer (signature only)**

`AnalyzerService::start_analyzer` gains a parameter after `ch_writer`:
```rust
        floor_writer: ultros_clickhouse::writer::Writer<ultros_clickhouse::rows::FloorChangeRow>,
```
stored in a new field `floor_writer` on `AnalyzerService` (add it to the struct, the `Debug` impl's `.field("floor_writer", &"<Writer>")`, the `Self { .. }` in `start_analyzer`, and the five test constructors as `floor_writer: ultros_clickhouse::writer::Writer::disabled(),`). `main.rs` passes `floor_writer.clone()`. Task 8 uses it.

- [ ] **Step 7: Build and run the `ultros` unit tests**

```bash
cargo check -p ultros 2>&1 | tail -10
CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros --bin ultros analyzer 2>&1 | tail -5
```
Expected: success / PASS. (The `CARGO_PROFILE_DEV_DEBUG=0` is what lets the `ultros` bin tests link on Windows.)

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
git add ultros/src/main.rs ultros/src/item_update_service.rs ultros/src/web.rs ultros/src/web/state.rs ultros/src/analyzer_service.rs
git commit -m "feat: record every listing change into ClickHouse listing_events

Websocket, catch-up and manual refresh paths mirror their change lists
through a bounded writer; the rollup leader runs the one-time seed.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Analyzer emits `floor_changes`

**Files:**
- Modify: `ultros/src/analyzer_service.rs` (`CheapestListings::add_listing` ~268; `rebuild_cheapest_from_db` ~762; `add_listings` ~1647; `remove_from_selector` ~1768-1830; tests ~2565)

**Interfaces:**
- Consumes: `floor_writer` field (Task 7), `FloorChangeRow::new`, `FloorChangeReason`, `insert_all`, `Writer::wait_ready`.
- Produces:
  ```rust
  impl CheapestListings { fn add_listing<'a, T>(&mut self, listing: &'a T) -> Option<CheapestListingValue> }  // Some(new value) iff the map changed
  fn floor_diff(old: &BTreeMap<ItemKey, CheapestListingValue>, new: &BTreeMap<ItemKey, CheapestListingValue>, world_id: i32, at: DateTime<Utc>) -> Vec<FloorChangeRow>;
  ```

- [ ] **Step 1: Write the failing tests**

In the `mod tests` that holds `cheapest_listings_add_listing_keeps_cheapest_for_item` (~2565), add:

```rust
    #[test]
    fn add_listing_reports_a_change_only_when_the_floor_moves_down_or_appears() {
        let mut cheapest = CheapestListings::default();
        let make = |price| ultros_db::listings::ListingSummary {
            item_id: 7,
            world_id: 1,
            price_per_unit: price,
            hq: false,
        };
        assert_eq!(cheapest.add_listing(&make(500)).map(|v| v.price), Some(500), "first listing creates the floor");
        assert_eq!(cheapest.add_listing(&make(700)).map(|v| v.price), None, "a dearer listing changes nothing");
        assert_eq!(cheapest.add_listing(&make(500)).map(|v| v.price), None, "an equal price is not a change");
        assert_eq!(cheapest.add_listing(&make(450)).map(|v| v.price), Some(450), "a cheaper listing lowers the floor");
    }

    #[test]
    fn floor_diff_reports_appearances_disappearances_and_price_changes_only() {
        let key = |item_id, hq| ItemKey { item_id, hq };
        let val = |price| CheapestListingValue { price, world_id: 40 };
        let old: BTreeMap<ItemKey, CheapestListingValue> = [
            (key(1, false), val(100)), // unchanged
            (key(2, false), val(200)), // price moves
            (key(3, true), val(300)),  // disappears
        ]
        .into_iter()
        .collect();
        let new: BTreeMap<ItemKey, CheapestListingValue> = [
            (key(1, false), val(100)),
            (key(2, false), val(250)),
            (key(4, false), val(400)), // appears
        ]
        .into_iter()
        .collect();
        let at = Utc::now();

        let mut rows = floor_diff(&old, &new, 40, at);
        rows.sort_by_key(|r| r.item_id);

        let summary: Vec<(i32, u8, u32)> = rows.iter().map(|r| (r.item_id, r.hq, r.price_per_unit)).collect();
        assert_eq!(summary, vec![(2, 0, 250), (3, 1, 0), (4, 0, 400)]);
        assert!(rows.iter().all(|r| r.reason == ultros_clickhouse::rows::FloorChangeReason::Resync));
        assert!(rows.iter().all(|r| r.world_id == 40 && r.event_time == at));
    }

    #[test]
    fn floor_diff_from_an_empty_map_emits_every_key() {
        let new: BTreeMap<ItemKey, CheapestListingValue> = [
            (ItemKey { item_id: 1, hq: false }, CheapestListingValue { price: 10, world_id: 40 }),
            (ItemKey { item_id: 1, hq: true }, CheapestListingValue { price: 20, world_id: 40 }),
        ]
        .into_iter()
        .collect();
        let rows = floor_diff(&BTreeMap::new(), &new, 40, Utc::now());
        assert_eq!(rows.len(), 2);
    }
```

Ensure the tests module imports `use std::collections::BTreeMap;` and `use chrono::Utc;` (the surrounding module already uses `super::*`, which brings in `ItemKey`, `CheapestListingValue`, `CheapestListings`, `floor_diff`).

- [ ] **Step 2: Run to verify they fail**

Run: `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros --bin ultros floor 2>&1 | tail -5`
Expected: compile errors (`add_listing` returns `()`, `floor_diff` undefined).

- [ ] **Step 3: `add_listing` reports changes; add `floor_diff`**

Replace `CheapestListings::add_listing`:

```rust
    /// Folds `listing` into the map. Returns the new stored value when the
    /// call changed the map — the key was absent, or the listing is strictly
    /// cheaper than what was stored — so callers can record floor moves.
    /// Ties keep the existing entry (and its world).
    fn add_listing<'a, T>(&mut self, listing: &'a T) -> Option<CheapestListingValue>
    where
        &'a T: Into<CheapestListingValue> + Into<ItemKey>,
    {
        let candidate: CheapestListingValue = listing.into();
        match self.item_map.entry(listing.into()) {
            Entry::Vacant(vacant) => {
                vacant.insert(candidate);
                Some(candidate)
            }
            Entry::Occupied(mut occupied) => {
                if candidate.price < occupied.get().price {
                    *occupied.get_mut() = candidate;
                    Some(candidate)
                } else {
                    None
                }
            }
        }
    }
```

Add, as a free function near `estimate_sale_price`:

```rust
/// Every key whose world-level floor differs between `old` and `new`, as
/// `resync` rows: present→absent is price 0, absent→present and price changes
/// carry the new price. Unchanged keys emit nothing, so a warm boot (map
/// restored from a fresh snapshot) is a handful of rows and a cold boot is
/// one row per key — the series' anchor.
fn floor_diff(
    old: &BTreeMap<ItemKey, CheapestListingValue>,
    new: &BTreeMap<ItemKey, CheapestListingValue>,
    world_id: i32,
    at: chrono::DateTime<Utc>,
) -> Vec<ultros_clickhouse::rows::FloorChangeRow> {
    use ultros_clickhouse::rows::{FloorChangeReason, FloorChangeRow};
    let mut rows = Vec::new();
    for (key, value) in new {
        if old.get(key).map(|v| v.price) != Some(value.price) {
            rows.push(FloorChangeRow::new(at, key.item_id, key.hq, world_id, value.price, FloorChangeReason::Resync));
        }
    }
    for key in old.keys() {
        if !new.contains_key(key) {
            rows.push(FloorChangeRow::new(at, key.item_id, key.hq, world_id, 0, FloorChangeReason::Resync));
        }
    }
    rows
}
```

Every existing call of `add_listing(...)` whose result is unused now triggers `unused_must_use`? No — `Option` is not `#[must_use]`, so `rebuild_cheapest_from_db`'s three `entry.add_listing(&value);` lines compile unchanged, as do the tests.

- [ ] **Step 4: Emit on the live add path (and fix the per-quality grouping)**

In `AnalyzerService::add_listings`, the grouping picks one min listing per *world* across both qualities, so an HQ listing arriving beside a cheaper NQ one never reaches the map. Group by `(world_id, hq)`:

```rust
        let listings = listings
            .iter()
            .into_grouping_map_by(|l| (l.0.world_id, l.0.hq))
            .min_by_key(|_key, val| val.0.price_per_unit);
```
Then in the loop, replace `world_entry.write().await.add_listing(listing);` with:
```rust
            if let Some(floor) = world_entry.write().await.add_listing(listing) {
                self.floor_writer.send(ultros_clickhouse::rows::FloorChangeRow::new(
                    Utc::now(),
                    listing.item_id,
                    listing.hq,
                    listing.world_id,
                    floor.price,
                    ultros_clickhouse::rows::FloorChangeReason::Listing,
                ));
            }
```
(The region/datacenter `add_listing` calls keep ignoring the result: only world floors are recorded.)

- [ ] **Step 5: Emit on the refill path**

In `remove_from_selector`, replace phase 1 and phase 3:

```rust
        // Phase 1: in-memory only. The lock is held for microseconds. Remember
        // the price each dropped key held so phase 3 can tell a real floor
        // move from a refill that landed on the same price.
        let dropped: BTreeMap<ItemKey, i32> = {
            let mut map = lock.write().await;
            listings
                .iter()
                .filter_map(|listing| {
                    let old = map.item_map.get(&ItemKey::from(*listing))?.price;
                    map.remove_if_cheapest(listing).map(|key| (key, old))
                })
                .collect()
        };
        if dropped.is_empty() {
            return;
        }
        let stale: BTreeSet<i32> = dropped.keys().map(|key| key.item_id).collect();
```
(keep phase 2 exactly as is; it consumes `stale`)
```rust
        // Phase 3: apply. `add_listing` keys on (item, hq), so both qualities from
        // the one query land on the right entries. Only world floors are
        // recorded; a refill that restores the same price is not a move.
        let now = Utc::now();
        let mut map = lock.write().await;
        for summary in &refill {
            let key = ItemKey::from(summary);
            if let (Some(floor), AnySelector::World(world_id)) = (map.add_listing(summary), selector)
                && dropped.get(&key) != Some(&floor.price)
            {
                self.floor_writer.send(ultros_clickhouse::rows::FloorChangeRow::new(
                    now,
                    key.item_id,
                    key.hq,
                    world_id,
                    floor.price,
                    ultros_clickhouse::rows::FloorChangeReason::Refill,
                ));
            }
        }
        if let AnySelector::World(world_id) = selector {
            for key in dropped.keys() {
                if !map.item_map.contains_key(key) {
                    self.floor_writer.send(ultros_clickhouse::rows::FloorChangeRow::new(
                        now,
                        key.item_id,
                        key.hq,
                        world_id,
                        0,
                        ultros_clickhouse::rows::FloorChangeReason::Refill,
                    ));
                }
            }
        }
```

- [ ] **Step 6: Emit on the resync path**

In `rebuild_cheapest_from_db`, replace the final loop:

```rust
        let at = Utc::now();
        let mut resync_rows = Vec::new();
        for (selector, listings) in fresh {
            if let Some(lock) = self.cheapest_items.get(&selector) {
                let mut current = lock.write().await;
                if let AnySelector::World(world_id) = selector {
                    resync_rows.extend(floor_diff(&current.item_map, &listings.item_map, world_id, at));
                }
                *current = listings;
            }
        }
        self.spawn_floor_resync_insert(resync_rows);
        Ok(())
```

Add the method to `impl AnalyzerService`:

```rust
    /// Bulk-insert a resync diff off the boot path. A cold boot's diff is one
    /// row per key — millions — which would swamp the bounded writer queue, so
    /// it goes straight to ClickHouse in chunks. Never awaited by the caller:
    /// the analyzer must go live whether or not ClickHouse is up. Not retried:
    /// the next resync re-derives the same state.
    fn spawn_floor_resync_insert(&self, rows: Vec<ultros_clickhouse::rows::FloorChangeRow>) {
        if rows.is_empty() {
            return;
        }
        let client = self.ch_client.clone();
        let writer = self.floor_writer.clone();
        tokio::spawn(async move {
            let ready = tokio::time::timeout(Duration::from_secs(600), writer.wait_ready()).await;
            if !matches!(ready, Ok(true)) {
                metrics::counter!("ultros_floor_changes_bulk_failures_total", "reason" => "writer_not_ready")
                    .increment(rows.len() as u64);
                warn!(rows = rows.len(), "floor resync dropped: ClickHouse schema never became ready");
                return;
            }
            match ultros_clickhouse::writer::insert_all(&client, &rows, 10_000).await {
                Ok(written) => info!(rows = written, "recorded floor resync"),
                Err(error) => {
                    metrics::counter!("ultros_floor_changes_bulk_failures_total", "reason" => "insert_failed")
                        .increment(rows.len() as u64);
                    warn!(?error, rows = rows.len(), "floor resync bulk insert failed; the next resync re-derives it");
                }
            }
        });
    }
```
`Duration` here is `std::time::Duration` — check which `Duration` the file imports at the top (it uses `chrono::Duration` for `MAX_SNAPSHOT_AGE`); write `std::time::Duration::from_secs(600)` explicitly to avoid the clash.

- [ ] **Step 7: Run the tests**

Run: `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p ultros --bin ultros analyzer 2>&1 | tail -8`
Expected: PASS, including the three new tests and the pre-existing `remove_if_cheapest_only_reports_keys_whose_price_actually_went_stale`.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
git add ultros/src/analyzer_service.rs
git commit -m "feat(analyzer): record world floor moves into ClickHouse floor_changes

Live listings, refills and Postgres resyncs each emit a row when the
lowest price for an (item, hq, world) changes. Also fixes the add path
grouping by world only, which dropped the HQ floor when an NQ listing
in the same event was cheaper.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: Docs, CI, local verification, PR

**Files:**
- Modify: `docs/ingest-observability.md` (metric table)
- Modify: `docs/clickhouse-recovery.md` (one paragraph)

- [ ] **Step 1: Document the metrics**

In the metric table of `docs/ingest-observability.md`, add rows:

```markdown
| `ultros_clickhouse_writer_*` | (existing) | `table` | Every writer metric now carries `table` = `sales`, `listing_events` or `floor_changes`; one bounded writer per table. |
| `ultros_listing_events_seed_failures_total` | counter | — | The one-time `listing_events` seed failed and will retry in 10 minutes. Runs on the rollup leader. |
| `ultros_floor_changes_bulk_failures_total` | counter | `reason` | A resync diff could not be bulk-inserted (`writer_not_ready`, `insert_failed`). Rows are dropped; the next resync re-derives them. |
```

In `docs/clickhouse-recovery.md`, after the "not durable replication" note, add:

```markdown
`listing_events` and `floor_changes` (added 2026-09) have **no Postgres backfill**: Postgres holds only the current board, so dropped rows are gone. `floor_changes` self-heals on the analyzer's next resync (every boot, and after bus lag); `listing_events` simply has a gap. Watch `ultros_clickhouse_writer_dropped_rows_total{table=...}`.
```

- [ ] **Step 2: Full CI check**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```
Expected: `REAL_EXIT=0`. Fix anything reported (clippy at `-D warnings`; watch for `collapsible_if` around the new `if let ... && ...` chains and for unused imports in `writer.rs` tests).

- [ ] **Step 3: Local end-to-end verification**

With the dev ClickHouse and Postgres up (`docker-compose.dev.yml`), run the app for a few minutes with the websocket enabled, then:

```bash
docker exec clickhouse clickhouse-client -q "SELECT source, kind, count() FROM listing_events GROUP BY source, kind ORDER BY source, kind FORMAT PrettyCompact"
docker exec clickhouse clickhouse-client -q "SELECT reason, count() FROM floor_changes GROUP BY reason FORMAT PrettyCompact"
docker exec clickhouse clickhouse-client -q "SELECT * FROM _listing_events_seed FORMAT PrettyCompact"
```
Expected: `snapshot/added` rows equal to the local `active_listing` count and one marker row; `websocket` rows of each kind appearing; `resync` rows from the boot diff and `listing`/`refill` rows trickling in. Record the counts in the PR description. If the local env cannot be brought up, say so explicitly in the PR and list which smoke tests ran.

- [ ] **Step 4: Commit and open the PR**

```bash
git add docs/ingest-observability.md docs/clickhouse-recovery.md
git commit -m "docs: listing history tracking metrics and recovery notes

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
git push -u origin claude/tracking-data-future-features-7fbd6a
gh pr create --base main --title "Record listing history: listing_events and floor_changes in ClickHouse" --body-file /tmp/pr_body.md
```

PR body (write to `/tmp/pr_body.md` first): what the two tables are, that nothing consumes them yet, the reprice-ordering caveat, the single-ingest-process assumption, the unmeasured-volume TTL escape hatch, the verification counts from Step 3, and which smoke tests ran. End with `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.

---

## Self-review

**Spec coverage.** `listing_events` DDL → Task 4. Seed with marker, torn-run cleanup, leader-scoped, 10-minute retry → Tasks 6 and 7. `floor_changes` DDL → Task 4; `listing`/`refill`/`resync` emission → Task 8; resync bulk insert with readiness wait and failure counter → Task 8. `ListingChange`/`ListingWrite` and the three write paths → Tasks 1–2. Generic writer, `table` label, `insert_all` → Task 5. Wiring (writers before socket spawn, `UpdateService`, `WebState`, shutdown order) → Task 7. Docs → Task 9. Tests listed in the spec: diff helpers (Task 1), row conversions (Task 3), generic writer (Task 5), `add_listing`/`floor_diff` (Task 8), schema/writer/marker smoke (Tasks 4, 6). The `sales/remove` gap remains out of scope as the spec says.

**Type consistency.** `ListingWrite { added, removed, changes }` is the same in Tasks 2, 7. `ListingEventRow::from_change(&ListingChange, ListingEventSource)` in Tasks 3, 7. `FloorChangeRow::new(DateTime<Utc>, i32, bool, i32, i32, FloorChangeReason)` in Tasks 3, 4, 5, 8. `insert_all(&ClickHouseClient, &[R], usize) -> Result<u64, _>` in Tasks 5, 6, 8. `Writer<R>::wait_ready() -> bool` unchanged from today. `LISTING_EVENTS_SEED_MARKER_TABLE` in Tasks 4, 6.
