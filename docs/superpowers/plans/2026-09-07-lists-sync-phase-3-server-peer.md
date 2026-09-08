# Lists Sync Phase 3: Server Peer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The server stores one Loro document per list, merges every write through one path that materialises the relational rows, records activity, emits today's events, and relays document updates over the websocket. Every existing writer (REST handlers, Discord bot) goes through that path. The legacy page keeps working unchanged.

**Architecture:** `ultros-db` gains `list_doc.rs`: the `list_doc` table access and the merge (`apply_list_update`, `edit_list_doc`) inside one transaction with a row lock, projecting `RowChange`s into `list_item` and `list`. The `ultros` crate gains `lists/sync.rs` (`ListSync`: merge plus events, activity and relay) and `lists/activity.rs` (pure classification). The websocket learns `SubscribeListDoc` and `ListDocUpdate` and relays through a new `list_docs` bus. The old row-writing functions in `ultros-db/src/lists.rs` are deleted, so nothing can bypass the merge path.

**Tech Stack:** sea-orm 2.0 (Postgres), axum 0.8 websockets, tokio broadcast buses, `ultros-list-doc` (Phase 2), `base64 0.22` in `ultros-api-types`.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md`, sections 4, 5 and 6.
- `list_item` and `list` rows are written only by the projection code in `ultros-db/src/list_doc.rs`. The old writers are deleted in Task 5, which makes the compiler the guard.
- The `lists` broadcast bus rises from 40 to 1,024 slots; the new `list_docs` bus has 1,024.
- Compaction thresholds: 5,000 changes or 256 KB.
- Bulk activity summary threshold: more than 10 rows of one kind.
- Database tests are `#[ignore]`d and read `MIGRATION_TEST_DATABASE_URL`, matching the existing pattern in `migration/`.
- Run `./check_ci.sh` before every commit; commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

---

## File structure

| File | Responsibility |
|---|---|
| `ultros-api-types/Cargo.toml` (modify) | add `base64` |
| `ultros-api-types/src/websocket.rs` (modify) | `base64_bytes` serde helper, `ListDocPayload`, two client and two server messages |
| `ultros-frontend/ultros-app/src/ws/realtime.rs` (modify) | route the two new server messages (keeps the frontend compiling; Phase 4 uses them) |
| `migration/src/m20260907_000001_list_doc.rs` (create), `migration/src/lib.rs` (modify) | `list_doc` table, natural-key dedupe and unique index |
| `ultros-db/src/entity/list_doc.rs` (create), `entity/mod.rs`, `entity/prelude.rs` (modify) | entity |
| `ultros-db/Cargo.toml` (modify) | add `ultros-list-doc` |
| `ultros-db/src/list_doc.rs` (create), `ultros-db/src/lib.rs` (modify) | stored documents, the merge, the projection |
| `ultros-list-doc/src/document.rs` (modify) | `sync_payload` |
| `ultros/Cargo.toml` (modify) | add `ultros-list-doc` |
| `ultros/src/lists/mod.rs`, `lists/activity.rs`, `lists/sync.rs` (create), `ultros/src/main.rs` (modify) | `ListSync` service, activity classification, wiring |
| `ultros/src/web/state.rs`, `ultros/src/web/error.rs` (modify) | `ListSync` in state, `ApiError: From<ListDocError>` |
| `ultros/src/web.rs` (modify) | seven list handlers rerouted |
| `ultros/src/discord/mod.rs`, `ultros/src/discord/ffxiv/lists.rs` (modify) | bot writes rerouted |
| `ultros-db/src/lists.rs` (modify) | delete the old row writers |
| `ultros/src/event.rs` (modify) | `ListDocEvent`, `list_docs` bus, `lists` capacity |
| `ultros/src/web/api/real_time_data.rs` (modify) | the two new client messages and the relay stream |

---

### Task 1: Websocket message types

**Files:**
- Modify: `ultros-api-types/Cargo.toml`
- Modify: `ultros-api-types/src/websocket.rs` (the `ListEventData` / `ServerClient` / `ClientMessage` enums at lines 236-290, plus the tests module at the end)
- Modify: `ultros-frontend/ultros-app/src/ws/realtime.rs` (`dispatch_message`, lines 251-297)
- Modify: `ultros/src/web/api/real_time_data.rs` (the `match msg` over `ClientMessage`, before `Message::Binary`)

**Interfaces:**
- Produces: `websocket::base64_bytes` (serde `with` module), `websocket::ListDocPayload::{Snapshot(Vec<u8>), Updates(Vec<u8>), UpToDate}`, `ClientMessage::SubscribeListDoc { subscription_id: Option<u64>, list_id: i32, version: Vec<u8> }`, `ClientMessage::ListDocUpdate { list_id: i32, update: Vec<u8> }`, `ServerClient::ListDocSubscribed { subscription_id: u64, list_id: i32, version: Vec<u8>, payload: ListDocPayload }`, `ServerClient::ListDocUpdate { list_id: i32, update: Vec<u8> }`. All byte fields travel as base64 strings.

- [ ] **Step 1: Add the dependency**

In `ultros-api-types/Cargo.toml` under `[dependencies]` add:

```toml
base64 = "0.22.1"
```

- [ ] **Step 2: Write the failing serde test**

Append to the existing `#[cfg(test)] mod tests` in `websocket.rs`:

```rust
    #[test]
    fn list_doc_messages_round_trip_bytes_as_base64() {
        let bytes = vec![0u8, 1, 127, 255];
        let subscribe = ClientMessage::SubscribeListDoc {
            subscription_id: Some(3),
            list_id: 9,
            version: bytes.clone(),
        };
        let text = serde_json::to_string(&subscribe).unwrap();
        assert!(text.contains("\"version\":\"AAF//w==\""), "{text}");
        let back: ClientMessage = serde_json::from_str(&text).unwrap();
        assert!(matches!(back, ClientMessage::SubscribeListDoc { version, list_id: 9, .. } if version == bytes));

        let subscribed = ServerClient::ListDocSubscribed {
            subscription_id: 3,
            list_id: 9,
            version: bytes.clone(),
            payload: ListDocPayload::Updates(bytes.clone()),
        };
        let text = serde_json::to_string(&subscribed).unwrap();
        let back: ServerClient = serde_json::from_str(&text).unwrap();
        assert!(matches!(back, ServerClient::ListDocSubscribed { payload: ListDocPayload::Updates(u), .. } if u == bytes));

        let update = ServerClient::ListDocUpdate {
            list_id: 9,
            update: bytes.clone(),
        };
        let back: ServerClient = serde_json::from_str(&serde_json::to_string(&update).unwrap()).unwrap();
        assert!(matches!(back, ServerClient::ListDocUpdate { update, .. } if update == bytes));

        let up_to_date: ListDocPayload =
            serde_json::from_str(&serde_json::to_string(&ListDocPayload::UpToDate).unwrap()).unwrap();
        assert_eq!(up_to_date, ListDocPayload::UpToDate);
        assert!(serde_json::from_str::<ClientMessage>(
            r#"{"ListDocUpdate":{"list_id":1,"update":"not base64!"}}"#
        )
        .is_err());
    }
```

- [ ] **Step 3: Run it to see it fail**

Run: `cargo test -p ultros-api-types list_doc_messages`
Expected: FAIL, `no variant named SubscribeListDoc`.

- [ ] **Step 4: Add the helper module and the variants**

Above `pub enum ListEventData` in `websocket.rs`:

```rust
/// Document bytes inside the JSON socket framing. Updates are small, so the
/// base64 overhead is accepted (spec section 5).
pub mod base64_bytes {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}

/// The server's answer to `SubscribeListDoc`.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub enum ListDocPayload {
    /// The client had no usable version: here is the whole document.
    Snapshot(#[serde(with = "base64_bytes")] Vec<u8>),
    /// What the client's version lacks.
    Updates(#[serde(with = "base64_bytes")] Vec<u8>),
    UpToDate,
}
```

Add to `ServerClient` after `ListUpdate(EventType<ListEventData>),`:

```rust
    ListDocSubscribed {
        subscription_id: u64,
        list_id: i32,
        #[serde(with = "base64_bytes")]
        version: Vec<u8>,
        payload: ListDocPayload,
    },
    /// Another peer's update, relayed. Wrapped in `SubscriptionEvent` by the
    /// server so the client routes it to the right handler.
    ListDocUpdate {
        list_id: i32,
        #[serde(with = "base64_bytes")]
        update: Vec<u8>,
    },
```

Add to `ClientMessage` after `SubscribeList { .. },`:

```rust
    /// Subscribe to a list's document (spec section 5). `version` is the
    /// client's encoded version vector; empty on a first visit.
    SubscribeListDoc {
        #[serde(default)]
        subscription_id: Option<u64>,
        list_id: i32,
        #[serde(with = "base64_bytes")]
        version: Vec<u8>,
    },
    /// The bytes of one local commit.
    ListDocUpdate {
        list_id: i32,
        #[serde(with = "base64_bytes")]
        update: Vec<u8>,
    },
```

- [ ] **Step 5: Keep the frontend and server matches exhaustive**

In `ultros-frontend/ultros-app/src/ws/realtime.rs`, `dispatch_message`: change the first match to include the new update variant in the "last update" bookkeeping:

```rust
        match &message {
            ServerClient::Sales(_)
            | ServerClient::Listings(_)
            | ServerClient::ListUpdate(_)
            | ServerClient::ListDocUpdate { .. } => {
                inner.set_last_update.set(Some(Utc::now()));
            }
            _ => {}
        }
```

and add two arms to the second match, before `ServerClient::SocketConnected | ServerClient::SubscriptionCreated => {}`:

```rust
            ServerClient::ListDocSubscribed { subscription_id, .. } => {
                if let Some(handler) = inner.handlers.borrow().get(&subscription_id) {
                    handler(message.clone());
                }
            }
            ServerClient::ListDocUpdate { .. } => {
                // Relayed updates normally arrive wrapped in SubscriptionEvent;
                // an unwrapped one goes to every handler like ListUpdate does.
                for handler in inner.handlers.borrow().values() {
                    handler(message.clone());
                }
            }
```

In `ultros/src/web/api/real_time_data.rs`, add a temporary arm to the `match msg` on `ClientMessage`, after the `SubscribeList` arm (Task 6 replaces it):

```rust
                                ClientMessage::SubscribeListDoc { .. }
                                | ClientMessage::ListDocUpdate { .. } => {
                                    sender
                                        .send(Message::Text(
                                            serde_json::to_string(&ServerClient::Error {
                                                message: "list documents are not enabled yet"
                                                    .to_string(),
                                            })?
                                            .into(),
                                        ))
                                        .await?;
                                }
```

- [ ] **Step 6: Run the tests and both frontend builds**

Run: `cargo test -p ultros-api-types`, `cargo check -p ultros`, `cargo check -p ultros-app`, `cargo check -p ultros-app --features hydrate --no-default-features`
Expected: all succeed.

- [ ] **Step 7: Commit**

```bash
git add ultros-api-types Cargo.lock ultros-frontend/ultros-app/src/ws/realtime.rs ultros/src/web/api/real_time_data.rs
git commit -m "feat(ws): list document sync messages with base64 bytes

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Migration and entity

**Files:**
- Create: `migration/src/m20260907_000001_list_doc.rs`
- Modify: `migration/src/lib.rs` (module declaration and the `migrations()` list)
- Create: `ultros-db/src/entity/list_doc.rs`
- Modify: `ultros-db/src/entity/mod.rs`, `ultros-db/src/entity/prelude.rs`

**Interfaces:**
- Produces: table `list_doc(list_id PK FK→list CASCADE, snapshot bytea, version bytea, changes_since_compaction int default 0, updated_at timestamptz default now())`; unique index `idx_list_item_natural_key` on `list_item (list_id, item_id, COALESCE(hq::int, -1))`; entity `ultros_db::entity::list_doc::{Entity, Model, ActiveModel, Column}` and `prelude::ListDoc`.

- [ ] **Step 1: Write the migration with its gated test**

```rust
//! The server's copy of each list's Loro document (spec section 4.1), and
//! the natural-key uniqueness the projection relies on (section 4.3).

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Duplicate `(list_id, item_id, hq)` rows predate the application-level
/// dedupe. Fold each group into its lowest id, summing what a merge of adds
/// would have summed, then drop the rest.
const FOLD_DUPLICATES: &str = r#"
WITH dupes AS (
    SELECT list_id, item_id, hq,
           MIN(id) AS keep_id,
           SUM(COALESCE(quantity, 1)) AS quantity,
           SUM(COALESCE(acquired, 0)) AS acquired
    FROM list_item
    GROUP BY list_id, item_id, hq
    HAVING COUNT(*) > 1
)
UPDATE list_item li
SET quantity = d.quantity, acquired = d.acquired
FROM dupes d
WHERE li.id = d.keep_id
"#;

const DELETE_DUPLICATES: &str = r#"
DELETE FROM list_item li
USING (
    SELECT l.id
    FROM list_item l
    WHERE EXISTS (
        SELECT 1 FROM list_item o
        WHERE o.list_id = l.list_id
          AND o.item_id = l.item_id
          AND o.hq IS NOT DISTINCT FROM l.hq
          AND o.id < l.id
    )
) x
WHERE li.id = x.id
"#;

/// NULL `hq` means "any quality" and must collide with itself, which a plain
/// unique index on a nullable column does not do.
const CREATE_NATURAL_KEY_INDEX: &str = r#"
CREATE UNIQUE INDEX IF NOT EXISTS idx_list_item_natural_key
ON list_item (list_id, item_id, (COALESCE(hq::int, -1)))
"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ListDoc::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ListDoc::ListId)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ListDoc::Snapshot).binary().not_null())
                    .col(ColumnDef::new(ListDoc::Version).binary().not_null())
                    .col(
                        ColumnDef::new(ListDoc::ChangesSinceCompaction)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(ListDoc::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_list_doc_list_id")
                            .from(ListDoc::Table, ListDoc::ListId)
                            .to(List::Table, List::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        let conn = manager.get_connection();
        conn.execute_unprepared(FOLD_DUPLICATES).await?;
        conn.execute_unprepared(DELETE_DUPLICATES).await?;
        conn.execute_unprepared(CREATE_NATURAL_KEY_INDEX).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP INDEX IF EXISTS idx_list_item_natural_key")
            .await?;
        manager
            .drop_table(Table::drop().table(ListDoc::Table).if_exists().to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ListDoc {
    Table,
    ListId,
    Snapshot,
    Version,
    ChangesSinceCompaction,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum List {
    Table,
    Id,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{Database, DbBackend, Statement, TransactionTrait};

    /// Session-local `list` and `list_item` shadow the real tables, so the
    /// dedupe and the index run against known rows and vanish on commit.
    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn duplicates_fold_into_the_lowest_id_and_the_index_exists() {
        let db = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let tx = db.begin().await.unwrap();
        tx.execute_unprepared("CREATE TEMPORARY TABLE list (id integer PRIMARY KEY) ON COMMIT DROP")
            .await
            .unwrap();
        tx.execute_unprepared(
            "CREATE TEMPORARY TABLE list_item (id serial PRIMARY KEY, list_id integer, item_id integer, hq boolean, quantity integer, acquired integer, target_price bigint) ON COMMIT DROP",
        )
        .await
        .unwrap();
        tx.execute_unprepared("INSERT INTO list VALUES (1)").await.unwrap();
        tx.execute_unprepared(
            "INSERT INTO list_item (list_id, item_id, hq, quantity, acquired) VALUES \
             (1, 5, NULL, 2, 1), (1, 5, NULL, 3, NULL), (1, 5, true, 1, 0), (1, 6, NULL, NULL, 4)",
        )
        .await
        .unwrap();
        let manager = SchemaManager::new(&tx);
        Migration.up(&manager).await.unwrap();
        let rows = tx
            .query_all_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT item_id, hq, quantity, acquired FROM list_item ORDER BY item_id, hq NULLS FIRST",
            ))
            .await
            .unwrap();
        let values: Vec<(i32, Option<bool>, Option<i32>, Option<i32>)> = rows
            .iter()
            .map(|row| {
                (
                    row.try_get("", "item_id").unwrap(),
                    row.try_get("", "hq").unwrap(),
                    row.try_get("", "quantity").unwrap(),
                    row.try_get("", "acquired").unwrap(),
                )
            })
            .collect();
        assert_eq!(
            values,
            vec![
                (5, None, Some(5), Some(1)),
                (5, Some(true), Some(1), Some(0)),
                (6, None, None, Some(4)),
            ]
        );
        let indexes = tx
            .query_all_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT indexname FROM pg_indexes WHERE indexname = 'idx_list_item_natural_key'",
            ))
            .await
            .unwrap();
        assert_eq!(indexes.len(), 1);
        tx.commit().await.unwrap();
    }
}
```

- [ ] **Step 2: Register it**

In `migration/src/lib.rs` add `mod m20260907_000001_list_doc;` next to the other module declarations and `Box::new(m20260907_000001_list_doc::Migration),` after `Box::new(m20260905_000001_list_item_acquired_integer::Migration),`.

- [ ] **Step 3: Write the entity**

`ultros-db/src/entity/list_doc.rs`:

```rust
//! `SeaORM` Entity. Hand-authored for the local-first list document store.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "list_doc")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub list_id: i32,
    pub snapshot: Vec<u8>,
    pub version: Vec<u8>,
    pub changes_since_compaction: i32,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::list::Entity",
        from = "Column::ListId",
        to = "super::list::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    List,
}

impl Related<super::list::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::List.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
```

Add `pub mod list_doc;` to `entity/mod.rs` after `pub mod list_activity;`, and `pub use super::list_doc::Entity as ListDoc;` to `entity/prelude.rs` after the `ListActivity` line.

- [ ] **Step 4: Build and run the gated test if a database is available**

Run: `cargo check -p migration -p ultros-db`
Expected: succeeds.
Run (when `MIGRATION_TEST_DATABASE_URL` points at a scratch Postgres): `MIGRATION_TEST_DATABASE_URL=postgres://... cargo test -p migration duplicates_fold -- --ignored`
Expected: 1 passed.

- [ ] **Step 5: Commit**

```bash
git add migration ultros-db/src/entity
git commit -m "feat(db): list_doc table and the list_item natural-key index

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: The merge path in `ultros-db`

**Files:**
- Modify: `ultros-db/Cargo.toml`
- Modify: `ultros-list-doc/src/document.rs` (add `SyncPayload` and `sync_payload`)
- Modify: `ultros-list-doc/src/lib.rs` (re-export `SyncPayload`)
- Create: `ultros-db/src/list_doc.rs`
- Modify: `ultros-db/src/lib.rs` (declare the module; add `UltrosDb::from_connection`)

**Interfaces:**
- Consumes: `ListDocument`, `RowSnapshot`, `MetaSnapshot`, `RowChange`, `RowKey`, `diff_rows`, `DocError` (Phase 2); `UltrosDb::get_permission`; entities `list`, `list_item`, `list_doc`; `AnySelector::try_from(&list::Model)` (`ultros-db/src/lists.rs:64`).
- Produces in `ultros-list-doc`: `SyncPayload::{Snapshot(Vec<u8>), Updates(Vec<u8>), UpToDate}`, `ListDocument::sync_payload(&self, client_version: &[u8]) -> Result<SyncPayload, DocError>`.
- Produces in `ultros-db::list_doc`: `ListDocError::{InvalidUpdate, MetaForbidden, List(ListError), Doc(DocError), Db(DbErr), Other(anyhow::Error)}`, `ProjectedChange { change: RowChange, row: list_item::Model }`, `MergeOutcome { list: list::Model, relay: Vec<u8>, changes: Vec<ProjectedChange>, meta_before: MetaSnapshot, meta_after: MetaSnapshot }` with `meta_changed()`, `StoredDoc { snapshot: Vec<u8>, version: Vec<u8> }`, `UltrosDb::from_connection(DatabaseConnection)`, `UltrosDb::list_doc_snapshot(list_id, user_id) -> Result<StoredDoc, ListDocError>`, `UltrosDb::apply_list_update(list_id, user_id, &[u8]) -> Result<MergeOutcome, ListDocError>`, `UltrosDb::edit_list_doc(list_id, user_id, FnOnce(&ListDocument) -> Result<R, DocError>) -> Result<(R, MergeOutcome), ListDocError>`; constants `COMPACT_AFTER_CHANGES = 5_000`, `COMPACT_AFTER_BYTES = 262_144`.

- [ ] **Step 1: Add `sync_payload` to the document crate, test first**

In `ultros-list-doc/src/document.rs` tests:

```rust
    #[test]
    fn sync_payload_picks_snapshot_updates_or_up_to_date() {
        let key = RowKey::new(1, None);
        let server = ListDocument::from_rows(meta(), &[row(1, Quality::Any, 1, 0)]);
        assert!(matches!(server.sync_payload(&[]).unwrap(), SyncPayload::Snapshot(_)));
        assert!(matches!(server.sync_payload(b"junk").unwrap(), SyncPayload::Snapshot(_)));
        let client = ListDocument::from_snapshot(&server.export_snapshot().unwrap()).unwrap();
        assert_eq!(server.sync_payload(&client.version()).unwrap(), SyncPayload::UpToDate);
        server.set_need(&key, 3).unwrap();
        let SyncPayload::Updates(bytes) = server.sync_payload(&client.version()).unwrap() else {
            panic!("expected updates");
        };
        client.import(&bytes).unwrap();
        assert_eq!(client.row(&key).unwrap().need, 3);
        // A client that is ahead gets nothing to import; it will send its own diff.
        client.set_need(&key, 4).unwrap();
        assert!(matches!(server.sync_payload(&client.version()).unwrap(), SyncPayload::Updates(_)));
    }
```

Then add to `document.rs`:

```rust
/// The server's answer to a subscribe handshake (spec section 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncPayload {
    Snapshot(Vec<u8>),
    Updates(Vec<u8>),
    UpToDate,
}
```

and inside `impl ListDocument`:

```rust
    /// What to send a peer that reports `client_version`: a snapshot when it
    /// has nothing usable, nothing when it is level, otherwise the updates it
    /// lacks. A peer that is ahead still gets `Updates`, possibly carrying
    /// nothing new; it then sends its own diff.
    pub fn sync_payload(&self, client_version: &[u8]) -> Result<SyncPayload, DocError> {
        if client_version.is_empty() {
            return Ok(SyncPayload::Snapshot(self.export_snapshot()?));
        }
        let Ok(client) = VersionVector::decode(client_version) else {
            return Ok(SyncPayload::Snapshot(self.export_snapshot()?));
        };
        if client == self.doc.oplog_vv() {
            return Ok(SyncPayload::UpToDate);
        }
        Ok(SyncPayload::Updates(
            self.doc.export(ExportMode::updates(&client))?,
        ))
    }
```

Add `SyncPayload` to the `pub use document::{...}` line in `lib.rs`. Run `cargo test -p ultros-list-doc sync_payload`; expected 1 passed.

- [ ] **Step 2: Add the dependency and the constructor**

`ultros-db/Cargo.toml` under `[dependencies]`:

```toml
ultros-list-doc = { path = "../ultros-list-doc" }
```

In `ultros-db/src/lib.rs`, inside `impl UltrosDb` next to `get_connection`:

```rust
    /// Wrap an existing connection. Used by tests that already opened one
    /// against a scratch database; production goes through `connect`.
    pub fn from_connection(db: DatabaseConnection) -> Self {
        Self { db }
    }
```

and declare the module next to `pub mod lists;`:

```rust
pub mod list_doc;
```

- [ ] **Step 3: Write the module**

`ultros-db/src/list_doc.rs`:

```rust
//! The server's copy of each list's Loro document and the single merge path
//! that keeps the relational rows a projection of it (spec section 4).
//!
//! Nothing else writes `list_item` or the `list` name and scope columns. The
//! old writers in `lists.rs` are gone, so this module is the only door.

use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    DatabaseTransaction, DbBackend, DbErr, EntityTrait, IntoActiveModel, QueryFilter, Statement,
    TransactionTrait,
};
use ultros_api_types::list::ListPermission;
use ultros_api_types::world_helper::AnySelector;
use ultros_list_doc::{
    DocError, ListDocument, MetaSnapshot, RowChange, RowKey, RowSnapshot, diff_rows,
};

use crate::UltrosDb;
use crate::entity::{list, list_doc, list_item};
use crate::lists::ListError;

/// Past either bound the next store is a shallow snapshot (spec section 4.1).
pub const COMPACT_AFTER_CHANGES: i32 = 5_000;
pub const COMPACT_AFTER_BYTES: usize = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ListDocError {
    #[error("update bytes are not a valid document update")]
    InvalidUpdate,
    #[error("only the list owner can change its name or scope")]
    MetaForbidden,
    #[error("{0}")]
    List(#[from] ListError),
    #[error("document: {0}")]
    Doc(#[from] DocError),
    #[error("database: {0}")]
    Db(#[from] DbErr),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// One projected row change with the relational row it touched: the row
/// after the change for adds and updates, the row before it for removals.
#[derive(Clone, Debug)]
pub struct ProjectedChange {
    pub change: RowChange,
    pub row: list_item::Model,
}

#[derive(Debug)]
pub struct MergeOutcome {
    /// The list row after any meta projection.
    pub list: list::Model,
    /// The update bytes, verbatim, for relaying to other peers.
    pub relay: Vec<u8>,
    pub changes: Vec<ProjectedChange>,
    pub meta_before: MetaSnapshot,
    pub meta_after: MetaSnapshot,
}

impl MergeOutcome {
    pub fn meta_changed(&self) -> bool {
        self.meta_before != self.meta_after
    }
}

#[derive(Clone, Debug)]
pub struct StoredDoc {
    pub snapshot: Vec<u8>,
    pub version: Vec<u8>,
}

fn meta_of(list: &list::Model) -> MetaSnapshot {
    MetaSnapshot {
        name: list.name.clone(),
        scope: AnySelector::try_from(list).ok(),
    }
}

fn row_snapshot(model: &list_item::Model) -> RowSnapshot {
    RowSnapshot {
        key: RowKey::new(model.item_id, model.hq),
        need: model.quantity.unwrap_or(1) as i64,
        acquired: model.acquired.unwrap_or(0) as i64,
        target: model.target_price,
    }
}

fn clamp_i32(value: i64) -> i32 {
    value.clamp(0, i32::MAX as i64) as i32
}

async fn lock_row(txn: &DatabaseTransaction, list_id: i32) -> Result<(), ListDocError> {
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT list_id FROM list_doc WHERE list_id = $1 FOR UPDATE",
        [list_id.into()],
    ))
    .await?;
    Ok(())
}

/// The stored document, locked for this transaction, built from the list's
/// rows on first touch. Two first touches can race on the insert; the loser
/// re-reads the winner's row.
async fn load_or_create(
    txn: &DatabaseTransaction,
    list_id: i32,
) -> Result<list_doc::Model, ListDocError> {
    lock_row(txn, list_id).await?;
    if let Some(stored) = list_doc::Entity::find_by_id(list_id).one(txn).await? {
        return Ok(stored);
    }
    let list = list::Entity::find_by_id(list_id)
        .one(txn)
        .await?
        .ok_or(ListError::NotFound)?;
    let rows = list_item::Entity::find()
        .filter(list_item::Column::ListId.eq(list_id))
        .all(txn)
        .await?;
    let snapshots: Vec<RowSnapshot> = rows.iter().map(row_snapshot).collect();
    let doc = ListDocument::from_rows(meta_of(&list), &snapshots);
    let fresh = list_doc::ActiveModel {
        list_id: Set(list_id),
        snapshot: Set(doc.export_snapshot()?),
        version: Set(doc.version()),
        changes_since_compaction: Set(0),
        updated_at: sea_orm::ActiveValue::NotSet,
    };
    list_doc::Entity::insert(fresh)
        .on_conflict(
            OnConflict::column(list_doc::Column::ListId)
                .do_nothing()
                .to_owned(),
        )
        .exec_without_returning(txn)
        .await?;
    lock_row(txn, list_id).await?;
    list_doc::Entity::find_by_id(list_id)
        .one(txn)
        .await?
        .ok_or_else(|| ListDocError::Db(DbErr::RecordNotFound(format!("list_doc {list_id}"))))
}

fn natural_key_filter(list_id: i32, key: &RowKey) -> sea_orm::Select<list_item::Entity> {
    let select = list_item::Entity::find()
        .filter(list_item::Column::ListId.eq(list_id))
        .filter(list_item::Column::ItemId.eq(key.item_id));
    match key.hq() {
        Some(hq) => select.filter(list_item::Column::Hq.eq(hq)),
        None => select.filter(list_item::Column::Hq.is_null()),
    }
}

async fn project_rows(
    txn: &DatabaseTransaction,
    list_id: i32,
    changes: Vec<RowChange>,
) -> Result<Vec<ProjectedChange>, ListDocError> {
    let mut projected = Vec::with_capacity(changes.len());
    for change in changes {
        match &change {
            RowChange::Added(row) => {
                let model = list_item::ActiveModel {
                    id: sea_orm::ActiveValue::NotSet,
                    item_id: Set(row.key.item_id),
                    list_id: Set(list_id),
                    hq: Set(row.key.hq()),
                    quantity: Set(Some(clamp_i32(row.need))),
                    acquired: Set(Some(clamp_i32(row.acquired))),
                    target_price: Set(row.target),
                }
                .insert(txn)
                .await?;
                projected.push(ProjectedChange { change, row: model });
            }
            RowChange::Removed(row) => {
                if let Some(model) = natural_key_filter(list_id, &row.key).one(txn).await? {
                    list_item::Entity::delete_by_id(model.id).exec(txn).await?;
                    projected.push(ProjectedChange { change, row: model });
                }
            }
            RowChange::Updated { after, .. } => {
                if let Some(model) = natural_key_filter(list_id, &after.key).one(txn).await? {
                    let mut active = model.into_active_model();
                    active.quantity = Set(Some(clamp_i32(after.need)));
                    active.acquired = Set(Some(clamp_i32(after.acquired)));
                    active.target_price = Set(after.target);
                    let model = active.update(txn).await?;
                    projected.push(ProjectedChange { change, row: model });
                }
            }
        }
    }
    Ok(projected)
}

async fn project_meta(
    txn: &DatabaseTransaction,
    list_id: i32,
    before: &MetaSnapshot,
    after: &MetaSnapshot,
) -> Result<list::Model, ListDocError> {
    let list = list::Entity::find_by_id(list_id)
        .one(txn)
        .await?
        .ok_or(ListError::NotFound)?;
    if before == after {
        return Ok(list);
    }
    let mut active = list.into_active_model();
    active.name = Set(after.name.clone());
    if let Some(scope) = after.scope {
        let (datacenter_id, region_id, world_id) = match scope {
            AnySelector::Datacenter(dc) => (Some(dc), None, None),
            AnySelector::Region(region) => (None, Some(region), None),
            AnySelector::World(world) => (None, None, Some(world)),
        };
        active.datacenter_id = Set(datacenter_id);
        active.region_id = Set(region_id);
        active.world_id = Set(world_id);
    }
    Ok(active.update(txn).await?)
}

async fn store(
    txn: &DatabaseTransaction,
    stored: &list_doc::Model,
    doc: &ListDocument,
) -> Result<(), ListDocError> {
    let changes = stored.changes_since_compaction + 1;
    let full = doc.export_snapshot()?;
    let (snapshot, changes) =
        if changes >= COMPACT_AFTER_CHANGES || full.len() >= COMPACT_AFTER_BYTES {
            (doc.export_shallow()?, 0)
        } else {
            (full, changes)
        };
    let mut active = stored.clone().into_active_model();
    active.snapshot = Set(snapshot);
    active.version = Set(doc.version());
    active.changes_since_compaction = Set(changes);
    active.updated_at = Set(chrono::Utc::now().into());
    active.update(txn).await?;
    Ok(())
}

/// Spec section 4.2, steps 1 to 6. Runs inside the caller's transaction.
async fn merge(
    txn: &DatabaseTransaction,
    list_id: i32,
    permission: ListPermission,
    update: &[u8],
) -> Result<MergeOutcome, ListDocError> {
    let stored = load_or_create(txn, list_id).await?;
    let doc = ListDocument::from_snapshot(&stored.snapshot)?;
    let rows_before = doc.rows();
    let meta_before = doc.meta();
    doc.import(update).map_err(|_| ListDocError::InvalidUpdate)?;
    let rows_after = doc.rows();
    let meta_after = doc.meta();
    if meta_after != meta_before && permission < ListPermission::Owner {
        return Err(ListDocError::MetaForbidden);
    }
    let list = project_meta(txn, list_id, &meta_before, &meta_after).await?;
    let changes = project_rows(txn, list_id, diff_rows(&rows_before, &rows_after)).await?;
    store(txn, &stored, &doc).await?;
    Ok(MergeOutcome {
        list,
        relay: update.to_vec(),
        changes,
        meta_before,
        meta_after,
    })
}

impl UltrosDb {
    /// The stored document for a reader, created from the rows on first touch.
    pub async fn list_doc_snapshot(
        &self,
        list_id: i32,
        user_id: i64,
    ) -> Result<StoredDoc, ListDocError> {
        let permission = self.get_permission(list_id, user_id).await?;
        if permission < ListPermission::Read {
            return Err(ListError::Forbidden("Insufficient permissions to read list").into());
        }
        let txn = self.db.begin().await?;
        let stored = load_or_create(&txn, list_id).await?;
        txn.commit().await?;
        Ok(StoredDoc {
            snapshot: stored.snapshot,
            version: stored.version,
        })
    }

    /// Merge update bytes from a peer. Needs write permission; a change to
    /// the name or scope needs the owner.
    pub async fn apply_list_update(
        &self,
        list_id: i32,
        user_id: i64,
        update: &[u8],
    ) -> Result<MergeOutcome, ListDocError> {
        let permission = self.get_permission(list_id, user_id).await?;
        if permission < ListPermission::Write {
            return Err(ListError::Forbidden("Insufficient permissions to edit list").into());
        }
        let txn = self.db.begin().await?;
        match merge(&txn, list_id, permission, update).await {
            Ok(outcome) => {
                txn.commit().await?;
                Ok(outcome)
            }
            Err(e) => {
                let _ = txn.rollback().await;
                Err(e)
            }
        }
    }

    /// Edit the document as the server peer on behalf of `user_id`, then merge
    /// the resulting update through the same path a socket update takes.
    pub async fn edit_list_doc<R>(
        &self,
        list_id: i32,
        user_id: i64,
        edit: impl FnOnce(&ListDocument) -> Result<R, DocError>,
    ) -> Result<(R, MergeOutcome), ListDocError> {
        let permission = self.get_permission(list_id, user_id).await?;
        if permission < ListPermission::Write {
            return Err(ListError::Forbidden("Insufficient permissions to edit list").into());
        }
        let txn = self.db.begin().await?;
        let result = async {
            let stored = load_or_create(&txn, list_id).await?;
            let doc = ListDocument::from_snapshot(&stored.snapshot)?;
            let before = doc.version();
            let value = edit(&doc)?;
            let update = doc.export_since(&before)?;
            let outcome = merge(&txn, list_id, permission, &update).await?;
            Ok::<_, ListDocError>((value, outcome))
        }
        .await;
        match result {
            Ok(value) => {
                txn.commit().await?;
                Ok(value)
            }
            Err(e) => {
                let _ = txn.rollback().await;
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::list_shared_user;
    use sea_orm::Database;
    use ultros_list_doc::Quality;

    const OWNER: i64 = 990_000_000_301;
    const EDITOR: i64 = 990_000_000_302;

    async fn db() -> UltrosDb {
        let conn = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        UltrosDb::from_connection(conn)
    }

    /// A list owned by OWNER with EDITOR granted write, torn down by the caller
    /// through `delete_list`, which cascades to `list_doc`.
    async fn scratch_list(db: &UltrosDb) -> i32 {
        let owner = db
            .get_or_create_discord_user(OWNER as u64, "ListDocOwner".into())
            .await
            .unwrap();
        db.get_or_create_discord_user(EDITOR as u64, "ListDocEditor".into())
            .await
            .unwrap();
        let list = db
            .create_list(owner, "scratch".into(), Some(AnySelector::Datacenter(5)))
            .await
            .unwrap();
        list_shared_user::ActiveModel {
            list_id: Set(list.id),
            user_id: Set(EDITOR),
            permission: Set(2),
        }
        .insert(db.get_connection())
        .await
        .unwrap();
        list.id
    }

    async fn peer(db: &UltrosDb, list_id: i32, user: i64) -> ListDocument {
        let stored = db.list_doc_snapshot(list_id, user).await.unwrap();
        ListDocument::from_snapshot(&stored.snapshot).unwrap()
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn a_peer_update_projects_into_rows_and_back_again() {
        let db = db().await;
        let list_id = scratch_list(&db).await;
        let key = RowKey::new(5, Some(true));
        let peer = peer(&db, list_id, EDITOR).await;
        let since = peer.version();
        peer.add_row(key, 3, Some(120)).unwrap();
        peer.add_acquired(&key, 1).unwrap();
        let outcome = db
            .apply_list_update(list_id, EDITOR, &peer.export_since(&since).unwrap())
            .await
            .unwrap();
        assert_eq!(outcome.changes.len(), 1);
        let row = &outcome.changes[0].row;
        assert_eq!((row.item_id, row.hq, row.quantity, row.acquired, row.target_price), (5, Some(true), Some(3), Some(1), Some(120)));
        assert!(!outcome.meta_changed());

        let since = peer.version();
        peer.set_need(&key, 9).unwrap();
        let outcome = db
            .apply_list_update(list_id, EDITOR, &peer.export_since(&since).unwrap())
            .await
            .unwrap();
        assert!(matches!(outcome.changes[0].change, RowChange::Updated { .. }));
        assert_eq!(outcome.changes[0].row.quantity, Some(9));

        let since = peer.version();
        peer.remove_row(&key).unwrap();
        let outcome = db
            .apply_list_update(list_id, EDITOR, &peer.export_since(&since).unwrap())
            .await
            .unwrap();
        assert!(matches!(outcome.changes[0].change, RowChange::Removed(_)));
        assert!(db.get_list_items(list_id, OWNER).await.unwrap().is_empty());
        db.delete_list(list_id, OWNER).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn legacy_rows_seed_the_document_on_first_touch() {
        let db = db().await;
        let list_id = scratch_list(&db).await;
        list_item::ActiveModel {
            id: sea_orm::ActiveValue::NotSet,
            item_id: Set(7),
            list_id: Set(list_id),
            hq: Set(None),
            quantity: Set(None),
            acquired: Set(None),
            target_price: Set(None),
        }
        .insert(db.get_connection())
        .await
        .unwrap();
        let doc = peer(&db, list_id, OWNER).await;
        assert_eq!(
            doc.rows(),
            vec![RowSnapshot { key: RowKey::new(7, None), need: 1, acquired: 0, target: None }]
        );
        assert_eq!(doc.meta().scope, Some(AnySelector::Datacenter(5)));
        db.delete_list(list_id, OWNER).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn meta_changes_need_the_owner_and_bad_bytes_change_nothing() {
        let db = db().await;
        let list_id = scratch_list(&db).await;
        let editor = peer(&db, list_id, EDITOR).await;
        let since = editor.version();
        editor.rename("renamed by editor").unwrap();
        let err = db
            .apply_list_update(list_id, EDITOR, &editor.export_since(&since).unwrap())
            .await
            .unwrap_err();
        assert!(matches!(err, ListDocError::MetaForbidden), "{err}");
        let (list, _) = db.get_list(list_id, OWNER).await.unwrap();
        assert_eq!(list.name, "scratch");

        let err = db.apply_list_update(list_id, OWNER, b"garbage").await.unwrap_err();
        assert!(matches!(err, ListDocError::InvalidUpdate), "{err}");

        let (_, outcome) = db
            .edit_list_doc(list_id, OWNER, |doc| {
                doc.rename("renamed by owner")?;
                doc.set_scope(AnySelector::World(79))
            })
            .await
            .unwrap();
        assert!(outcome.meta_changed());
        assert_eq!((outcome.list.name.as_str(), outcome.list.world_id), ("renamed by owner", Some(79)));
        db.delete_list(list_id, OWNER).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn edit_as_server_merges_a_quality_move() {
        let db = db().await;
        let list_id = scratch_list(&db).await;
        let key = RowKey::new(8, None);
        db.edit_list_doc(list_id, EDITOR, |doc| doc.add_row(key, 2, None))
            .await
            .unwrap();
        let (moved, outcome) = db
            .edit_list_doc(list_id, EDITOR, |doc| doc.set_quality(&key, Quality::Hq))
            .await
            .unwrap();
        assert_eq!(moved, RowKey::new(8, Some(true)));
        let kinds: Vec<bool> = outcome
            .changes
            .iter()
            .map(|c| matches!(c.change, RowChange::Added(_)))
            .collect();
        assert_eq!(kinds, vec![false, true], "removed the any row, added the hq row");
        let rows = db.get_list_items(list_id, OWNER).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].item_id, rows[0].hq, rows[0].quantity), (8, Some(true), Some(2)));
        db.delete_list(list_id, OWNER).await.unwrap();
    }
}
```

- [ ] **Step 4: Build, then run the gated tests against a scratch database**

Run: `cargo check -p ultros-db`
Expected: succeeds.
Run: `MIGRATION_TEST_DATABASE_URL=postgres://... cargo test -p ultros-db list_doc -- --ignored --test-threads=1`
Expected: 4 passed. (The tests create and delete their own list; the two discord users remain, which is fine for a scratch database.)

- [ ] **Step 5: Commit**

```bash
git add ultros-db ultros-list-doc Cargo.lock
git commit -m "feat(db): list document storage and the single merge path

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: `ListSync` service, activity classification, wiring

**Files:**
- Modify: `ultros/Cargo.toml`
- Create: `ultros/src/lists/mod.rs`, `ultros/src/lists/activity.rs`, `ultros/src/lists/sync.rs`
- Modify: `ultros/src/main.rs` (module list at lines 3-20; state construction around lines 713-760)
- Modify: `ultros/src/web/state.rs` (`WebState` and a `FromRef`)
- Modify: `ultros/src/web/error.rs` (`From<ListDocError>`)
- Modify: `ultros/src/event.rs` (`ListDocEvent`, the bus, capacities)
- Modify: `ultros/src/discord/mod.rs` (`Data` field, `start_discord` parameter)

**Interfaces:**
- Consumes: `UltrosDb::{apply_list_update, edit_list_doc, list_doc_snapshot, get_or_create_discord_user, record_list_activity}`, `MergeOutcome`, `ProjectedChange`, `ListDocError`, `SyncPayload`; `resolve_item_name` (`ultros/src/alerts/price_alert_tracker.rs:114`); `EventSenders`.
- Produces: `event::ListDocEvent { list_id: i32, update: Vec<u8>, origin_socket: Option<u64> }`, `EventSenders.list_docs` / `EventReceivers.list_docs`; `lists::{Actor, Origin, ListSync, Applied}`; `lists::activity::{ActivityEntry, entries_for, BULK_THRESHOLD}`; `WebState.list_sync`; `discord::Data.list_sync`; `ApiError: From<ListDocError>`.

- [ ] **Step 1: Event bus**

In `ultros/src/event.rs`:

Add after `pub(crate) type EventProducer<T> = ...;`:

```rust
/// One merged list-document update, relayed to every other subscriber of
/// that list. `origin_socket` is the socket that sent it, so the relay skips
/// the sender; server-side writers carry `None`.
#[derive(Debug)]
pub(crate) struct ListDocEvent {
    pub(crate) list_id: i32,
    pub(crate) update: Vec<u8>,
    pub(crate) origin_socket: Option<u64>,
}

/// Ring size for the list buses. A MakePlace import or a bulk HQ change
/// emits one event per row; 40 slots turned those into `Stale` refetch storms.
const LISTS_BUS_SIZE: usize = 1024;
```

In `create_event_busses`, change `let (list_sender, list_receiver) = channel(40);` to `channel(LISTS_BUS_SIZE)` and add `let (list_doc_sender, list_doc_receiver) = channel(LISTS_BUS_SIZE);`; add `list_docs: list_doc_sender,` to the `EventSenders` literal and `list_docs: list_doc_receiver,` to the `EventReceivers` literal. Add the field `pub(crate) list_docs: EventProducer<ListDocEvent>,` to `EventSenders`, `pub(crate) list_docs: EventBus<ListDocEvent>,` to `EventReceivers`, and `list_docs: self.list_docs.resubscribe(),` to `impl Clone for EventReceivers`. The destructuring `let EventReceivers { .. } = events;` in `real_time_data.rs` must gain `list_docs,` (used in Task 6; bind it as `list_docs: _` until then to keep the build green... no: bind it as `list_docs` and add `let _ = &list_docs;` so Task 6 can remove that line).

- [ ] **Step 2: Activity classification with tests**

`ultros/src/lists/activity.rs`:

```rust
//! Turn projected row changes into activity-feed entries (spec section 4.4),
//! with the same kinds, payload shapes and messages the REST handlers wrote.

use serde_json::json;
use ultros_api_types::list::ListActivityKind;
use ultros_db::list_doc::ProjectedChange;
use ultros_list_doc::{MetaSnapshot, RowChange};

/// More rows than this of one kind in a single merge collapse into one
/// summary entry, as bulk add and bulk HQ always did.
pub(crate) const BULK_THRESHOLD: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivityEntry {
    pub kind: ListActivityKind,
    pub list_item_id: Option<i32>,
    pub item_id: Option<i32>,
    pub payload: serde_json::Value,
    pub message: String,
}

fn row_payload(row: &ultros_db::entity::list_item::Model) -> serde_json::Value {
    json!({
        "quantity": row.quantity,
        "acquired": row.acquired,
        "hq": row.hq,
        "target_price": row.target_price,
    })
}

pub(crate) fn entries_for(
    actor_name: &str,
    changes: &[ProjectedChange],
    meta: Option<&MetaSnapshot>,
    item_name: impl Fn(i32) -> String,
) -> Vec<ActivityEntry> {
    let mut entries = Vec::new();
    let added: Vec<&ProjectedChange> = changes
        .iter()
        .filter(|c| matches!(c.change, RowChange::Added(_)))
        .collect();
    let removed: Vec<&ProjectedChange> = changes
        .iter()
        .filter(|c| matches!(c.change, RowChange::Removed(_)))
        .collect();
    let updated: Vec<&ProjectedChange> = changes
        .iter()
        .filter(|c| matches!(c.change, RowChange::Updated { .. }))
        .collect();

    if added.len() > BULK_THRESHOLD {
        entries.push(ActivityEntry {
            kind: ListActivityKind::ItemAdded,
            list_item_id: None,
            item_id: None,
            payload: json!({ "bulk": true, "count": added.len() }),
            message: format!("{actor_name} imported {} items", added.len()),
        });
    } else {
        for change in added {
            entries.push(ActivityEntry {
                kind: ListActivityKind::ItemAdded,
                list_item_id: Some(change.row.id),
                item_id: Some(change.row.item_id),
                payload: row_payload(&change.row),
                message: format!("{actor_name} added {}", item_name(change.row.item_id)),
            });
        }
    }

    if removed.len() > BULK_THRESHOLD {
        entries.push(ActivityEntry {
            kind: ListActivityKind::ItemsRemoved,
            list_item_id: None,
            item_id: None,
            payload: json!({ "count": removed.len() }),
            message: format!("{actor_name} removed {} items", removed.len()),
        });
    } else {
        for change in removed {
            entries.push(ActivityEntry {
                kind: ListActivityKind::ItemRemoved,
                list_item_id: Some(change.row.id),
                item_id: Some(change.row.item_id),
                payload: row_payload(&change.row),
                message: format!("{actor_name} removed {}", item_name(change.row.item_id)),
            });
        }
    }

    if updated.len() > BULK_THRESHOLD {
        entries.push(ActivityEntry {
            kind: ListActivityKind::ItemUpdated,
            list_item_id: None,
            item_id: None,
            payload: json!({ "bulk": true, "count": updated.len() }),
            message: format!("{actor_name} updated {} items", updated.len()),
        });
    } else {
        for change in updated {
            let RowChange::Updated { before, after } = &change.change else {
                continue;
            };
            let mut diff = serde_json::Map::new();
            if before.need != after.need {
                diff.insert("quantity".into(), json!([before.need, after.need]));
            }
            if before.acquired != after.acquired {
                diff.insert("acquired".into(), json!([before.acquired, after.acquired]));
            }
            if before.target != after.target {
                diff.insert("target_price".into(), json!([before.target, after.target]));
            }
            let acquired_now = after.acquired >= after.need && before.acquired < before.need;
            let name = item_name(change.row.item_id);
            entries.push(ActivityEntry {
                kind: if acquired_now {
                    ListActivityKind::ItemAcquired
                } else {
                    ListActivityKind::ItemUpdated
                },
                list_item_id: Some(change.row.id),
                item_id: Some(change.row.item_id),
                payload: serde_json::Value::Object(diff),
                message: if acquired_now {
                    format!("{actor_name} got {name}")
                } else {
                    format!("{actor_name} updated {name}")
                },
            });
        }
    }

    if let Some(meta) = meta {
        entries.push(ActivityEntry {
            kind: ListActivityKind::ListUpdated,
            list_item_id: None,
            item_id: None,
            payload: json!({ "name": meta.name }),
            message: format!("{actor_name} updated list {}", meta.name),
        });
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_db::entity::list_item;
    use ultros_list_doc::{RowKey, RowSnapshot};

    fn row(id: i32, item: i32) -> list_item::Model {
        list_item::Model {
            id,
            item_id: item,
            list_id: 1,
            hq: None,
            quantity: Some(2),
            acquired: Some(0),
            target_price: None,
        }
    }

    fn snapshot(item: i32, need: i64, acquired: i64) -> RowSnapshot {
        RowSnapshot {
            key: RowKey::new(item, None),
            need,
            acquired,
            target: None,
        }
    }

    #[test]
    fn single_changes_get_one_entry_each_with_the_legacy_shapes() {
        let changes = vec![
            ProjectedChange {
                change: RowChange::Added(snapshot(5, 2, 0)),
                row: row(11, 5),
            },
            ProjectedChange {
                change: RowChange::Updated {
                    before: snapshot(6, 2, 1),
                    after: snapshot(6, 2, 2),
                },
                row: row(12, 6),
            },
            ProjectedChange {
                change: RowChange::Removed(snapshot(7, 1, 0)),
                row: row(13, 7),
            },
        ];
        let entries = entries_for("Aaron", &changes, None, |id| format!("item{id}"));
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].kind, ListActivityKind::ItemAdded);
        assert_eq!(entries[0].message, "Aaron added item5");
        assert_eq!(entries[0].payload["quantity"], 2);
        assert_eq!(entries[1].kind, ListActivityKind::ItemRemoved);
        assert_eq!(entries[2].kind, ListActivityKind::ItemAcquired);
        assert_eq!(entries[2].message, "Aaron got item6");
        assert_eq!(entries[2].payload["acquired"], serde_json::json!([1, 2]));
        assert!(entries[2].payload.get("quantity").is_none());
    }

    #[test]
    fn a_partial_acquire_is_an_update_and_meta_adds_list_updated() {
        let changes = vec![ProjectedChange {
            change: RowChange::Updated {
                before: snapshot(6, 5, 1),
                after: snapshot(6, 5, 2),
            },
            row: row(12, 6),
        }];
        let meta = MetaSnapshot {
            name: "Glamour".into(),
            scope: None,
        };
        let entries = entries_for("Aaron", &changes, Some(&meta), |_| "x".into());
        assert_eq!(entries[0].kind, ListActivityKind::ItemUpdated);
        assert_eq!(entries[1].kind, ListActivityKind::ListUpdated);
        assert_eq!(entries[1].message, "Aaron updated list Glamour");
    }

    #[test]
    fn more_than_ten_of_a_kind_collapse_into_a_summary() {
        let changes: Vec<ProjectedChange> = (0..11)
            .map(|i| ProjectedChange {
                change: RowChange::Added(snapshot(i, 1, 0)),
                row: row(100 + i, i),
            })
            .collect();
        let entries = entries_for("Aaron", &changes, None, |_| "x".into());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].payload, serde_json::json!({ "bulk": true, "count": 11 }));
        assert_eq!(entries[0].message, "Aaron imported 11 items");
    }
}
```

- [ ] **Step 3: The service**

`ultros/src/lists/sync.rs`:

```rust
//! `ListSync`: the merge path plus everything the rest of the product needs
//! to hear about it (spec sections 4.2, 4.4, 4.5, 5).

use std::sync::Arc;

use tracing::warn;
use ultros_api_types::list::{List, ListActivity};
use ultros_api_types::websocket::{ListDocPayload, ListEventData};
use ultros_db::UltrosDb;
use ultros_db::list_doc::{ListDocError, MergeOutcome, ProjectedChange};
use ultros_list_doc::{DocError, ListDocument, RowChange, SyncPayload};

use crate::alerts::price_alert_tracker::resolve_item_name;
use crate::event::{EventSenders, EventType, ListDocEvent};
use crate::lists::activity::entries_for;
use crate::web::oauth::AuthDiscordUser;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    Socket(u64),
    Rest,
    Bot,
}

#[derive(Clone, Debug)]
pub(crate) struct Actor {
    pub user_id: i64,
    pub username: String,
    pub origin: Origin,
}

impl Actor {
    pub(crate) fn from_user(user: &AuthDiscordUser, origin: Origin) -> Self {
        Self {
            user_id: user.id as i64,
            username: user.name.clone(),
            origin,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Applied {
    pub relay: Vec<u8>,
    pub changes: Vec<ProjectedChange>,
    pub meta_changed: bool,
}

#[derive(Clone)]
pub(crate) struct ListSync {
    db: UltrosDb,
    senders: EventSenders,
}

impl ListSync {
    pub(crate) fn new(db: UltrosDb, senders: EventSenders) -> Self {
        Self { db, senders }
    }

    /// Bytes from a peer.
    pub(crate) async fn apply_update(
        &self,
        list_id: i32,
        actor: &Actor,
        update: &[u8],
    ) -> Result<Applied, ListDocError> {
        let outcome = self
            .db
            .apply_list_update(list_id, actor.user_id, update)
            .await?;
        Ok(self.publish(list_id, actor, outcome).await)
    }

    /// A legacy writer's edit, made by the server peer on the actor's behalf.
    pub(crate) async fn edit_as_server<R>(
        &self,
        list_id: i32,
        actor: &Actor,
        edit: impl FnOnce(&ListDocument) -> Result<R, DocError>,
    ) -> Result<(R, Applied), ListDocError> {
        let (value, outcome) = self.db.edit_list_doc(list_id, actor.user_id, edit).await?;
        Ok((value, self.publish(list_id, actor, outcome).await))
    }

    /// The subscribe handshake: the server's version and what the client lacks.
    pub(crate) async fn subscribe_payload(
        &self,
        list_id: i32,
        user_id: i64,
        client_version: &[u8],
    ) -> Result<(Vec<u8>, ListDocPayload), ListDocError> {
        let stored = self.db.list_doc_snapshot(list_id, user_id).await?;
        let doc = ListDocument::from_snapshot(&stored.snapshot)?;
        let payload = match doc.sync_payload(client_version)? {
            SyncPayload::Snapshot(bytes) => ListDocPayload::Snapshot(bytes),
            SyncPayload::Updates(bytes) => ListDocPayload::Updates(bytes),
            SyncPayload::UpToDate => ListDocPayload::UpToDate,
        };
        Ok((doc.version(), payload))
    }

    /// Legacy events for the old page and the alert trackers, activity rows,
    /// and the relay to other document subscribers. Best effort: a failure
    /// here is logged, never returned, because the merge already committed.
    async fn publish(&self, list_id: i32, actor: &Actor, outcome: MergeOutcome) -> Applied {
        for change in &outcome.changes {
            let item = ultros_api_types::list::ListItem::from(change.row.clone());
            let event = match change.change {
                RowChange::Added(_) => EventType::added(ListEventData::ListItem(item)),
                RowChange::Removed(_) => EventType::removed(ListEventData::ListItem(item)),
                RowChange::Updated { .. } => EventType::updated(ListEventData::ListItem(item)),
            };
            if let Err(e) = self.senders.lists.send(event) {
                warn!(error = %e, "failed to broadcast list item event");
            }
        }
        if outcome.meta_changed() {
            match List::try_from(outcome.list.clone()) {
                Ok(list) => {
                    if let Err(e) = self
                        .senders
                        .lists
                        .send(EventType::updated(ListEventData::List(list)))
                    {
                        warn!(error = %e, "failed to broadcast list event");
                    }
                }
                Err(e) => warn!(error = %e, list_id, "list row has no scope"),
            }
        }

        let meta = outcome.meta_changed().then_some(&outcome.meta_after);
        let entries = entries_for(&actor.username, &outcome.changes, meta, resolve_item_name);
        if !entries.is_empty() {
            if let Err(e) = self
                .db
                .get_or_create_discord_user(actor.user_id as u64, actor.username.clone())
                .await
            {
                warn!(error = %e, "failed to ensure the activity actor exists");
            }
        }
        for entry in entries {
            match self
                .db
                .record_list_activity(
                    list_id,
                    actor.user_id,
                    actor.username.clone(),
                    entry.kind,
                    entry.list_item_id,
                    entry.item_id,
                    entry.payload,
                    entry.message,
                )
                .await
            {
                Ok(activity) => {
                    let activity = ListActivity::from(activity);
                    if let Err(e) = self
                        .senders
                        .lists
                        .send(EventType::added(ListEventData::Activity(activity)))
                    {
                        warn!(error = %e, "failed to broadcast list activity");
                    }
                }
                Err(e) => warn!(error = %e, list_id, "failed to record list activity"),
            }
        }

        let origin_socket = match actor.origin {
            Origin::Socket(id) => Some(id),
            Origin::Rest | Origin::Bot => None,
        };
        if let Err(e) = self.senders.list_docs.send(EventType::Update(Arc::new(ListDocEvent {
            list_id,
            update: outcome.relay.clone(),
            origin_socket,
        }))) {
            warn!(error = %e, "failed to relay list document update");
        }

        Applied {
            relay: outcome.relay,
            changes: outcome.changes,
            meta_changed: outcome.meta_changed(),
        }
    }
}
```

`ultros/src/lists/mod.rs`:

```rust
//! Lists: the local-first document's server side (spec section 4).

pub(crate) mod activity;
pub(crate) mod sync;

pub(crate) use sync::{Actor, Applied, ListSync, Origin};
```

Add `mod lists;` to `ultros/src/main.rs` after `mod item_update_service;`. Add to `ultros/Cargo.toml` `[dependencies]`:

```toml
ultros-list-doc = { path = "../ultros-list-doc" }
```

Check `resolve_item_name` is `pub(crate)` (it is, at `alerts/price_alert_tracker.rs:114`); if the `alerts` module or `price_alert_tracker` is private to `alerts`, make the module `pub(crate) mod price_alert_tracker;` in `ultros/src/alerts/mod.rs`.

- [ ] **Step 4: Error mapping**

In `ultros/src/web/error.rs`, add:

```rust
impl From<ultros_db::list_doc::ListDocError> for ApiError {
    fn from(error: ultros_db::list_doc::ListDocError) -> Self {
        use ultros_db::list_doc::ListDocError;
        use ultros_db::lists::ListError;
        match error {
            ListDocError::List(inner) => ApiError::from(anyhow::Error::from(inner)),
            ListDocError::MetaForbidden => ApiError::from(anyhow::Error::from(
                ListError::Forbidden("only the list owner can change its name or scope"),
            )),
            ListDocError::InvalidUpdate => ApiError::from(anyhow::Error::from(
                ListError::BadRequest("invalid document update"),
            )),
            other => ApiError::from(anyhow::anyhow!("{other}")),
        }
    }
}
```

(`ApiError: From<anyhow::Error>` already exists and maps a `ListError` in the chain to its status, as `list_permission.rs` relies on.)

- [ ] **Step 5: State and wiring**

`ultros/src/web/state.rs`: add `pub(crate) list_sync: crate::lists::ListSync,` to `WebState` and

```rust
impl FromRef<WebState> for crate::lists::ListSync {
    fn from_ref(input: &WebState) -> Self {
        input.list_sync.clone()
    }
}
```

`ultros/src/main.rs`: after `let (senders, receivers) = create_event_busses();` add `let list_sync = lists::ListSync::new(db.clone(), senders.clone());`. Pass `list_sync.clone()` as a new argument to `start_discord(...)` and add `list_sync,` to the `WebState { ... }` literal.

`ultros/src/discord/mod.rs`: add `list_sync: crate::lists::ListSync,` to `Data`, a `list_sync: crate::lists::ListSync,` parameter to `start_discord` (after `db: UltrosDb,`), and `list_sync,` to the `Data { ... }` literal.

- [ ] **Step 6: Build and test**

Run: `cargo test -p ultros --lib lists::` and `cargo check -p ultros`
Expected: 3 activity tests pass; the crate builds.

- [ ] **Step 7: Commit**

```bash
git add ultros Cargo.lock
git commit -m "feat(lists): ListSync service with activity, legacy events and relay

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Reroute every writer and delete the old ones

**Files:**
- Modify: `ultros/src/web.rs` (handlers `edit_list` 1600-1638, `post_item_to_list` 1639-1683, `post_items_to_list` 1684-1718, `edit_list_item` 1719-1760, `delete_list_item` 1761-1792, `bulk_edit_list_items_hq` 1799-1832, `delete_multiple_list_items` 1833-1870; the helpers `record_list_activity` and `item_change_payload` at 133-194 lose their last callers except `create_list`, `delete_list` and the share handlers, which keep using `record_list_activity`)
- Modify: `ultros/src/discord/ffxiv/lists.rs` (`add_item` 322-357, `remove_item` 359-385)
- Modify: `ultros-db/src/lists.rs` (delete `add_item_to_list`, `update_list_item`, `set_list_item_target_price`, `add_items_to_list`, `set_list_items_hq`, `remove_item_from_list`, `update_list`)

**Interfaces:**
- Consumes: `ListSync::{edit_as_server}`, `Actor`, `Origin`, `RowKey`, `Quality`, `UltrosDb::get_list_item`, `UltrosDb::get_list_items`.
- Produces: identical HTTP paths, bodies and status codes; identical bot commands.

- [ ] **Step 1: Rewrite the web handlers**

Add these imports near the top of `web.rs`:

```rust
use crate::lists::{Actor, ListSync, Origin};
use ultros_list_doc::{Quality, RowKey};
```

Replace the seven handlers:

```rust
pub(crate) async fn edit_list(
    State(list_sync): State<ListSync>,
    user: AuthDiscordUser,
    Json(list): Json<List>,
) -> Result<Json<()>, ApiError> {
    let actor = Actor::from_user(&user, Origin::Rest);
    let name = list.name.clone();
    let scope = list.wdr_filter;
    list_sync
        .edit_as_server(list.id, &actor, move |doc| {
            doc.rename(&name)?;
            doc.set_scope(scope)
        })
        .await?;
    Ok(Json(()))
}

pub(crate) async fn post_item_to_list(
    State(list_sync): State<ListSync>,
    user: AuthDiscordUser,
    perm: crate::web::list_permission::RequireListPermission<
        { crate::web::list_permission::WRITE },
    >,
    Json(item): Json<ListItem>,
) -> Result<Json<()>, ApiError> {
    let actor = Actor::from_user(&user, Origin::Rest);
    let key = RowKey::new(item.item_id, item.hq);
    let need = item.quantity.unwrap_or(1) as i64;
    let acquired = item.acquired.unwrap_or(0) as i64;
    let target = item.target_price;
    list_sync
        .edit_as_server(perm.list_id, &actor, move |doc| {
            doc.add_row(key, need, target)?;
            if acquired != 0 {
                doc.add_acquired(&key, acquired)?;
            }
            Ok(())
        })
        .await?;
    Ok(Json(()))
}

pub(crate) async fn post_items_to_list(
    State(list_sync): State<ListSync>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
    Json(items): Json<Vec<ListItem>>,
) -> Result<Json<()>, ApiError> {
    let actor = Actor::from_user(&user, Origin::Rest);
    list_sync
        .edit_as_server(id, &actor, move |doc| {
            for item in items {
                let key = RowKey::new(item.item_id, item.hq);
                doc.add_row(key, item.quantity.unwrap_or(1) as i64, item.target_price)?;
                let acquired = item.acquired.unwrap_or(0) as i64;
                if acquired != 0 {
                    doc.add_acquired(&key, acquired)?;
                }
            }
            Ok(())
        })
        .await?;
    Ok(Json(()))
}

pub(crate) async fn edit_list_item(
    State(db): State<UltrosDb>,
    State(list_sync): State<ListSync>,
    user: AuthDiscordUser,
    Json(item): Json<ListItem>,
) -> Result<Json<()>, ApiError> {
    let before = db.get_list_item(item.id, user.id as i64).await?;
    let actor = Actor::from_user(&user, Origin::Rest);
    let key = RowKey::new(before.item_id, before.hq);
    let quality = Quality::from(item.hq);
    let need = item.quantity.unwrap_or(1) as i64;
    let acquired = item.acquired.unwrap_or(0) as i64;
    let target = item.target_price;
    list_sync
        .edit_as_server(before.list_id, &actor, move |doc| {
            let key = if quality != key.quality {
                doc.set_quality(&key, quality)?
            } else {
                key
            };
            doc.set_need(&key, need)?;
            doc.set_acquired(&key, acquired)?;
            doc.set_target(&key, target)
        })
        .await?;
    Ok(Json(()))
}

pub(crate) async fn delete_list_item(
    State(db): State<UltrosDb>,
    State(list_sync): State<ListSync>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<()>, ApiError> {
    let item = db.get_list_item(id, user.id as i64).await?;
    let actor = Actor::from_user(&user, Origin::Rest);
    let key = RowKey::new(item.item_id, item.hq);
    list_sync
        .edit_as_server(item.list_id, &actor, move |doc| doc.remove_row(&key))
        .await?;
    Ok(Json(()))
}

pub(crate) async fn bulk_edit_list_items_hq(
    State(db): State<UltrosDb>,
    State(list_sync): State<ListSync>,
    user: AuthDiscordUser,
    Json(data): Json<BulkHqUpdate>,
) -> Result<Json<()>, ApiError> {
    let actor = Actor::from_user(&user, Origin::Rest);
    let quality = Quality::from(data.hq);
    let mut by_list: HashMap<i32, Vec<RowKey>> = HashMap::new();
    for id in data.ids {
        let item = db.get_list_item(id, user.id as i64).await?;
        by_list
            .entry(item.list_id)
            .or_default()
            .push(RowKey::new(item.item_id, item.hq));
    }
    for (list_id, keys) in by_list {
        list_sync
            .edit_as_server(list_id, &actor, move |doc| {
                for key in keys {
                    if key.quality != quality {
                        doc.set_quality(&key, quality)?;
                    }
                }
                Ok(())
            })
            .await?;
    }
    Ok(Json(()))
}

pub(crate) async fn delete_multiple_list_items(
    State(db): State<UltrosDb>,
    State(list_sync): State<ListSync>,
    user: AuthDiscordUser,
    Json(ids): Json<Vec<i32>>,
) -> Result<Json<()>, ApiError> {
    let actor = Actor::from_user(&user, Origin::Rest);
    let mut by_list: HashMap<i32, Vec<RowKey>> = HashMap::new();
    for id in ids {
        let item = db.get_list_item(id, user.id as i64).await?;
        by_list
            .entry(item.list_id)
            .or_default()
            .push(RowKey::new(item.item_id, item.hq));
    }
    for (list_id, keys) in by_list {
        list_sync
            .edit_as_server(list_id, &actor, move |doc| {
                for key in keys {
                    doc.remove_row(&key)?;
                }
                Ok(())
            })
            .await?;
    }
    Ok(Json(()))
}
```

`HashMap` is `std::collections::HashMap` (import if `web.rs` does not already). Delete `item_change_payload` (its only caller was the old `edit_list_item`). Keep `record_list_activity` for `create_list`, `delete_list` and the share handlers. Remove now-unused imports (`ActiveValue`, `try_join_all` if unused) so clippy stays clean.

- [ ] **Step 2: Rewrite the bot commands**

In `ultros/src/discord/ffxiv/lists.rs`, add `use crate::lists::{Actor, Origin};` and `use ultros_list_doc::RowKey;`, then replace the bodies:

```rust
async fn add_item(
    ctx: Context<'_>,
    #[description = "name of the list to add an item to"]
    #[autocomplete = "autocomplete_list_name"]
    list_name: String,
    #[description = "item to add"]
    #[autocomplete = "autocomplete_item_name_global"]
    item_name: String,
    #[description = "quantity of the item to add. Leave blank for no quantity"] quantity: Option<
        i32,
    >,
    #[description = "hq? Leave blank for no filter"] hq: Option<bool>,
) -> Result<(), Error> {
    let author_id = ctx.author().id.get() as i64;
    let user_lang = discord_locale_to_xiv_language(ctx.locale());
    let item_id = resolve_item_id_any_locale(&item_name).ok_or(anyhow!("Unable to find item"))?;
    let display_name = localized_item_name(item_id, user_lang);
    let list = resolve_list(&ctx, author_id, &list_name)
        .await?
        .ok_or(anyhow!("List not found"))?;
    let actor = Actor {
        user_id: author_id,
        username: ctx.author().name.clone(),
        origin: Origin::Bot,
    };
    let key = RowKey::new(item_id, hq);
    let need = quantity.unwrap_or(1) as i64;
    ctx.data()
        .list_sync
        .edit_as_server(list.id, &actor, move |doc| doc.add_row(key, need, None))
        .await?;
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("Item added")
                .description(format!("{} added to list {}", display_name, list.name)),
        ),
    )
    .await?;
    Ok(())
}

/// Remove an item from a list
#[poise::command(slash_command, prefix_command)]
async fn remove_item(
    ctx: Context<'_>,
    #[description = "name of the list to remove an item from"]
    #[autocomplete = "autocomplete_list_name"]
    list_name: String,
    #[description = "item to remove"] item_name: String,
) -> Result<(), Error> {
    let author_id = ctx.author().id.get() as i64;
    let id = resolve_item_id_any_locale(&item_name).ok_or(anyhow!("Unable to find item"))?;
    let list = resolve_list(&ctx, author_id, &list_name)
        .await?
        .ok_or(anyhow!("Unable to find list"))?;
    let item = ctx
        .data()
        .db
        .get_list_items(list.id, author_id)
        .await?
        .into_iter()
        .find(|i| i.item_id == id)
        .ok_or(anyhow!("Unable to find item on list"))?;
    let actor = Actor {
        user_id: author_id,
        username: ctx.author().name.clone(),
        origin: Origin::Bot,
    };
    let key = RowKey::new(item.item_id, item.hq);
    ctx.data()
        .list_sync
        .edit_as_server(list.id, &actor, move |doc| doc.remove_row(&key))
        .await?;
    Ok(())
}
```

`Error` in the bot is `Box<dyn std::error::Error + Send + Sync>`; `ListDocError` converts through `?` because it implements `std::error::Error`.

- [ ] **Step 3: Delete the old writers**

In `ultros-db/src/lists.rs` delete the functions `add_item_to_list`, `update_list_item`, `set_list_item_target_price`, `add_items_to_list`, `set_list_items_hq`, `remove_item_from_list` and `update_list`. Delete the `HashMap` import if it becomes unused. `get_permission`, `create_list`, `delete_list`, `get_lists_for_user`, `get_list_by_name_for_user`, `get_list`, `get_list_items`, `get_list_item`, `record_list_activity`, `get_list_activity`, `get_list_items_with_target`, `get_list_by_id`, `get_listings_for_list` and the share functions stay.

- [ ] **Step 4: Build everything and run the CI script**

Run: `./check_ci.sh > "$SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"`
Expected: `REAL_EXIT=0`. Any remaining caller of a deleted function fails to compile; reroute it through `ListSync` the same way.

- [ ] **Step 5: Run the list flow end to end**

Bring up a test-auth server (`AGENTS.md`, "test-auth feature") and run:

```bash
cd integration && BASE_URL=http://127.0.0.1:8080 npm run test:list-flow
cd integration && BASE_URL=http://127.0.0.1:8080 npm run test:list-bulk-edit
cd integration && BASE_URL=http://127.0.0.1:8080 npm run test:list-view-rename
```

Expected: all pass. The legacy page now writes through the merge path and refetches on the events the merge emits.

- [ ] **Step 6: Commit**

```bash
git add ultros ultros-db
git commit -m "refactor(lists): every writer goes through the document merge path

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Websocket subscribe, update and relay

**Files:**
- Modify: `ultros/src/web/api/real_time_data.rs`

**Interfaces:**
- Consumes: `ListSync::{subscribe_payload, apply_update}`, `Actor`, `Origin::Socket`, `EventReceivers.list_docs`, `ListDocEvent`, the message types from Task 1.
- Produces: the handshake and relay behaviour of spec section 5; `fn relay_for(event: &ListDocEvent, list_id: i32, socket_id: u64) -> Option<Vec<u8>>`.

- [ ] **Step 1: Write the failing test for the relay filter**

Append to `real_time_data.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ListDocEvent;

    #[test]
    fn relay_skips_other_lists_and_the_sending_socket() {
        let event = ListDocEvent {
            list_id: 9,
            update: vec![1, 2, 3],
            origin_socket: Some(7),
        };
        assert_eq!(relay_for(&event, 9, 8), Some(vec![1, 2, 3]));
        assert_eq!(relay_for(&event, 9, 7), None, "the sender already has it");
        assert_eq!(relay_for(&event, 10, 8), None, "another list");
        let server_side = ListDocEvent {
            list_id: 9,
            update: vec![4],
            origin_socket: None,
        };
        assert_eq!(relay_for(&server_side, 9, 7), Some(vec![4]));
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p ultros --lib real_time_data`
Expected: FAIL, `cannot find function relay_for`.

- [ ] **Step 3: Implement**

Add the imports and the socket id source at the top of `real_time_data.rs`:

```rust
use std::sync::atomic::{AtomicU64, Ordering};

use crate::event::ListDocEvent;
use crate::lists::{Actor, ListSync, Origin};

/// Distinguishes sockets so a relay never echoes an update to its sender.
static NEXT_SOCKET_ID: AtomicU64 = AtomicU64::new(1);

fn relay_for(event: &ListDocEvent, list_id: i32, socket_id: u64) -> Option<Vec<u8>> {
    if event.list_id != list_id || event.origin_socket == Some(socket_id) {
        return None;
    }
    Some(event.update.clone())
}
```

Give the handler the service: add `State(list_sync): State<ListSync>,` to `real_time_data`'s parameters and pass it to `handle_socket(websocket, events, worlds, db, user, list_sync)`; add `list_sync: ListSync,` to `handle_socket`'s parameters. In `handle_socket`, destructure `list_docs,` from `EventReceivers` (replacing the placeholder from Task 4) and add `let socket_id = NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed);` after the destructuring.

Replace the temporary arm from Task 1 with:

```rust
                                ClientMessage::SubscribeListDoc {
                                    subscription_id,
                                    list_id,
                                    version,
                                } => {
                                    let subscription_id = subscription_id.unwrap_or_else(|| {
                                        let id = next_subscription_id;
                                        next_subscription_id += 1;
                                        id
                                    });
                                    if !activate_subscription(
                                        &active_subscriptions,
                                        subscription_id,
                                    ) {
                                        sender
                                            .send(Message::Text(
                                                serde_json::to_string(&ServerClient::Error {
                                                    message: format!(
                                                        "too many active subscriptions, max is {MAX_SUBSCRIPTIONS_PER_SOCKET}"
                                                    ),
                                                })?
                                                .into(),
                                            ))
                                            .await?;
                                        continue;
                                    }
                                    let user_id = user.as_ref().map(|u| u.id as i64).unwrap_or(0);
                                    match list_sync
                                        .subscribe_payload(list_id, user_id, &version)
                                        .await
                                    {
                                        Ok((server_version, payload)) => {
                                            let active = active_subscriptions.clone();
                                            let stream =
                                                BroadcastStream::new(list_docs.resubscribe())
                                                    .filter_map(move |event| {
                                                        let active = active.clone();
                                                        async move {
                                                            if !is_subscription_active(
                                                                &active,
                                                                subscription_id,
                                                            ) {
                                                                return None;
                                                            }
                                                            let event = match event {
                                                                Ok(event) => event,
                                                                Err(_) => {
                                                                    return Some(
                                                                        ServerClient::Stale {
                                                                            subscription_id,
                                                                        },
                                                                    );
                                                                }
                                                            };
                                                            let update = relay_for(
                                                                event.as_ref(),
                                                                list_id,
                                                                socket_id,
                                                            )?;
                                                            wrap_subscription_event(
                                                                subscription_id,
                                                                Some(ServerClient::ListDocUpdate {
                                                                    list_id,
                                                                    update,
                                                                }),
                                                            )
                                                        }
                                                    });
                                            subscriptions.push(Box::pin(stream));
                                            sender
                                                .send(Message::Text(
                                                    serde_json::to_string(
                                                        &ServerClient::ListDocSubscribed {
                                                            subscription_id,
                                                            list_id,
                                                            version: server_version,
                                                            payload,
                                                        },
                                                    )?
                                                    .into(),
                                                ))
                                                .await?;
                                        }
                                        Err(e) => {
                                            deactivate_subscription(
                                                &active_subscriptions,
                                                subscription_id,
                                            );
                                            sender
                                                .send(Message::Text(
                                                    serde_json::to_string(&ServerClient::Error {
                                                        message: format!(
                                                            "list {list_id}: {e}"
                                                        ),
                                                    })?
                                                    .into(),
                                                ))
                                                .await?;
                                        }
                                    }
                                }
                                ClientMessage::ListDocUpdate { list_id, update } => {
                                    let Some(user) = user.as_ref() else {
                                        sender
                                            .send(Message::Text(
                                                serde_json::to_string(&ServerClient::Error {
                                                    message: "sign in to edit lists".to_string(),
                                                })?
                                                .into(),
                                            ))
                                            .await?;
                                        continue;
                                    };
                                    let actor = Actor::from_user(user, Origin::Socket(socket_id));
                                    if let Err(e) =
                                        list_sync.apply_update(list_id, &actor, &update).await
                                    {
                                        sender
                                            .send(Message::Text(
                                                serde_json::to_string(&ServerClient::Error {
                                                    message: format!("list {list_id}: {e}"),
                                                })?
                                                .into(),
                                            ))
                                            .await?;
                                    }
                                }
```

`event.as_ref()` yields `&Arc<ListDocEvent>`, which derefs to `&ListDocEvent` for `relay_for`.

- [ ] **Step 4: Run the tests and the build**

Run: `cargo test -p ultros --lib real_time_data` and `cargo check -p ultros`
Expected: 1 passed; builds.

- [ ] **Step 5: Hand check the handshake with a websocket client**

With a test-auth server running and a logged-in cookie (`curl -c` from `/test/login?user_id=...&username=...`), connect with `websocat` and send:

```json
{"SubscribeListDoc":{"list_id":1,"version":""}}
```

Expected: a `ListDocSubscribed` reply whose `payload` is `{"Snapshot":"<base64>"}` and whose `version` is non-empty. Sending `{"ListDocUpdate":{"list_id":1,"update":"AAAA"}}` yields an `Error` mentioning `invalid document update`, and the server log shows no panic.

- [ ] **Step 6: Commit**

```bash
git add ultros/src/web/api/real_time_data.rs
git commit -m "feat(ws): list document subscribe, update and relay

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Full verification

- [ ] **Step 1: CI script**

Run: `./check_ci.sh > "$SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"`
Expected: `REAL_EXIT=0`. If clippy is OOM-killed (exit 137), re-run with `cargo clippy --all-targets -j 2 -- -D warnings`.

- [ ] **Step 2: Gated database tests**

Run: `MIGRATION_TEST_DATABASE_URL=postgres://... cargo test -p migration -p ultros-db -- --ignored --test-threads=1`
Expected: the Phase 3 tests pass (older ignored tests in those crates have their own prerequisites and are not part of this gate).

- [ ] **Step 3: End-to-end suites that touch lists**

Against a test-auth server: `test:list-flow`, `test:list-bulk-edit`, `test:list-view-rename`, `test:shared-list`, `test:group-shared-list`, `test:list-card-edit`.
Expected: all pass unchanged.

- [ ] **Step 4: Changelog entry**

Per `AGENTS.md`, this phase has no player-visible change on its own (the toggle only switches an identical page until Phase 4), so no entry is added yet; Phase 5 writes the entry for the whole experiment.

---

## Self-review

- Spec coverage: 4.1 storage, first touch and compaction (Tasks 2, 3); 4.2 the merge steps and both entry points (Task 3); the legacy-writer table (Task 5); 4.3 projection and the unique index with dedupe (Tasks 2, 3); 4.4 activity kinds, payloads and the bulk threshold (Task 4); 4.5 both buses and the capacity change (Task 4); 5 the handshake, permissions, base64 framing and relay (Tasks 1, 6); 6 the failure table (Tasks 3 and 6: forbidden, invalid, database errors reply with `Error` and store nothing). The spec's "grep-based test" for the row-writer invariant is superseded by deleting the writers (Task 5), which the compiler enforces; the spec's Risks section is updated by Phase 5.
- Placeholders: none. The alert target-price path in the spec's table has no caller in the codebase (`set_list_item_target_price` is dead), so it is deleted rather than rerouted.
- Type consistency: `Actor { user_id: i64, username: String, origin }` and `Origin::{Socket(u64), Rest, Bot}` are used identically in Tasks 4, 5, 6; `edit_as_server` returns `(R, Applied)` everywhere; `ListDocError` variants match between Task 3 and the error mapping in Task 4; `ListDocEvent.origin_socket: Option<u64>` matches `relay_for`.
