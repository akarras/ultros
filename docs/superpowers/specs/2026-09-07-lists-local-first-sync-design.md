# Lists local-first sync — design spec (2026-09-07)

First of four Lists 2.0 sub-projects. This one replaces the list page's data
layer with a local-first CRDT document synced through the server, shipped
behind a Labs toggle with no visual redesign. The three that follow build on
it: the build-mode redesign (skeleton, click-to-edit rows, selection bar,
toasts), shopping mode on the recipe planner engine, and the external API plus
Dalamud companion plugin.

## Problem

The list page (`/list/:id`, `ultros-frontend/ultros-app/src/routes/list_view.rs`)
treats the server as the only copy of the list:

1. Every edit is a request followed by a refetch of the whole list plus every
   listing for it. Mutation endpoints return unit. There is no optimistic
   update, so the page feels like a form, not a tool.
2. Websocket `ListUpdate` payloads are discarded in favour of another full
   refetch, and the list event bus holds 40 messages, so bursts degrade into
   `Stale` refetch storms.
3. Undo does not exist and has nothing to attach to: there is no client-side
   document, and the server is last-write-wins with no version.
4. Nothing works offline, and two people editing one shared list clobber each
   other silently.

The server should do what only it can: authenticate, enforce sharing and
permissions, keep every device converged, and feed the parts of the product
that read list rows (the Discord bot, price alerts, the activity feed).
Everything else belongs in the browser.

## Decisions already made

Settled during the brainstorm and not reopened here:

- The list document is a CRDT built on the `loro` crate (pinned `=1.16.0`,
  default features, which include `counter`). Measured cost in a size-optimised
  wasm build: 1.99 MB raw, 703 KB compressed, on top of a production bundle of
  16.8 MB raw and 4.9 MB compressed. Accepted in exchange for a native counter,
  a local undo manager, version-vector sync and shallow snapshots.
- Items are organised by sort only. No manual order, no sections, so no
  sequence CRDT is involved.
- The visual redesign is a separate spec. This spec keeps the existing page and
  every existing component, swapping only where its data comes from.
- Ships behind Labs, mirroring the recipe analyzer's toggle, so the current
  feature keeps working untouched while this dials in.

## Design

### 1. Document schema

One Loro document per list. Both the server and the browser use the same typed
wrapper, `ListDocument`, from a new workspace crate `ultros-list-doc`
(depends on `loro`, `serde`, `ultros-api-types`; no Leptos, no database).

```
root map "meta"
  name:    string
  scope:   string   AnySelector encoded as "world:79" | "datacenter:5" | "region:1"
  schema:  integer  1

root map "rows"
  key "{item_id}:{quality}"   quality ∈ any | hq | nq
  value: LoroMap
    item:     integer
    quality:  string     any | hq | nq   (mirrors ListItem.hq: None | Some(true) | Some(false))
    need:     integer    ≥ 0; a legacy NULL quantity imports as 1
    target:   integer    absent = no target price
    acquired: LoroCounter   a legacy NULL acquired imports as 0
```

- `acquired` is a counter container, so a purchase ticked on two devices, or
  by two people, counts twice instead of one overwriting the other. Setting an
  absolute value (the legacy REST path, "got all", a typed number) is an
  increment of `new - current` by the writing peer.
- Removing a row is `rows.delete(key)`. A later add creates a fresh row under
  the same key. Loro's map is last-writer-wins per key, so a concurrent remove
  and add resolve by timestamp, and an edit inside a row that someone
  concurrently removed is lost with the row. That is the intended
  "remove wins" behaviour for a shopping list.
- Changing quality moves the row: delete the old key, insert a new row with the
  same fields under the new key. Undo restores both halves.
- Relational row ids (`list_item.id`) never enter the document. They are an
  artefact of the projection (section 4.3).

`ListDocument` API (names are binding for the plan):

```rust
pub struct RowKey { pub item_id: i32, pub quality: Quality }
pub enum Quality { Any, Hq, Nq }              // From/Into Option<bool>
pub struct RowSnapshot { pub key: RowKey, pub need: i64, pub acquired: i64, pub target: Option<i64> }
/// `scope` is `None` only for a document nobody wrote a scope into; consumers
/// treat that as "leave the list's scope alone".
pub struct MetaSnapshot { pub name: String, pub scope: Option<AnySelector> }

impl ListDocument {
    pub fn new() -> Self;                                // LoroDoc::new, Loro's default random peer id
    pub fn from_snapshot(bytes: &[u8]) -> Result<Self, DocError>;
    pub fn from_rows(meta: MetaSnapshot, rows: &[RowSnapshot]) -> Self;

    pub fn meta(&self) -> MetaSnapshot;
    pub fn rows(&self) -> Vec<RowSnapshot>;               // via get_deep_value, sorted by key
    pub fn row(&self, key: &RowKey) -> Option<RowSnapshot>;

    pub fn rename(&self, name: &str);
    pub fn set_scope(&self, scope: AnySelector);
    pub fn add_row(&self, key: RowKey, need: i64, target: Option<i64>);   // merges need into an existing key
    pub fn remove_row(&self, key: &RowKey);
    pub fn set_need(&self, key: &RowKey, need: i64);
    pub fn set_target(&self, key: &RowKey, target: Option<i64>);
    pub fn set_quality(&self, key: &RowKey, quality: Quality) -> RowKey;  // the move
    pub fn add_acquired(&self, key: &RowKey, delta: i64);
    pub fn set_acquired(&self, key: &RowKey, value: i64);                 // increment by value - current
    pub fn commit(&self);

    pub fn version(&self) -> Vec<u8>;                     // oplog_vv().encode()
    pub fn export_snapshot(&self) -> Result<Vec<u8>, DocError>;   // ExportMode::Snapshot
    pub fn export_shallow(&self) -> Result<Vec<u8>, DocError>;    // ExportMode::shallow_snapshot(&state_frontiers())
    pub fn export_all(&self) -> Result<Vec<u8>, DocError>;        // ExportMode::all_updates()
    pub fn export_since(&self, version: &[u8]) -> Result<Vec<u8>, DocError>;  // ExportMode::updates(&vv); empty = everything
    pub fn sync_payload(&self, client_version: &[u8]) -> Result<SyncPayload, DocError>;  // Snapshot | Updates | UpToDate
    pub fn is_ahead_of(&self, version: &[u8]) -> bool;
    pub fn import(&self, bytes: &[u8]) -> Result<ImportReport, DocError>;     // wraps ImportStatus
    pub fn on_local_update(&self, f: impl Fn(&[u8]) + Send + Sync + 'static) -> Subscription;  // subscribe_local_update
    pub fn on_change(&self, f: impl Fn() + Send + Sync + 'static) -> Subscription;             // subscribe_root
}

pub fn diff_rows(before: &[RowSnapshot], after: &[RowSnapshot]) -> Vec<RowChange>;
pub enum RowChange { Added(RowSnapshot), Removed(RowSnapshot), Updated { before: RowSnapshot, after: RowSnapshot } }
```

Every mutating method commits its own transaction so that Loro's undo manager
and `subscribe_local_update` see one operation per user action. Callers that
want several changes in one undo step, such as a bulk HQ change or a
MakePlace import, wrap them in `UndoManager::group_start` / `group_end`.

### 2. Peers and identity

- Every document gets Loro's default random peer id when it is created, in
  the browser and on the server alike. Nothing stores or reuses a peer id:
  Loro's own guidance is that a peer id shared by two concurrent writers,
  such as two tabs of one browser, can corrupt a document. The undo stack is
  per open page anyway, so a fresh id per page load costs nothing.
- The server creates a fresh document, and therefore a fresh peer id, for
  every merge it performs on behalf of a legacy writer.
- Loro records wall-clock timestamps on changes. On wasm it reads `Date.now`
  through wasm-bindgen, which the hydrate build already links.

### 3. Client

#### 3.1 Local-first document

`ultros-frontend/ultros-app/src/list_doc/` owns the browser side:

- `store.rs`: persistence. Snapshots live in `localStorage` under
  `ultros.listdoc.v1.{list_id}` as base64, with `ultros.listdoc.index` holding
  last-used timestamps and the last known `ListPermission` per list. At most
  20 lists are kept; saving a 21st evicts the least recently used. No peer id
  is stored (section 2). Saves happen on a 500 ms debounce after any change and
  on `visibilitychange` to hidden. A save that fails (quota, private mode) is
  logged and ignored; the document keeps working in memory.
- `sync.rs`: the socket client (section 5). It exposes a
  `SyncStatus` signal reusing today's `RealtimeStatus` values so the existing
  Live badge works unchanged.
- `handle.rs`: `ListDocHandle`, a `Copy` handle stored in context that wraps
  the document, a `RwSignal<u64>` revision bumped from `on_change`, and the
  undo manager. Components read `rows()` and `meta()` through memos on the
  revision.
- `undo.rs`: the undo manager and key handling (section 3.3).

Page open sequence:

1. Read the local snapshot. If present, build the document and render the
   page from it immediately, using the permission cached beside it (section
   3.1, `store.rs`) so a writable list stays editable offline.
2. Fetch the list's permission and owner name through the existing
   `GET /api/v1/list/{id}` (a client wrapper is added; the route exists with
   `RequireListPermission<READ>`). On success, refresh the cached permission.
   If it fails with no local snapshot, show today's error state. If it fails
   with a snapshot, keep rendering from the snapshot with the `offline`
   status.
3. Connect and subscribe with the local version (section 5). Apply what the
   server sends; send what it lacks.
4. Fetch listings through the existing `GET /api/v1/list/{id}/listings` as
   today. Prices need the network. The list itself does not.

#### 3.2 Adapter for the existing page

Under the Labs toggle the route renders `ListViewSync`, a copy of `ListView`
whose resource is replaced by the document. To keep every existing component
untouched, an adapter produces today's shapes from the document:

- `ListItem { id, item_id, list_id, hq, quantity, acquired, target_price }`
  is built per row, with `id` a stable 32-bit FNV hash of the row key. This id
  exists only for `<For>` keys and row callbacks on the Labs page; the Labs
  page never sends it to a REST endpoint.
- The `Action`s the rows and modals dispatch (`add_item`, `edit_item`,
  `delete_item`, `delete_items`, `edit_items_hq`, `edit_list_action`) are
  replaced by actions of the same signatures whose bodies mutate the document.
  A quantity edit becomes `set_need`, an acquired edit `set_acquired`, HQ
  `set_quality`, target price `set_target`, rename `rename`, and world change
  `set_scope`. Bulk HQ and bulk delete wrap their loop in an undo group.
- Bulk add paths (`AddRecipeToCurrentListModal`, `MakePlaceImporter`,
  `AddToList` from other pages) keep using REST. Their writes reach the
  document through the server merge path and arrive on the socket as a delta,
  so the Labs page still updates live. Converting those modals to write the
  document directly is left to the redesign spec.
- `AutoMarkPurchases` keeps its sale-history subscription and calls
  `add_acquired(key, 1)` instead of mutating the resource.
- `BuyingView`'s "Mark purchased" calls `add_acquired`.

The non-Labs page is byte-for-byte unchanged.

#### 3.3 Undo and keys

- One `UndoManager::new(&doc)` per open list, `set_merge_interval(1000)`,
  `set_max_undo_steps(100)`. Imports from the socket are remote operations
  and never enter the stack, so undo only ever reverts this device's edits.
- Keys: `Ctrl+Z` undo, `Ctrl+Shift+Z` and `Ctrl+Y` redo, `Cmd` in place of
  `Ctrl` when `PlatformHotkeys.apple` is set. One `keydown` listener on
  `window`, registered by `ListViewSync`, ignored when the event target is an
  `input`, `textarea`, `select` or `contenteditable` element, or when any
  `Modal` is open. A pure `classify_key(event) -> Option<UndoKey>` function
  carries the rules and is unit tested.
- Undo of a key that another peer overwrote after your edit: Loro's manager
  transforms stack items against remote diffs. Phase 1 pins the observed
  behaviour with a test (`undo_after_remote_overwrite`) and this spec adopts
  whatever it is, documented in the crate. The plan does not add a guard
  layer on top.
- No toast on undo in this spec. Toast actions belong to the redesign.

### 4. Server as a peer

#### 4.1 Storage

New table `list_doc` (migration `m20260907_000001_list_doc`):

| column | type | notes |
|---|---|---|
| `list_id` | integer PK, FK `list.id` ON DELETE CASCADE | |
| `snapshot` | bytea NOT NULL | latest `ExportMode::Snapshot` or shallow snapshot |
| `version` | bytea NOT NULL | encoded `oplog_vv` of the stored snapshot |
| `changes_since_compaction` | integer NOT NULL DEFAULT 0 | |
| `updated_at` | timestamptz NOT NULL DEFAULT now() | |

Lists that predate the table get a document built with
`ListDocument::from_rows` on first touch (subscribe or legacy write) inside
the same locked transaction, so two first touches cannot race. No bulk
backfill is needed.

Compaction: when `changes_since_compaction` exceeds 5,000 or the snapshot
exceeds 256 KB, the merge path stores `export_shallow()` instead of a full
snapshot and resets the counter. Initial sync (section 5) always sends a
snapshot to a client whose version is empty, so trimmed history never leaves
an import pending on a fresh client.

#### 4.2 The single merge path

`ultros/src/lists/sync.rs` exposes one entry point that every writer uses:

```rust
pub async fn apply_update(&self, list_id: i32, actor: Actor, update: &[u8]) -> Result<Applied, ListSyncError>;
pub async fn edit_as_server<R>(&self, list_id: i32, actor: Actor, f: impl FnOnce(&ListDocument) -> R) -> Result<(R, Applied), ListSyncError>;

pub struct Actor { pub user_id: i64, pub username: String, pub origin: Origin }  // Origin::Socket(socket_id) | Origin::Rest | Origin::Bot | Origin::Alerts
pub struct Applied { pub relay: Vec<u8>, pub changes: Vec<RowChange>, pub meta_changed: bool }
```

`apply_update`, in one database transaction:

1. `SELECT ... FOR UPDATE` on `list_doc` (creating it from rows if absent).
2. Load `ListDocument::from_snapshot`, take `rows()` and `meta()` as *before*.
3. `import(update)`. A decode failure returns `ListSyncError::InvalidUpdate`.
4. Compare `meta()` with *before*. If it changed and the actor is not the
   list owner, return `ListSyncError::Forbidden`. The document was loaded
   for this merge only and nothing has been stored, so rejecting is just not
   storing; the transaction rolls back.
5. Take *after* snapshots, `diff_rows`, and update the projection (4.3).
6. Store the new snapshot and version, bump `changes_since_compaction`, and
   compact if due.
7. Record activity (4.4) and emit events (4.5).

`edit_as_server` loads the document, records `version()`, runs the closure,
then calls `apply_update` with `export_since(version)`. Legacy writers use it:

| today's write | becomes |
|---|---|
| `db.add_item_to_list` (REST `add/item`, bot `/list add_item`) | `edit_as_server` → `add_row` |
| `db.add_items_to_list` (REST `add/items`) | `edit_as_server` → `add_row` per item |
| `db.update_list_item` (REST `item/edit`) | `set_need` / `set_acquired` / `set_target` / `set_quality` by diff against the row |
| `db.set_list_items_hq` (REST `item/hq`) | `set_quality` per id |
| `db.remove_item_from_list` (REST delete, bot `/list remove_item`) | `remove_row` |
| `db.set_list_item_target_price` (alerts) | `set_target` |
| `db.update_list` (REST `list/edit`, bot) | `rename` / `set_scope` |

The `ultros-db` functions stay as the projection's row writers and lose their
public role as the write API: the web handlers, the bot commands and the alert
path call `ListSync` instead. `delete_list` is unchanged; the cascade removes
the document.

#### 4.3 Projection

`list_item` remains the read model for everything that is not the Labs page:
the legacy page, `GET /api/v1/list/{id}/listings`, the Discord bot, the
price-alert tracker (`get_list_items_with_target`) and the list index. After
each merge, `RowChange`s are applied to it by natural key `(list_id, item_id,
hq)`: added rows insert, removed rows delete, updated rows set the changed
columns. Row ids stay stable for the life of a row. A unique index on
`(list_id, item_id, hq)` is added in the same migration; it is the invariant
the projection relies on and the application code already assumes.

Nothing writes `list_item` outside the merge path once this ships. The two
`#[ignore]`d migration tests gain a sibling that asserts the unique index
exists.

#### 4.4 Activity

Each `RowChange` records one `list_activity` row with today's kinds and
payloads: `ItemAdded`, `ItemRemoved`, `ItemUpdated` with the
`item_change_payload` before/after map, and `ItemAcquired` when `acquired`
crosses from below `need` to at least `need`. `meta` changes record
`ListUpdated`. When one update changes more than 10 rows of the same kind,
one summary row is recorded instead, `{"bulk": true, "count": n}`, matching
what bulk add and bulk HQ record today. The actor is the socket user, the REST
user, the bot's author, or the alerts system user.

#### 4.5 Events

- Today's `ListEventData` events are still emitted per changed row so the
  legacy page refetches and `list_update_alert_tracker` keeps firing. The
  `lists` bus capacity rises from 40 to 1,024.
- A new bus `EventSenders.list_docs: EventProducer<ListDocEvent>` with
  capacity 1,024 carries `{ list_id, update: Arc<Vec<u8>>, origin }`. Socket
  subscribers relay `update` to every subscribed client except the one whose
  `origin` sent it.

### 5. Websocket protocol

Additions to `ultros-api-types::websocket`, on the existing JSON framing with
bytes as base64 strings (`serde` `with` helper in the same module):

```rust
ClientMessage::SubscribeListDoc { subscription_id: Option<u64>, list_id: i32, version: Base64 }
ClientMessage::ListDocUpdate   { list_id: i32, update: Base64 }

ServerClient::ListDocSubscribed { subscription_id: u64, list_id: i32, version: Base64, payload: ListDocPayload }
ServerClient::ListDocUpdate     { list_id: i32, update: Base64 }

enum ListDocPayload { Snapshot(Base64), Updates(Base64), UpToDate }
```

Handshake:

1. Client sends `SubscribeListDoc` with its `version` (empty for a first
   visit). Requires `ListPermission::Read`, checked as `SubscribeList` does.
2. Server replies `ListDocSubscribed` with its own `version` and either a
   `Snapshot` (client version empty or unknown to the server), `Updates`
   (`export_since(client_version)`), or `UpToDate`.
3. Client imports the payload, then sends `ListDocUpdate` with
   `export_since(server_version)` if that is non-empty.
4. From here, every local commit goes out as `ListDocUpdate` through
   `on_local_update`; every relayed `ListDocUpdate` is imported. `Unsubscribe`
   reuses the existing message.

An update requires `ListPermission::Write` and goes through `apply_update`
with `Origin::Socket`. On `ListSyncError` the server replies
`ServerClient::Error { message }` wrapped in the subscription's
`SubscriptionEvent` and keeps its state; the client's operations remain in
its document and are retried by the next handshake. Reconnects run the handshake again, which is how offline
edits converge: no separate queue exists. `Stale` on the subscription also
triggers a fresh handshake.

The existing `SubscribeList` subscription is not used by the Labs page. The
market-listing subscription and its `is_list_market_update_relevant` refetch of
listings stay as they are.

### 6. Permissions and failure handling

| situation | behaviour |
|---|---|
| subscribe without read | `Error`, no subscription |
| update without write | `Error`, nothing stored |
| update changes `meta` by a non-owner | `Forbidden`, nothing stored |
| update fails to decode | `InvalidUpdate`, nothing stored |
| database unavailable during merge | `Error`; the client keeps local operations; the handshake retries |
| local storage quota exceeded | in-memory only for that session, console warning |
| socket offline at page open with a local snapshot | page renders from the snapshot, status `offline`, edits accepted |
| socket offline with no local snapshot | today's error state |
| a peer's undo targets a key changed remotely | Loro's transform, pinned by test in phase 1 |

### 7. Labs gate

Restore `ultros-frontend/ultros-app/src/global_state/labs.rs` and the
`LabsSettings` section of `routes/settings.rs` from `bc2b1df2^` (PR #1305
deleted them when the recipe experiment shipped), with one token:

```rust
pub const LAB_LISTS_SYNC: &str = "lists-sync";
```

The `LABS` cookie and the `?labs=lists-sync` URL override work as before. In
`lib.rs` the `/list/:id` route renders `ListViewSync` when `use_lab(LAB_LISTS_SYNC)`
is true and `ListView` otherwise. The list index at `/list` is unchanged.
Shipping the experiment deletes the old `ListView`, the REST-driven actions it
owns, and the token; this spec does not schedule that.

### 8. Testing

Unit, `cargo test`:

- `ultros-list-doc`: key encoding round trips; every typed operation;
  `set_quality` moves without losing fields; `add_row` on an existing key
  merges `need`; three peers applying a seeded random sequence of operations
  and importing each other's updates in every order converge to identical
  `rows()` and `meta()`; `diff_rows` classifies added, removed and updated
  including counter changes; snapshot, shallow snapshot and `export_since`
  round trips; a shallow snapshot imports cleanly into a fresh document.
- Undo, in the same crate: undo and redo of add, remove, need, target,
  quality move and counter increments; grouped bulk change undoes as one;
  remote imports never enter the stack; `undo_after_remote_overwrite` pins
  the transform behaviour.
- `ultros`: `apply_update` merge path against a test database (behind the
  existing `MIGRATION_TEST_DATABASE_URL` gate): projection rows after add,
  update, remove and quality move; activity kinds and bulk summarisation;
  owner-only `meta`; first-touch creation from legacy rows; compaction
  threshold; unique index present.
- `ultros-app`: `classify_key` for every binding, editable targets and the
  Apple modifier; the adapter's `ListItem` mapping; `LabsSettings` snapshot;
  storage LRU eviction with a fake storage.

Integration, `integration/list-sync.cjs` under `test-auth` with the
`LABS=lists-sync` cookie:

- Two browser contexts on one shared list: an edit in A appears in B without
  a reload, and the activity feed in B records A's user.
- A goes offline (`page.setOfflineMode(true)`), edits, comes back: both
  contexts and the server converge, the legacy page shows the same rows.
- A REST write from a third client (today's `add/item`) appears in A.
- `Ctrl+Z` in A reverts A's last edit on the server and in B; `Ctrl+Y` redoes.
- `list-flow.cjs` runs twice, with and without the Labs cookie, and passes
  both times. The test ids it depends on are unchanged.

Bundle: `target/site/pkg/ultros.wasm` size raw and `gzip -9`, before and
after, recorded in the PR. Expected increase at most 750 KB compressed.

### 9. Budget and CI

- `loro` and its 146 transitive crates compile for the server binary too.
  The first PR watches clippy memory; the documented `cargo clippy -j 2`
  fallback exists if CI needs it.
- Pin `loro = "=1.16.0"`. Bumps are deliberate, with the convergence tests as
  the gate.

### 10. Phasing for the plan

1. **Labs restore.** The module, settings section, token, route split with a
   placeholder `ListViewSync` identical to `ListView`. Ships alone.
2. **`ultros-list-doc` crate.** Schema, wrapper, diff, undo tests,
   convergence tests. No consumers yet.
3. **Server peer.** Migration, `ListSync`, projection, activity, events,
   legacy writers rerouted, socket messages. Legacy page still serves
   everything; integration tests prove REST and bot writes flow through the
   document.
4. **Client.** Store, sync, handle, undo and keys, the adapter, `ListViewSync`
   on the document. End-to-end suite.
5. **Promotion readiness.** Bundle numbers, changelog entry, a week on prod
   under Labs with shared lists, then the redesign spec starts.

## Out of scope

- Any visual change to the list page, the header, rows, toolbar or modes.
- The list index at `/list` going local-first.
- Shopping mode, the planner engine, travel routes.
- API tokens, CORS, the Dalamud plugin, inventory sync.
- Toast actions, keyboard shortcuts beyond undo and redo.
- Deleting the legacy `ListView` and its REST-driven actions.
- Cross-tab undo history, persisting the undo stack.
- Server-side revert of another user's change from the activity feed.

## Risks

- Loro is younger than yrs or automerge and its API moves; the exact pin and
  the convergence suite are the guard.
- The projection is correct only while nothing writes `list_item` directly.
  A grep-based test in `ultros` asserts that the only callers of the
  `ultros-db` row writers are inside `lists/sync.rs`.
- Base64 inside JSON frames costs a third more bytes than binary frames.
  Updates are tiny, so this is accepted; switching to binary frames is a
  transport change for later.
- Local storage is per browser profile; a user with two browsers has two
  peers, which is correct and slightly surprising when their undo stacks
  differ.
