# Lists Sync Phase 4: Client Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Under the `lists-sync` Labs toggle, the list page runs on a local Loro document: it renders from a browser-stored snapshot, syncs over the websocket, accepts edits offline, and undoes and redoes with the keyboard. Every existing list component is reused unchanged.

**Architecture:** A new `list_doc` module owns the browser side: `store` (localStorage with an LRU cap), `adapter` (today's `ListItem` shapes and an `Edit` enum applied as one undo step each), `handle` (`ListDocHandle`, a `Copy` handle wrapping the document, undo manager, a revision signal and an outbox), `sync` (handshake and steady state on the existing realtime client), and `undo` (key classification and the window listener). `ListViewSync` is a copy of `ListView` whose data source and actions are swapped; its `Resource` still produces the old result type so the table, rows, buying view, summary and drawers render as they always did.

**Tech Stack:** Leptos 0.8, leptos-use (`use_event_listener_with_options`, `use_window`, `use_document`), `ultros-list-doc`, `base64 0.22`, gloo-timers, Puppeteer.

## Global Constraints

- **Required before implementing Task 3:** Replace the `row_id` hash sketch
  with collision-free identity for row keys and callbacks. The shown 31-bit
  FNV mapping gives both `21482:any` and `41373:hq` the ID `1708812798`, so
  `find_key` can edit or delete the wrong row. Carry the canonical `RowKey`
  through callbacks, or maintain an explicit one-to-one ID mapping for the
  open list; never assume a hash is unique. Add a regression that puts this
  pair in one list and independently edits, removes, and restores each row.

- **Required before implementing Tasks 2, 5, and 9:** Scope snapshot keys,
  indexes, and in-memory handles to the authenticated user as well as the list.
  The single-user storage and fallback snippets below are incomplete sketches:
  adapt their signatures and callers rather than copying the list-id-only keys
  or treating every API error as offline. Logout/account changes must close the
  old handle and subscription. Explicit authentication/permission denial or a
  deleted-list response must purge the affected cached snapshot and permission,
  clear the rendered document, and stop syncing. Only transient transport/server
  failures may use the same user's offline cache. Gate this with browser tests
  for account A switching to B, permission revocation, list deletion, and a real
  offline-to-online reconnect that preserves authorized edits.

- Spec: `docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md`, sections 3 and 8; deviations recorded here and in the spec by Task 10: `AutoMarkPurchases` gains an optional `on_purchase` callback instead of calling the document directly, and the Labs page keeps a `Resource` (built from the document) so `AutoMarkPurchases` and the `Transition` body stay untouched.
- The non-Labs page is byte-for-byte unchanged, except `AutoMarkPurchases` gaining an optional prop whose absence preserves today's behaviour.
- All new user-facing strings in all seven locales. This phase adds none: status words reuse `RealtimeStatus`'s existing vocabulary.
- Test ids and selectors the e2e suites depend on are preserved (`.list-toolbar`, `list-settings-btn`, `list-rename-btn`, `list-auto-mark-btn`, `list-filter-row`, `button[data-datacenter]`, `realtime-status-indicator`, `button[aria-label="Mark as acquired"]`).
- Run `./check_ci.sh` before every commit; commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

---

## File structure

| File | Responsibility |
|---|---|
| `ultros-list-doc/src/document.rs`, `src/lib.rs` (modify) | `is_ahead_of`, re-export `Subscription` |
| `ultros-frontend/ultros-app/Cargo.toml` (modify) | `ultros-list-doc`, `base64` |
| `ultros-frontend/ultros-app/src/error.rs` (modify) | `AppError::ListDoc` |
| `ultros-frontend/ultros-app/src/list_doc/mod.rs` (create) | module list |
| `ultros-frontend/ultros-app/src/list_doc/store.rs` (create) | persistence, LRU, permission cache |
| `ultros-frontend/ultros-app/src/list_doc/adapter.rs` (create) | `row_id`, `to_list_item`, `view_result`, `Edit`, `apply` |
| `ultros-frontend/ultros-app/src/list_doc/handle.rs` (create) | `ListDocHandle` |
| `ultros-frontend/ultros-app/src/ws/realtime.rs` (modify) | `subscribe_list_doc`, `send_list_doc_update` |
| `ultros-frontend/ultros-app/src/list_doc/sync.rs` (create) | handshake, relay, outbox drain |
| `ultros-frontend/ultros-app/src/list_doc/undo.rs` (create) | `classify_key`, `install` |
| `ultros-frontend/ultros-app/src/components/list/auto_mark_purchases.rs` (modify) | `on_purchase` prop |
| `ultros-frontend/ultros-app/src/routes/list_view_sync.rs` (replace) | `ListViewSync`, `ListRoute` |
| `ultros-frontend/ultros-app/src/lib.rs` (modify) | `mod list_doc;` |
| `integration/list-sync.cjs` (create), `integration/package.json`, `scripts/run_e2e.sh` (modify) | two-browser convergence suite |

---

### Task 1: Crate additions and the error variant

**Files:**
- Modify: `ultros-list-doc/src/document.rs`, `ultros-list-doc/src/lib.rs`
- Modify: `ultros-frontend/ultros-app/Cargo.toml`
- Modify: `ultros-frontend/ultros-app/src/error.rs`

**Interfaces:**
- Produces: `ListDocument::is_ahead_of(&self, version: &[u8]) -> bool`; `pub use loro::Subscription;` from `ultros_list_doc`; `AppError::ListDoc(String)` with `From<ultros_list_doc::DocError>`.

- [ ] **Step 1: Test and implement `is_ahead_of`**

In `document.rs` tests:

```rust
    #[test]
    fn is_ahead_of_reports_unsent_local_work() {
        let a = ListDocument::from_rows(meta(), &[row(1, Quality::Any, 1, 0)]);
        let b = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
        assert!(!a.is_ahead_of(&b.version()));
        a.set_need(&RowKey::new(1, None), 2).unwrap();
        assert!(a.is_ahead_of(&b.version()));
        b.import(&a.export_since(&b.version()).unwrap()).unwrap();
        assert!(!a.is_ahead_of(&b.version()));
        assert!(a.is_ahead_of(b"junk"), "an unreadable version gets everything");
        assert!(a.is_ahead_of(&[]));
    }
```

Then in `impl ListDocument`:

```rust
    /// True when this document holds operations the holder of `version` has
    /// not seen: there is something to send it.
    pub fn is_ahead_of(&self, version: &[u8]) -> bool {
        let Ok(theirs) = VersionVector::decode(version) else {
            return true;
        };
        let mine = self.doc.oplog_vv();
        mine.iter()
            .any(|(peer, counter)| theirs.get(peer).is_none_or(|seen| seen < counter))
    }
```

If `iter()` does not resolve on `VersionVector`, replace the body's last statement with `!theirs.includes_vv(&mine)`; both express the same test.

Add `pub use loro::Subscription;` to `lib.rs` (the browser stores subscriptions without depending on `loro` itself).

Run: `cargo test -p ultros-list-doc is_ahead_of`; expected 1 passed.

- [ ] **Step 2: Frontend dependencies and error**

`ultros-frontend/ultros-app/Cargo.toml` under `[dependencies]`:

```toml
ultros-list-doc = { path = "../../ultros-list-doc" }
base64 = "0.22.1"
```

In `error.rs`, add a variant to `AppError` after `InternalApiTimeout`:

```rust
    /// The local list document refused an edit (spec section 3.2).
    #[error("List document: {0}")]
    ListDoc(String),
```

and after the enum:

```rust
impl From<ultros_list_doc::DocError> for AppError {
    fn from(error: ultros_list_doc::DocError) -> Self {
        AppError::ListDoc(error.to_string())
    }
}
```

If `error.rs` matches exhaustively on `AppError` anywhere (search `match` arms over it), add the new arm treating it like `Json`.

- [ ] **Step 3: Build both halves and commit**

Run: `cargo check -p ultros-app` and `cargo check -p ultros-app --features hydrate --no-default-features`
Expected: both succeed (the hydrate build now compiles Loro to wasm; expect a few minutes the first time).

```bash
git add ultros-list-doc ultros-frontend/ultros-app/Cargo.toml ultros-frontend/ultros-app/src/error.rs Cargo.lock
git commit -m "feat(app): list-doc dependency, is_ahead_of, ListDoc error

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Persistence with an LRU cap

**Files:**
- Create: `ultros-frontend/ultros-app/src/list_doc/mod.rs`, `ultros-frontend/ultros-app/src/list_doc/store.rs`
- Modify: `ultros-frontend/ultros-app/src/lib.rs` (module list)

**Interfaces:**
- Produces: `trait Storage { get, set -> bool, remove }`, `BrowserStorage`, `MAX_LISTS = 20`, `Loaded { snapshot: Vec<u8>, permission: i16 }`, `load(&impl Storage, list_id) -> Option<Loaded>`, `save(&impl Storage, list_id, snapshot, permission, now_ms) -> bool`, `remember_permission(&impl Storage, list_id, permission, now_ms) -> bool`, `now_ms() -> f64`.

- [ ] **Step 1: Module skeleton**

`list_doc/mod.rs`:

```rust
//! The browser side of the local-first list document (spec section 3).

pub mod store;
```

Add `mod list_doc;` to `lib.rs` next to `mod ws;` (or wherever the crate's private modules are declared; `pub(crate)` visibility is fine).

- [ ] **Step 2: Write `store.rs` with tests**

```rust
//! Browser persistence for list documents (spec section 3.1): one snapshot
//! per list under `ultros.listdoc.v1.{id}`, an index of last use and last
//! known permission, and eviction beyond `MAX_LISTS`.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

pub const MAX_LISTS: usize = 20;
const DOC_PREFIX: &str = "ultros.listdoc.v1.";
const INDEX_KEY: &str = "ultros.listdoc.index";

pub trait Storage {
    fn get(&self, key: &str) -> Option<String>;
    /// False when the write did not stick (quota, private mode).
    fn set(&self, key: &str, value: &str) -> bool;
    fn remove(&self, key: &str);
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub last_used_ms: f64,
    pub permission: i16,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub lists: BTreeMap<i32, IndexEntry>,
}

pub struct Loaded {
    pub snapshot: Vec<u8>,
    pub permission: i16,
}

fn doc_key(list_id: i32) -> String {
    format!("{DOC_PREFIX}{list_id}")
}

pub fn read_index(storage: &impl Storage) -> Index {
    storage
        .get(INDEX_KEY)
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_index(storage: &impl Storage, index: &Index) -> bool {
    serde_json::to_string(index)
        .map(|text| storage.set(INDEX_KEY, &text))
        .unwrap_or(false)
}

pub fn load(storage: &impl Storage, list_id: i32) -> Option<Loaded> {
    let text = storage.get(&doc_key(list_id))?;
    let snapshot = STANDARD.decode(text).ok()?;
    let permission = read_index(storage)
        .lists
        .get(&list_id)
        .map(|entry| entry.permission)
        .unwrap_or(0);
    Some(Loaded {
        snapshot,
        permission,
    })
}

/// Save a snapshot, touch the index, and evict the least recently used lists
/// beyond `MAX_LISTS`. The document keeps working in memory when this fails.
pub fn save(
    storage: &impl Storage,
    list_id: i32,
    snapshot: &[u8],
    permission: i16,
    now_ms: f64,
) -> bool {
    if !storage.set(&doc_key(list_id), &STANDARD.encode(snapshot)) {
        return false;
    }
    let mut index = read_index(storage);
    index.lists.insert(
        list_id,
        IndexEntry {
            last_used_ms: now_ms,
            permission,
        },
    );
    while index.lists.len() > MAX_LISTS {
        let Some((&oldest, _)) = index
            .lists
            .iter()
            .min_by(|a, b| a.1.last_used_ms.total_cmp(&b.1.last_used_ms))
        else {
            break;
        };
        index.lists.remove(&oldest);
        storage.remove(&doc_key(oldest));
    }
    write_index(storage, &index)
}

pub fn remember_permission(
    storage: &impl Storage,
    list_id: i32,
    permission: i16,
    now_ms: f64,
) -> bool {
    let mut index = read_index(storage);
    let entry = index.lists.entry(list_id).or_default();
    entry.permission = permission;
    entry.last_used_ms = now_ms;
    write_index(storage, &index)
}

pub struct BrowserStorage;

impl Storage for BrowserStorage {
    fn get(&self, key: &str) -> Option<String> {
        #[cfg(not(feature = "ssr"))]
        {
            web_sys::window()?.local_storage().ok()??.get_item(key).ok()?
        }
        #[cfg(feature = "ssr")]
        {
            let _ = key;
            None
        }
    }

    fn set(&self, key: &str, value: &str) -> bool {
        #[cfg(not(feature = "ssr"))]
        {
            web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
                .map(|s| s.set_item(key, value).is_ok())
                .unwrap_or(false)
        }
        #[cfg(feature = "ssr")]
        {
            let _ = (key, value);
            false
        }
    }

    fn remove(&self, key: &str) {
        #[cfg(not(feature = "ssr"))]
        {
            if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
                let _ = s.remove_item(key);
            }
        }
        #[cfg(feature = "ssr")]
        {
            let _ = key;
        }
    }
}

pub fn now_ms() -> f64 {
    #[cfg(not(feature = "ssr"))]
    {
        js_sys::Date::now()
    }
    #[cfg(feature = "ssr")]
    {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MemoryStorage(RefCell<HashMap<String, String>>);

    impl Storage for MemoryStorage {
        fn get(&self, key: &str) -> Option<String> {
            self.0.borrow().get(key).cloned()
        }
        fn set(&self, key: &str, value: &str) -> bool {
            self.0.borrow_mut().insert(key.to_string(), value.to_string());
            true
        }
        fn remove(&self, key: &str) {
            self.0.borrow_mut().remove(key);
        }
    }

    struct FullStorage;

    impl Storage for FullStorage {
        fn get(&self, _key: &str) -> Option<String> {
            None
        }
        fn set(&self, _key: &str, _value: &str) -> bool {
            false
        }
        fn remove(&self, _key: &str) {}
    }

    #[test]
    fn save_then_load_round_trips_bytes_and_permission() {
        let storage = MemoryStorage::default();
        assert!(load(&storage, 7).is_none());
        assert!(save(&storage, 7, &[1, 2, 3], 2, 1000.0));
        let loaded = load(&storage, 7).unwrap();
        assert_eq!(loaded.snapshot, vec![1, 2, 3]);
        assert_eq!(loaded.permission, 2);
        assert!(remember_permission(&storage, 7, 3, 2000.0));
        assert_eq!(load(&storage, 7).unwrap().permission, 3);
    }

    #[test]
    fn the_least_recently_used_list_is_evicted_past_the_cap() {
        let storage = MemoryStorage::default();
        for id in 1..=MAX_LISTS as i32 {
            assert!(save(&storage, id, &[id as u8], 1, id as f64));
        }
        assert!(save(&storage, 5, &[5], 1, 100.0), "list 5 becomes the newest");
        assert!(save(&storage, 99, &[99], 1, 101.0), "one past the cap");
        assert!(load(&storage, 1).is_none(), "list 1 was the oldest");
        assert!(load(&storage, 5).is_some());
        assert!(load(&storage, 99).is_some());
        assert_eq!(read_index(&storage).lists.len(), MAX_LISTS);
    }

    #[test]
    fn a_full_store_reports_failure_without_panicking() {
        assert!(!save(&FullStorage, 1, &[1], 1, 0.0));
        assert!(load(&FullStorage, 1).is_none());
    }
}
```

- [ ] **Step 3: Run the tests and commit**

Run: `cargo test -p ultros-app --lib list_doc::store`
Expected: 3 passed.

```bash
git add ultros-frontend/ultros-app/src/list_doc ultros-frontend/ultros-app/src/lib.rs
git commit -m "feat(app): list document persistence with an LRU cap

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Adapter to today's shapes and the `Edit` enum

**Files:**
- Create: `ultros-frontend/ultros-app/src/list_doc/adapter.rs`
- Modify: `ultros-frontend/ultros-app/src/list_doc/mod.rs`

**Interfaces:**
- Consumes: `ListDocument`, `ListUndo`, `RowKey`, `Quality`, `RowSnapshot`, `DocError`; `ultros_api_types::list::{ListItem, ListWithPermission}`, `ActiveListing`, `AnySelector`.
- Produces: `row_id(&RowKey) -> i32`, `to_list_item(list_id, &RowSnapshot) -> ListItem`, `find_key(&ListDocument, id) -> Option<RowKey>`, `view_result(&ListWithPermission, &ListDocument, &HashMap<i32, Vec<ActiveListing>>) -> (ListWithPermission, Vec<(ListItem, Vec<ActiveListing>)>)`, `Edit::{Add(ListItem), Edit(ListItem), Remove(i32), RemoveMany(Vec<i32>), SetQuality(Vec<i32>, Option<bool>), AddAcquired { item_id: i32, delta: i64 }, Rename { name: String, scope: AnySelector }}`, `apply(&ListDocument, &mut ListUndo, Edit) -> Result<(), DocError>`.

- [ ] **Step 1: Write the module with tests**

```rust
//! Today's row shapes from the document, so every existing list component
//! renders unchanged (spec section 3.2), and the edits those components
//! dispatch, each applied as one undo step.

use std::collections::HashMap;

use ultros_api_types::ActiveListing;
use ultros_api_types::list::{ListItem, ListWithPermission};
use ultros_api_types::world_helper::AnySelector;
use ultros_list_doc::{DocError, ListDocument, ListUndo, Quality, RowKey, RowSnapshot};

/// A stable 32-bit id for `<For>` keys and row callbacks: FNV-1a over the
/// key text, masked positive. It exists only on the Labs page and is never
/// sent to a REST endpoint.
pub fn row_id(key: &RowKey) -> i32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in key.to_string().bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    (hash & 0x7fff_ffff) as i32
}

fn clamp_i32(value: i64) -> i32 {
    value.clamp(0, i32::MAX as i64) as i32
}

pub fn to_list_item(list_id: i32, row: &RowSnapshot) -> ListItem {
    ListItem {
        id: row_id(&row.key),
        item_id: row.key.item_id,
        list_id,
        hq: row.key.hq(),
        quantity: Some(clamp_i32(row.need)),
        acquired: Some(clamp_i32(row.acquired)),
        target_price: row.target,
    }
}

pub fn find_key(doc: &ListDocument, id: i32) -> Option<RowKey> {
    doc.rows().into_iter().map(|r| r.key).find(|k| row_id(k) == id)
}

/// The `(list, rows with listings)` the page has always rendered. `base`
/// supplies permission, owner and ids; the document supplies name, scope
/// and rows; `listings` is keyed by item id.
pub fn view_result(
    base: &ListWithPermission,
    doc: &ListDocument,
    listings: &HashMap<i32, Vec<ActiveListing>>,
) -> (ListWithPermission, Vec<(ListItem, Vec<ActiveListing>)>) {
    let meta = doc.meta();
    let mut list = base.clone();
    if !meta.name.is_empty() {
        list.list.name = meta.name;
    }
    if let Some(scope) = meta.scope {
        list.list.wdr_filter = scope;
    }
    let list_id = list.list.id;
    let rows = doc
        .rows()
        .iter()
        .map(|row| {
            (
                to_list_item(list_id, row),
                listings.get(&row.key.item_id).cloned().unwrap_or_default(),
            )
        })
        .collect();
    (list, rows)
}

/// One user action. `Remove`, `Edit` and `SetQuality` ignore ids that are
/// already gone, so a double click or a stale row is harmless.
#[derive(Clone, Debug)]
pub enum Edit {
    Add(ListItem),
    Edit(ListItem),
    Remove(i32),
    RemoveMany(Vec<i32>),
    SetQuality(Vec<i32>, Option<bool>),
    AddAcquired { item_id: i32, delta: i64 },
    Rename { name: String, scope: AnySelector },
}

pub fn apply(doc: &ListDocument, undo: &mut ListUndo, edit: Edit) -> Result<(), DocError> {
    match edit {
        Edit::Add(item) => {
            let key = RowKey::new(item.item_id, item.hq);
            let acquired = item.acquired.unwrap_or(0) as i64;
            undo.group(|| {
                doc.add_row(key, item.quantity.unwrap_or(1) as i64, item.target_price)?;
                if acquired != 0 {
                    doc.add_acquired(&key, acquired)?;
                }
                Ok(())
            })
        }
        Edit::Edit(item) => {
            let Some(key) = find_key(doc, item.id) else {
                return Ok(());
            };
            let quality = Quality::from(item.hq);
            undo.group(|| {
                let key = if quality != key.quality {
                    doc.set_quality(&key, quality)?
                } else {
                    key
                };
                doc.set_need(&key, item.quantity.unwrap_or(1) as i64)?;
                doc.set_acquired(&key, item.acquired.unwrap_or(0) as i64)?;
                doc.set_target(&key, item.target_price)
            })
        }
        Edit::Remove(id) => match find_key(doc, id) {
            Some(key) => doc.remove_row(&key),
            None => Ok(()),
        },
        Edit::RemoveMany(ids) => undo.group(|| {
            for id in ids {
                if let Some(key) = find_key(doc, id) {
                    doc.remove_row(&key)?;
                }
            }
            Ok(())
        }),
        Edit::SetQuality(ids, hq) => {
            let quality = Quality::from(hq);
            undo.group(|| {
                for id in ids {
                    if let Some(key) = find_key(doc, id)
                        && key.quality != quality
                    {
                        doc.set_quality(&key, quality)?;
                    }
                }
                Ok(())
            })
        }
        Edit::AddAcquired { item_id, delta } => {
            // The first row of that item with room, as auto-mark always chose.
            let Some(row) = doc
                .rows()
                .into_iter()
                .find(|r| r.key.item_id == item_id && r.acquired < r.need)
            else {
                return Ok(());
            };
            doc.add_acquired(&row.key, delta)
        }
        Edit::Rename { name, scope } => undo.group(|| {
            doc.rename(&name)?;
            doc.set_scope(scope)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_api_types::list::{List, ListPermission};
    use ultros_list_doc::MetaSnapshot;

    fn doc() -> ListDocument {
        ListDocument::from_rows(
            MetaSnapshot {
                name: "Doc name".into(),
                scope: Some(AnySelector::World(79)),
            },
            &[RowSnapshot {
                key: RowKey::new(10, None),
                need: 2,
                acquired: 0,
                target: None,
            }],
        )
    }

    fn base() -> ListWithPermission {
        ListWithPermission {
            list: List {
                id: 5,
                owner: 1,
                name: "Rest name".into(),
                wdr_filter: AnySelector::Datacenter(1),
            },
            permission: ListPermission::Owner,
            owner_name: Some("Aaron".into()),
        }
    }

    #[test]
    fn row_ids_are_positive_stable_and_distinct_per_quality() {
        let any = row_id(&RowKey::new(10, None));
        let hq = row_id(&RowKey::new(10, Some(true)));
        assert!(any > 0 && hq > 0);
        assert_ne!(any, hq);
        assert_eq!(any, row_id(&RowKey::new(10, None)));
    }

    #[test]
    fn view_result_takes_name_scope_and_rows_from_the_document() {
        let doc = doc();
        let mut listings = HashMap::new();
        listings.insert(10, vec![]);
        let (list, rows) = view_result(&base(), &doc, &listings);
        assert_eq!(list.list.name, "Doc name");
        assert_eq!(list.list.wdr_filter, AnySelector::World(79));
        assert_eq!(list.permission, ListPermission::Owner);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, to_list_item(5, &doc.rows()[0]));
        assert_eq!(rows[0].0.list_id, 5);
    }

    #[test]
    fn edits_apply_and_each_is_one_undo_step() {
        let doc = doc();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        let existing = to_list_item(5, &doc.rows()[0]);

        apply(&doc, &mut undo, Edit::Add(ListItem { item_id: 10, list_id: 5, quantity: Some(3), ..Default::default() })).unwrap();
        assert_eq!(doc.row(&RowKey::new(10, None)).unwrap().need, 5, "add merges into the same key");

        let mut edited = existing.clone();
        edited.hq = Some(true);
        edited.quantity = Some(4);
        edited.acquired = Some(1);
        apply(&doc, &mut undo, Edit::Edit(edited)).unwrap();
        let moved = doc.row(&RowKey::new(10, Some(true))).unwrap();
        assert_eq!((moved.need, moved.acquired), (4, 1));
        assert!(doc.row(&RowKey::new(10, None)).is_none());
        assert!(undo.undo().unwrap(), "the move plus the edits undo together");
        assert_eq!(doc.row(&RowKey::new(10, None)).unwrap().need, 5);

        apply(&doc, &mut undo, Edit::Remove(999)).unwrap();
        apply(&doc, &mut undo, Edit::AddAcquired { item_id: 10, delta: 1 }).unwrap();
        assert_eq!(doc.row(&RowKey::new(10, None)).unwrap().acquired, 1);
        apply(&doc, &mut undo, Edit::Rename { name: "New".into(), scope: AnySelector::Region(1) }).unwrap();
        assert_eq!(doc.meta().name, "New");
        assert!(undo.undo().unwrap());
        assert_eq!(doc.meta().name, "Doc name");

        let id = row_id(&RowKey::new(10, None));
        apply(&doc, &mut undo, Edit::SetQuality(vec![id], Some(false))).unwrap();
        assert!(doc.row(&RowKey::new(10, Some(false))).is_some());
        apply(&doc, &mut undo, Edit::RemoveMany(vec![row_id(&RowKey::new(10, Some(false)))])).unwrap();
        assert!(doc.rows().is_empty());
    }
}
```

Add `pub mod adapter;` to `list_doc/mod.rs`.

- [ ] **Step 2: Run the tests and commit**

Run: `cargo test -p ultros-app --lib list_doc::adapter`
Expected: 3 passed.

```bash
git add ultros-frontend/ultros-app/src/list_doc
git commit -m "feat(app): document adapter to today's list shapes and the Edit enum

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: `ListDocHandle`

**Files:**
- Create: `ultros-frontend/ultros-app/src/list_doc/handle.rs`
- Modify: `ultros-frontend/ultros-app/src/list_doc/mod.rs`

**Interfaces:**
- Consumes: `store`, `adapter::{apply, Edit}`, `ListDocument`, `ListUndo`, `Subscription`; `StoredValue::new_local`, `gloo_timers::callback::Timeout`, `leptos_use::{use_event_listener, use_document}`.
- Produces: `ListDocHandle` (`Clone + Copy`) with `open(list_id) -> Self`, fields `list_id: i32`, `revision: RwSignal<u64>`, `outbox: RwSignal<Vec<Vec<u8>>>`, `status: RwSignal<String>`, `permission: RwSignal<i16>`; methods `with_doc`, `rows`, `meta`, `version`, `apply(Edit) -> Result<(), DocError>`, `import(&[u8]) -> Result<(), DocError>`, `export_since(&[u8]) -> Result<Vec<u8>, DocError>`, `is_ahead_of(&[u8]) -> bool`, `undo() -> bool`, `redo() -> bool`, `set_status(&str)`, `remember_permission(i16)`, `save_now()`.

- [ ] **Step 1: Write the module**

```rust
//! The page's handle on its document (spec section 3.1): the document, its
//! undo manager, a revision signal that memos derive from, an outbox of
//! local commits for the socket, and persistence on a debounce.

use leptos::prelude::*;
use ultros_list_doc::{DocError, ListDocument, ListUndo, MetaSnapshot, RowSnapshot, Subscription};

use crate::list_doc::adapter::{self, Edit};
use crate::list_doc::store::{self, BrowserStorage};

const SAVE_DEBOUNCE_MS: u32 = 500;

#[derive(Clone, Copy)]
pub struct ListDocHandle {
    pub list_id: i32,
    doc: StoredValue<ListDocument, LocalStorage>,
    undo: StoredValue<ListUndo, LocalStorage>,
    subscriptions: StoredValue<Vec<Subscription>, LocalStorage>,
    save_timer: StoredValue<Option<gloo_timers::callback::Timeout>, LocalStorage>,
    /// Bumped on every change, local or remote.
    pub revision: RwSignal<u64>,
    /// Local commits waiting for the socket, in order.
    pub outbox: RwSignal<Vec<Vec<u8>>>,
    /// `RealtimeStatus` vocabulary: connecting, live, reconnecting, offline.
    pub status: RwSignal<String>,
    /// Last known `ListPermission` as `i16`, cached beside the snapshot.
    pub permission: RwSignal<i16>,
}

impl ListDocHandle {
    /// Load the browser's snapshot for this list, or start empty. Loro's
    /// default random peer id is used; nothing stores or reuses peer ids.
    pub fn open(list_id: i32) -> Self {
        let loaded = store::load(&BrowserStorage, list_id);
        let doc = loaded
            .as_ref()
            .and_then(|l| ListDocument::from_snapshot(&l.snapshot).ok())
            .unwrap_or_default();
        let permission = loaded.map(|l| l.permission).unwrap_or(0);
        let undo = ListUndo::new(&doc);
        let revision = RwSignal::new(0u64);
        let outbox = RwSignal::new(Vec::new());
        let on_change = doc.on_change(move || revision.update(|r| *r += 1));
        let on_local = doc.on_local_update(move |bytes| {
            let bytes = bytes.to_vec();
            outbox.update(|queue| queue.push(bytes));
        });
        let handle = Self {
            list_id,
            doc: StoredValue::new_local(doc),
            undo: StoredValue::new_local(undo),
            subscriptions: StoredValue::new_local(vec![on_change, on_local]),
            save_timer: StoredValue::new_local(None),
            revision,
            outbox,
            status: RwSignal::new("connecting".to_string()),
            permission: RwSignal::new(permission),
        };
        handle.install_persistence();
        handle
    }

    pub fn with_doc<R>(&self, f: impl FnOnce(&ListDocument) -> R) -> R {
        self.doc.with_value(f)
    }

    pub fn rows(&self) -> Vec<RowSnapshot> {
        self.with_doc(|doc| doc.rows())
    }

    pub fn meta(&self) -> MetaSnapshot {
        self.with_doc(|doc| doc.meta())
    }

    pub fn version(&self) -> Vec<u8> {
        self.with_doc(|doc| doc.version())
    }

    /// One user action, one undo step. The document commits inside, which
    /// bumps `revision` and pushes the update onto `outbox`.
    pub fn apply(&self, edit: Edit) -> Result<(), DocError> {
        self.doc.with_value(|doc| {
            let mut result = Ok(());
            self.undo
                .update_value(|undo| result = adapter::apply(doc, undo, edit));
            result
        })
    }

    pub fn import(&self, bytes: &[u8]) -> Result<(), DocError> {
        self.with_doc(|doc| doc.import(bytes).map(|_| ()))
    }

    pub fn export_since(&self, version: &[u8]) -> Result<Vec<u8>, DocError> {
        self.with_doc(|doc| doc.export_since(version))
    }

    pub fn is_ahead_of(&self, version: &[u8]) -> bool {
        self.with_doc(|doc| doc.is_ahead_of(version))
    }

    pub fn undo(&self) -> bool {
        let mut done = false;
        self.undo
            .update_value(|undo| done = undo.undo().unwrap_or(false));
        done
    }

    pub fn redo(&self) -> bool {
        let mut done = false;
        self.undo
            .update_value(|undo| done = undo.redo().unwrap_or(false));
        done
    }

    pub fn set_status(&self, status: &str) {
        self.status.set(status.to_string());
    }

    pub fn remember_permission(&self, permission: i16) {
        self.permission.set(permission);
        let _ = store::remember_permission(
            &BrowserStorage,
            self.list_id,
            permission,
            store::now_ms(),
        );
    }

    pub fn save_now(&self) {
        let snapshot = self.with_doc(|doc| doc.export_snapshot());
        if let Ok(snapshot) = snapshot {
            let _ = store::save(
                &BrowserStorage,
                self.list_id,
                &snapshot,
                self.permission.get_untracked(),
                store::now_ms(),
            );
        }
    }

    /// Save half a second after the last change, and immediately when the
    /// tab is hidden. Client only: the server has no storage to write.
    fn install_persistence(&self) {
        #[cfg(not(feature = "ssr"))]
        {
            let handle = *self;
            Effect::new(move |_| {
                let _ = handle.revision.get();
                // Dropping the previous timeout cancels it.
                handle.save_timer.update_value(|timer| *timer = None);
                let timeout = gloo_timers::callback::Timeout::new(SAVE_DEBOUNCE_MS, move || {
                    handle.save_now();
                });
                handle.save_timer.set_value(Some(timeout));
            });
            let handle = *self;
            let _ = leptos_use::use_event_listener(
                leptos_use::use_document(),
                leptos::ev::visibilitychange,
                move |_| {
                    let hidden = web_sys::window()
                        .and_then(|w| w.document())
                        .map(|d| d.hidden())
                        .unwrap_or(false);
                    if hidden {
                        handle.save_now();
                    }
                },
            );
        }
    }
}
```

Add `pub mod handle;` to `list_doc/mod.rs`. The unused-field warning on `subscriptions` is intended: it keeps the subscriptions alive; add `#[allow(dead_code)]` on that field with the comment `// Held so the document keeps notifying; dropping unsubscribes.`

- [ ] **Step 2: Build both halves and commit**

Run: `cargo check -p ultros-app` and `cargo check -p ultros-app --features hydrate --no-default-features`
Expected: both succeed.

```bash
git add ultros-frontend/ultros-app/src/list_doc
git commit -m "feat(app): ListDocHandle with revision, outbox and persistence

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Realtime client methods and the sync loop

**Files:**
- Modify: `ultros-frontend/ultros-app/src/ws/realtime.rs` (both `mod client` blocks)
- Create: `ultros-frontend/ultros-app/src/list_doc/sync.rs`
- Modify: `ultros-frontend/ultros-app/src/list_doc/mod.rs`

**Interfaces:**
- Consumes: `ClientMessage::{SubscribeListDoc, ListDocUpdate}`, `ServerClient::{ListDocSubscribed, ListDocUpdate, Stale, Error}`, `ListDocPayload`, `ListDocHandle`.
- Produces: `RealtimeClient::subscribe_list_doc(&self, list_id: i32, version: Vec<u8>, handler: impl Fn(ServerClient) + 'static) -> RealtimeSubscription`, `RealtimeClient::send_list_doc_update(&self, list_id: i32, update: Vec<u8>)`, `list_doc::sync::start(handle: ListDocHandle, realtime: RealtimeClient, on_stale: impl Fn() + Clone + 'static, on_remote_change: impl Fn() + Clone + 'static) -> RealtimeSubscription`.

- [ ] **Step 1: Realtime client methods**

In the `#[cfg(not(feature = "ssr"))] mod client` block, inside `impl RealtimeClient` after `subscribe_list`:

```rust
        /// Spec section 5: subscribe to a list's document with the local
        /// version. The stored subscribe message is replayed on reconnect,
        /// which re-runs the handshake with the version from subscribe time;
        /// the server then sends a superset, and imports are idempotent.
        pub(crate) fn subscribe_list_doc(
            &self,
            list_id: i32,
            version: Vec<u8>,
            handler: impl Fn(ServerClient) + 'static,
        ) -> RealtimeSubscription {
            let subscription_id = self.next_subscription_id();
            self.inner
                .handlers
                .borrow_mut()
                .insert(subscription_id, Box::new(handler));
            self.send_subscription(
                subscription_id,
                ClientMessage::SubscribeListDoc {
                    subscription_id: Some(subscription_id),
                    list_id,
                    version,
                },
            );
            RealtimeSubscription {
                client: self.clone(),
                subscription_id,
            }
        }

        /// One local commit. Queued and replayed if the socket is closed.
        pub(crate) fn send_list_doc_update(&self, list_id: i32, update: Vec<u8>) {
            self.send_control(ClientMessage::ListDocUpdate { list_id, update });
        }
```

In the `#[cfg(feature = "ssr")] mod client` block, inside `impl RealtimeClient`:

```rust
        pub(crate) fn subscribe_list_doc(
            &self,
            _list_id: i32,
            _version: Vec<u8>,
            _handler: impl Fn(ServerClient) + 'static,
        ) -> RealtimeSubscription {
            RealtimeSubscription
        }

        pub(crate) fn send_list_doc_update(&self, _list_id: i32, _update: Vec<u8>) {}
```

- [ ] **Step 2: The sync loop**

`list_doc/sync.rs`:

```rust
//! The handshake and steady state for one document on the realtime socket
//! (spec section 5). Offline edits need no queue: every reconnect re-runs
//! the handshake, and the client sends whatever the server lacks.

use leptos::prelude::*;
use ultros_api_types::websocket::{ListDocPayload, ServerClient};

use crate::list_doc::handle::ListDocHandle;
use crate::ws::realtime::{RealtimeClient, RealtimeSubscription};

/// Subscribe with the local version, apply what the server sends, send what
/// it lacks, then relay local commits and import remote ones. Dropping the
/// returned subscription unsubscribes.
pub fn start(
    handle: ListDocHandle,
    realtime: RealtimeClient,
    on_stale: impl Fn() + Clone + 'static,
    on_remote_change: impl Fn() + Clone + 'static,
) -> RealtimeSubscription {
    handle.set_status("connecting");
    let list_id = handle.list_id;
    let sender = realtime.clone();
    let subscription = realtime.subscribe_list_doc(list_id, handle.version(), move |message| {
        match message {
            ServerClient::ListDocSubscribed {
                version, payload, ..
            } => {
                let imported = match payload {
                    ListDocPayload::Snapshot(bytes) | ListDocPayload::Updates(bytes) => {
                        handle.import(&bytes).is_ok()
                    }
                    ListDocPayload::UpToDate => true,
                };
                if imported && handle.is_ahead_of(&version) {
                    // The handshake diff covers everything the outbox holds.
                    handle.outbox.set(Vec::new());
                    if let Ok(diff) = handle.export_since(&version) {
                        sender.send_list_doc_update(list_id, diff);
                    }
                }
                handle.set_status("live");
                handle.save_now();
            }
            ServerClient::ListDocUpdate { update, .. } => {
                if handle.import(&update).is_ok() {
                    on_remote_change();
                }
                handle.set_status("live");
            }
            ServerClient::Stale { .. } => {
                handle.set_status("reconnecting");
                on_stale();
            }
            ServerClient::Error { message } => {
                log::warn!("list {list_id} sync: {message}");
            }
            _ => {}
        }
    });

    let sender = realtime;
    Effect::new(move |_| {
        let pending = handle.outbox.get();
        if pending.is_empty() {
            return;
        }
        for update in pending {
            sender.send_list_doc_update(list_id, update);
        }
        handle.outbox.set(Vec::new());
    });
    subscription
}
```

Add `pub mod sync;` to `list_doc/mod.rs`.

- [ ] **Step 3: Build both halves and commit**

Run: `cargo check -p ultros-app` and `cargo check -p ultros-app --features hydrate --no-default-features`
Expected: both succeed.

```bash
git add ultros-frontend/ultros-app/src/ws/realtime.rs ultros-frontend/ultros-app/src/list_doc
git commit -m "feat(app): list document handshake and relay on the realtime socket

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Undo keys

**Files:**
- Create: `ultros-frontend/ultros-app/src/list_doc/undo.rs`
- Modify: `ultros-frontend/ultros-app/src/list_doc/mod.rs`

**Interfaces:**
- Consumes: `PlatformHotkeys` (`global_state::platform::use_platform_hotkeys().apple`), `ListDocHandle::{undo, redo}`.
- Produces: `UndoKey::{Undo, Redo}`, `KeyContext { ctrl, meta, shift, apple, editable_target, modal_open }`, `classify_key(&str, KeyContext) -> Option<UndoKey>`, `install(handle: ListDocHandle, modal_open: Signal<bool>)`.

- [ ] **Step 1: Write the module with tests**

```rust
//! Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y, Cmd on Apple (spec section 3.3). One
//! window listener per open page, ignored while an editable element has
//! focus or one of the page's modals is open.

use leptos::prelude::*;

use crate::list_doc::handle::ListDocHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoKey {
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct KeyContext {
    pub ctrl: bool,
    pub meta: bool,
    pub shift: bool,
    pub apple: bool,
    pub editable_target: bool,
    pub modal_open: bool,
}

pub fn classify_key(key: &str, ctx: KeyContext) -> Option<UndoKey> {
    if ctx.editable_target || ctx.modal_open {
        return None;
    }
    let modifier = if ctx.apple { ctx.meta } else { ctx.ctrl };
    if !modifier {
        return None;
    }
    match key.to_ascii_lowercase().as_str() {
        "z" if ctx.shift => Some(UndoKey::Redo),
        "z" => Some(UndoKey::Undo),
        "y" if !ctx.apple => Some(UndoKey::Redo),
        _ => None,
    }
}

/// Register the window listener. Hydrate only: the server never sees keys.
pub fn install(handle: ListDocHandle, modal_open: Signal<bool>) {
    #[cfg(feature = "hydrate")]
    {
        use leptos_use::{UseEventListenerOptions, use_event_listener_with_options, use_window};
        use wasm_bindgen::JsCast;

        let apple = crate::global_state::platform::use_platform_hotkeys().apple;
        let _ = use_event_listener_with_options(
            use_window(),
            leptos::ev::keydown,
            move |ev: web_sys::KeyboardEvent| {
                let editable_target = ev
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .map(|el| {
                        let tag = el.tag_name().to_ascii_uppercase();
                        tag == "INPUT"
                            || tag == "TEXTAREA"
                            || tag == "SELECT"
                            || el
                                .dyn_ref::<web_sys::HtmlElement>()
                                .is_some_and(|h| h.is_content_editable())
                    })
                    .unwrap_or(false);
                let ctx = KeyContext {
                    ctrl: ev.ctrl_key(),
                    meta: ev.meta_key(),
                    shift: ev.shift_key(),
                    apple: apple.get_untracked(),
                    editable_target,
                    modal_open: modal_open.get_untracked(),
                };
                match classify_key(&ev.key(), ctx) {
                    Some(UndoKey::Undo) => {
                        ev.prevent_default();
                        handle.undo();
                    }
                    Some(UndoKey::Redo) => {
                        ev.prevent_default();
                        handle.redo();
                    }
                    None => {}
                }
            },
            UseEventListenerOptions::default()
                .capture(false)
                .passive(false),
        );
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = (handle, modal_open);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl() -> KeyContext {
        KeyContext {
            ctrl: true,
            ..Default::default()
        }
    }

    #[test]
    fn windows_bindings() {
        assert_eq!(classify_key("z", ctrl()), Some(UndoKey::Undo));
        assert_eq!(classify_key("Z", KeyContext { shift: true, ..ctrl() }), Some(UndoKey::Redo));
        assert_eq!(classify_key("y", ctrl()), Some(UndoKey::Redo));
        assert_eq!(classify_key("z", KeyContext::default()), None);
        assert_eq!(classify_key("z", KeyContext { meta: true, ..Default::default() }), None);
        assert_eq!(classify_key("a", ctrl()), None);
    }

    #[test]
    fn apple_uses_cmd_and_has_no_cmd_y() {
        let cmd = KeyContext {
            meta: true,
            apple: true,
            ..Default::default()
        };
        assert_eq!(classify_key("z", cmd), Some(UndoKey::Undo));
        assert_eq!(classify_key("z", KeyContext { shift: true, ..cmd }), Some(UndoKey::Redo));
        assert_eq!(classify_key("y", cmd), None);
        assert_eq!(classify_key("z", KeyContext { ctrl: true, apple: true, ..Default::default() }), None);
    }

    #[test]
    fn editable_targets_and_modals_swallow_the_keys() {
        assert_eq!(classify_key("z", KeyContext { editable_target: true, ..ctrl() }), None);
        assert_eq!(classify_key("z", KeyContext { modal_open: true, ..ctrl() }), None);
    }
}
```

Add `pub mod undo;` to `list_doc/mod.rs`. If `web_sys::HtmlElement::is_content_editable` is not in the enabled web-sys features, add `"HtmlElement"` (already present) and nothing else is needed; `KeyboardEvent` comes with `leptos::ev::keydown`.

- [ ] **Step 2: Run the tests and commit**

Run: `cargo test -p ultros-app --lib list_doc::undo` and `cargo check -p ultros-app --features hydrate --no-default-features`
Expected: 3 passed; builds.

```bash
git add ultros-frontend/ultros-app/src/list_doc
git commit -m "feat(app): undo and redo keys for the list document

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: `AutoMarkPurchases` learns a callback

**Files:**
- Modify: `ultros-frontend/ultros-app/src/components/list/auto_mark_purchases.rs` (component signature at line 16, the sale handler at lines 35-46)

**Interfaces:**
- Produces: `#[prop(optional, into)] on_purchase: Option<Callback<i32>>` on `AutoMarkPurchases`. When set, a matching sale calls it with the item id instead of mutating the resource and posting to REST.

- [ ] **Step 1: Add the prop**

Change the signature to:

```rust
#[component]
pub fn AutoMarkPurchases(
    list_view: Resource<ListViewResult>,
    /// The Labs page routes purchases into its document instead of the
    /// resource-plus-REST path; without it, behaviour is unchanged.
    #[prop(optional, into)]
    on_purchase: Option<Callback<i32>>,
) -> impl IntoView {
```

and the handler body to:

```rust
            move |message| {
                let ServerClient::Sales(EventType::Added(event)) = message else {
                    return;
                };
                for (sale, _) in event.sales {
                    match on_purchase {
                        Some(callback) => callback.run(sale.sold_item_id),
                        None => mark_item_purchased(list_view, sale.sold_item_id),
                    }
                }
            },
```

- [ ] **Step 2: Run the component's tests and commit**

Run: `cargo test -p ultros-app --lib auto_mark`
Expected: the existing snapshot and `apply_purchase` tests pass unchanged.

```bash
git add ultros-frontend/ultros-app/src/components/list/auto_mark_purchases.rs
git commit -m "feat(lists): AutoMarkPurchases accepts an on_purchase callback

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: `ListViewSync` on the document

**Files:**
- Replace: `ultros-frontend/ultros-app/src/routes/list_view_sync.rs`
- Reference: `ultros-frontend/ultros-app/src/routes/list_view.rs` (the source of the copy)

**Interfaces:**
- Consumes: everything from Tasks 3-7; `get_list_items_with_listings`, `get_list_activity` from `api.rs`.
- Produces: `ListViewSync`, `ListRoute` (remounts on list id change).

- [ ] **Step 1: Copy the page**

Copy `routes/list_view.rs` over `routes/list_view_sync.rs` in full (`cp`), then in the new file: rename `pub fn ListView()` to `pub fn ListViewSync()`, and delete the private helpers that exist in `list_view.rs` and would now be duplicated. Instead import them: replace the definitions of `MenuState`, `filter_excluded`, `IdList`, `NameList`, `sort_list_items`, `remaining_quantity`, `cheapest_price_per_unit`, `list_item_table_columns`, `list_item_table_skeleton_columns`, `ActivityFeed` and the `#[cfg(test)] mod tests` block with:

```rust
use crate::routes::list_view::{
    ActivityFeed, IdList, ListViewResult, MenuState, NameList, cheapest_price_per_unit,
    filter_excluded, list_item_table_columns, list_item_table_skeleton_columns,
    remaining_quantity, sort_list_items,
};
```

and make those items `pub(crate)` in `list_view.rs` (visibility only; no behaviour change). Add to `list_view.rs`:

```rust
pub(crate) type ListViewResult =
    Result<(ListWithPermission, Vec<(ListItem, Vec<ActiveListing>)>), crate::error::AppError>;
```

(`auto_mark_purchases.rs` defines the same alias privately; leave it.) Where `list_view.rs` uses a name that is now `pub(crate)`, nothing else changes.

- [ ] **Step 2: Replace the data-source block**

In `list_view_sync.rs`, replace everything from `let add_item = Action::new(` through the `on_cleanup(move || { ... });` that clears the two subscriptions (the block that is lines 322-460 in `list_view.rs`) with:

```rust
    // ---- Local-first document (spec sections 3.1, 3.2) ----
    let handle = ListDocHandle::open(list_id.get_untracked());
    provide_context(handle);
    let realtime_status: Signal<String> = handle.status.into();

    let add_item = Action::new(move |item: &ListItem| {
        let edit = Edit::Add(item.clone());
        async move { handle.apply(edit).map_err(AppError::from) }
    });
    let delete_item = Action::new(move |id: &i32| {
        let edit = Edit::Remove(*id);
        async move { handle.apply(edit).map_err(AppError::from) }
    });
    let edit_item = Action::new(move |item: &ListItem| {
        let edit = Edit::Edit(item.clone());
        async move { handle.apply(edit).map_err(AppError::from) }
    });
    let delete_items = Action::new(move |ids: &Vec<i32>| {
        let edit = Edit::RemoveMany(ids.clone());
        async move { handle.apply(edit).map_err(AppError::from) }
    });
    let edit_items_hq = Action::new(move |(ids, hq): &(Vec<i32>, Option<bool>)| {
        let edit = Edit::SetQuality(ids.clone(), *hq);
        async move { handle.apply(edit).map_err(AppError::from) }
    });
    let edit_list_action = Action::new(move |list: &ultros_api_types::list::List| {
        let edit = Edit::Rename {
            name: list.name.clone(),
            scope: list.wdr_filter,
        };
        async move { handle.apply(edit).map_err(AppError::from) }
    });

    let bulk_pending =
        Signal::derive(move || delete_items.pending().get() || edit_items_hq.pending().get());
    let bulk_error = Signal::derive(move || {
        delete_items
            .value()
            .get()
            .and_then(|result| result.err().map(|e| e.to_string()))
            .or_else(|| {
                edit_items_hq
                    .value()
                    .get()
                    .and_then(|result| result.err().map(|e| e.to_string()))
            })
    });

    // Listings come from the existing endpoint and are cached per list; the
    // document supplies rows, so a local edit never refetches prices.
    let (external_update_version, set_external_update_version) = signal(0);
    let (activity_update_version, set_activity_update_version) = signal(0);
    let (listings_version, set_listings_version) = signal(0u32);
    let (last_update_at, set_last_update_at) =
        signal::<Option<chrono::DateTime<chrono::Utc>>>(None);
    let listings_cache: StoredValue<Option<ListingsCache>> = StoredValue::new(None);

    let list_view = Resource::new(
        move || {
            (
                list_id(),
                handle.revision.get(),
                listings_version.get(),
                external_update_version.get(),
            )
        },
        move |(id, _, listings_v, _)| load_view(id, handle, listings_cache, listings_v),
    );
    let user_resource = Resource::new(|| {}, |_| async move { crate::api::get_login().await.ok() });
    let self_user_id = Signal::derive(move || user_resource.get().flatten().map(|u| u.id));

    let activity_view = Resource::new(
        move || (list_id(), activity_update_version.get()),
        move |(id, _)| get_list_activity(id),
    );

    let realtime = use_realtime();
    let doc_subscription = StoredValue::new(None::<RealtimeSubscription>);
    let activity_subscription = StoredValue::new(None::<RealtimeSubscription>);
    let list_market_subscription = StoredValue::new(None::<RealtimeSubscription>);
    let (resync, set_resync) = signal(0u32);

    let realtime_for_doc = realtime.clone();
    Effect::new(move |_| {
        resync.track();
        doc_subscription.update_value(|sub| *sub = None);
        let Some(realtime) = realtime_for_doc.clone() else {
            handle.set_status("offline");
            return;
        };
        let sub = crate::list_doc::sync::start(
            handle,
            realtime,
            move || set_resync.update(|n| *n += 1),
            move || set_last_update_at.set(Some(chrono::Utc::now())),
        );
        doc_subscription.set_value(Some(sub));
    });

    // The legacy list subscription only drives the activity feed now.
    let realtime_for_activity = realtime.clone();
    Effect::new(move |_| {
        activity_subscription.update_value(|sub| *sub = None);
        let id = list_id.get();
        let Some(realtime) = realtime_for_activity.clone() else {
            return;
        };
        if id != 0 {
            let sub = realtime.subscribe_list(id, move |message| {
                if let ServerClient::ListUpdate(WEvent::Added(ListEventData::Activity(_))) = message {
                    set_activity_update_version.update(|v| *v += 1);
                }
            });
            activity_subscription.set_value(Some(sub));
        }
    });

    let realtime_for_market = realtime.clone();
    Effect::new(move |_| {
        list_market_subscription.update_value(|sub| *sub = None);
        let Some(Ok((list, items))) = list_view.get() else {
            return;
        };
        let item_ids = items
            .iter()
            .map(|(item, _)| item.item_id)
            .collect::<Vec<_>>();
        if item_ids.is_empty() {
            return;
        }
        let Some(realtime) = realtime_for_market.clone() else {
            return;
        };
        let filter = FilterPredicate::World(list.list.wdr_filter)
            .and(FilterPredicate::Items(item_ids.clone()));
        let sub = realtime.subscribe_market(filter, SocketMessageType::Listings, move |message| {
            if is_list_market_update_relevant(&message, &item_ids) {
                set_last_update_at.set(Some(chrono::Utc::now()));
                set_listings_version.update(|v| *v += 1);
            }
        });
        list_market_subscription.set_value(Some(sub));
    });
    on_cleanup(move || {
        doc_subscription.update_value(|sub| *sub = None);
        activity_subscription.update_value(|sub| *sub = None);
        list_market_subscription.update_value(|sub| *sub = None);
        handle.save_now();
    });
```

Then, right after the block of modal signals (`let (confirm_bulk_delete, set_confirm_bulk_delete) = signal(false);`), add:

```rust
    crate::list_doc::undo::install(
        handle,
        Signal::derive(move || {
            item_modal_open() || recipe_modal_open() || subscribe_open() || settings_open()
                || confirm_bulk_delete()
        }),
    );
```

Add these definitions above `pub fn ListViewSync`:

```rust
/// Prices for the rows, fetched once per list and again only when the market
/// subscription or an import says so.
#[derive(Clone)]
struct ListingsCache {
    list_id: i32,
    version: u32,
    list: ListWithPermission,
    listings: HashMap<i32, Vec<ActiveListing>>,
}

async fn load_view(
    id: i32,
    handle: ListDocHandle,
    cache: StoredValue<Option<ListingsCache>>,
    listings_version: u32,
) -> ListViewResult {
    #[cfg(feature = "ssr")]
    {
        let _ = (handle, cache, listings_version);
        get_list_items_with_listings(id).await
    }
    #[cfg(not(feature = "ssr"))]
    {
        let cached = cache
            .get_value()
            .filter(|c| c.list_id == id && c.version == listings_version);
        let base = match cached {
            Some(cached) => cached,
            None => match get_list_items_with_listings(id).await {
                Ok((list, items)) => {
                    handle.remember_permission(list.permission as i16);
                    let fresh = ListingsCache {
                        list_id: id,
                        version: listings_version,
                        list,
                        listings: items.into_iter().map(|(item, l)| (item.item_id, l)).collect(),
                    };
                    cache.set_value(Some(fresh.clone()));
                    fresh
                }
                Err(error) => match cache.get_value().filter(|c| c.list_id == id) {
                    // Stale prices beat no page.
                    Some(stale) => stale,
                    None => match offline_list(id, handle) {
                        Some(list) => ListingsCache {
                            list_id: id,
                            version: listings_version,
                            list,
                            listings: HashMap::new(),
                        },
                        None => return Err(error),
                    },
                },
            },
        };
        let doc = handle.with_doc(|d| d.clone());
        Ok(crate::list_doc::adapter::view_result(&base.list, &doc, &base.listings))
    }
}

/// The list as far as the browser knows it with no server at all: the cached
/// permission and the document's own name and scope.
fn offline_list(id: i32, handle: ListDocHandle) -> Option<ListWithPermission> {
    let meta = handle.meta();
    let permission = ListPermission::from(handle.permission.get_untracked());
    if permission == ListPermission::None {
        return None;
    }
    Some(ListWithPermission {
        list: ultros_api_types::list::List {
            id,
            owner: 0,
            name: meta.name,
            wdr_filter: meta.scope?,
        },
        permission,
        owner_name: None,
    })
}
```

`ListPermission` is `#[repr(i16)]`; `list.permission as i16` gives its stored value and `ListPermission::from(i16)` reads it back. `ListDocument` is `Clone` (shared document), so `handle.with_doc(|d| d.clone())` hands `view_result` a reference without holding the stored value across the call.

- [ ] **Step 3: Adjust the remaining call sites in the copy**

- `<AutoMarkPurchases list_view=list_view />` becomes `<AutoMarkPurchases list_view=list_view on_purchase=Callback::new(move |item_id: i32| { let _ = handle.apply(Edit::AddAcquired { item_id, delta: 1 }); }) />`.
- `refresh=move || { list_view.refetch() }` on `MakePlaceImporter` becomes `refresh=move || set_listings_version.update(|v| *v += 1)`.
- The recipe modal's `on_success` that bumps `external_update_version` and `activity_update_version` stays as is: an import arrives through the socket and `external_update_version` triggers the listings fetch for the new items.
- Delete the `set_external_update_version` usages that no longer exist; keep the signal if the recipe modal uses it.
- Imports at the top of the file: replace the `crate::api::{add_item_to_list, ...}` import with `use crate::api::{get_list_activity, get_list_items_with_listings};`, add `use crate::error::AppError;`, `use crate::list_doc::adapter::Edit;`, `use crate::list_doc::handle::ListDocHandle;`, `use std::collections::HashMap;`, `use ultros_api_types::list::{ListPermission, ListWithPermission};`, `use ultros_api_types::websocket::{EventType as WEvent, ListEventData};`.

- [ ] **Step 4: `ListRoute` remounts on id change**

Replace `ListRoute` with:

```rust
/// Picks the page for `/list/:id`. The `LABS` cookie is server-visible, so
/// the server and the hydrating client make the same choice. The id is
/// tracked so moving between lists builds a fresh page and document.
#[component]
pub fn ListRoute() -> impl IntoView {
    let sync = use_lab(LAB_LISTS_SYNC);
    let params = use_params_map();
    let id = Memo::new(move |_| params.with(|p| p.get("id").unwrap_or_default()));
    move || {
        id.track();
        if sync.get() {
            view! { <ListViewSync /> }.into_any()
        } else {
            view! { <ListView /> }.into_any()
        }
    }
}
```

- [ ] **Step 5: Confirm the copy differs only where intended**

Run: `git diff --no-index ultros-frontend/ultros-app/src/routes/list_view.rs ultros-frontend/ultros-app/src/routes/list_view_sync.rs | grep '^[-+]' | grep -v '^[-+][-+]' | wc -l`
Expected: the diff is confined to the imports, the component name, the data-source block, the two call sites in Step 3, the helpers moved to imports, the removed tests block, and `ListRoute`. Anything else is a copy mistake.

- [ ] **Step 6: Build both halves, run unit tests, commit**

Run: `cargo check -p ultros-app`, `cargo check -p ultros-app --features hydrate --no-default-features`, `cargo test -p ultros-app --lib`
Expected: all succeed.

```bash
git add ultros-frontend/ultros-app/src/routes
git commit -m "feat(lists): ListViewSync renders the local document behind Labs

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: Two-browser end-to-end suite

**Files:**
- Create: `integration/list-sync.cjs`
- Modify: `integration/package.json` (scripts), `scripts/run_e2e.sh` (the test-auth block)

**Interfaces:**
- Consumes: `/test/login`, `POST /api/v1/list/create`, `POST /api/v1/list/{id}/share/user` with `{ user_id, permission: "Write" }`, `POST /api/v1/list/{id}/add/item`, `GET /api/v1/list/{id}/listings`, the `LABS` cookie, `button[aria-label="Mark as acquired"]` and `"Mark unacquired"`.

- [ ] **Step 1: Write the suite**

```js
#!/usr/bin/env node
/* eslint-disable no-console */
/**
 * Lists local-first sync E2E (Labs `lists-sync`). Requires a server built
 * with `--features test-auth`.
 *
 *   1. Owner creates a list, adds two items, shares it with an editor.
 *   2. Both open it under Labs. The owner marks a row acquired; the editor
 *      sees it without a reload.
 *   3. The owner goes offline, un-marks it, comes back: everyone converges
 *      and the server agrees.
 *   4. Ctrl+Z on the owner reverts the last edit for everyone.
 *   5. A REST add from the owner's API client appears on both pages.
 *   6. A third session without Labs sees the same rows.
 *
 * Env: BASE_URL (default http://127.0.0.1:8080), HEADLESS ("false" to watch),
 * TIMEOUT_MS (default 30000).
 */

"use strict";

const USERS = {
  owner: { id: 990000000401, username: "ListSyncOwner" },
  editor: { id: 990000000402, username: "ListSyncEditor" },
};

async function login(page, baseUrl, user, labs) {
  const url = new URL("/test/login", baseUrl);
  url.searchParams.set("user_id", String(user.id));
  url.searchParams.set("username", user.username);
  url.searchParams.set("redirect", "/list");
  const resp = await page.goto(url.toString(), { waitUntil: "domcontentloaded" });
  if (!resp || resp.status() >= 400) {
    throw new Error(`test login failed for ${user.username}: ${resp ? resp.status() : -1}`);
  }
  if (labs) {
    await page.setCookie({ name: "LABS", value: "lists-sync", url: baseUrl, path: "/" });
  }
}

async function api(page, method, path, body) {
  return page.evaluate(
    async ({ method, path, body }) => {
      const r = await fetch(path, {
        method,
        credentials: "include",
        headers: body === undefined ? {} : { "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const text = await r.text();
      let parsed = null;
      try {
        parsed = text ? JSON.parse(text) : null;
      } catch {
        parsed = text;
      }
      return { status: r.status, body: parsed };
    },
    { method, path, body },
  );
}

function fail(failures, msg) {
  console.error(`  X ${msg}`);
  failures.push(msg);
}

function pass(msg) {
  console.log(`  + ${msg}`);
}

async function waitForHydration(page, timeout) {
  await page.waitForFunction(
    () => !!document.querySelector('[data-testid="list-settings-btn"]'),
    { timeout },
  );
}

// Counts of the two acquire toggles tell us the rows and their state.
async function rowState(page) {
  return page.evaluate(() => ({
    unacquired: document.querySelectorAll('button[aria-label="Mark as acquired"]').length,
    acquired: document.querySelectorAll('button[aria-label="Mark unacquired"]').length,
  }));
}

async function waitForState(page, predicate, timeout) {
  const started = Date.now();
  let last = null;
  while (Date.now() - started < timeout) {
    last = await rowState(page);
    if (predicate(last)) return last;
    await new Promise((r) => setTimeout(r, 250));
  }
  return last;
}

async function serverAcquired(page, listId) {
  const res = await api(page, "GET", `/api/v1/list/${listId}/listings`);
  return res.body[1].map(([item]) => item.acquired || 0);
}

async function main() {
  const puppeteer = require("puppeteer");
  const BASE_URL = process.env.BASE_URL || "http://127.0.0.1:8080";
  const TIMEOUT_MS = Number(process.env.TIMEOUT_MS || 30000);
  const headless = process.env.HEADLESS !== "false";
  const browser = await puppeteer.launch({
    headless,
    args: ["--no-sandbox", "--disable-setuid-sandbox"],
  });
  const failures = [];
  let listId = null;
  let ownerPage = null;

  try {
    const ownerContext = await browser.createBrowserContext();
    const editorContext = await browser.createBrowserContext();
    const legacyContext = await browser.createBrowserContext();
    ownerPage = await ownerContext.newPage();
    const editorPage = await editorContext.newPage();
    const legacyPage = await legacyContext.newPage();
    for (const p of [ownerPage, editorPage, legacyPage]) {
      p.setDefaultTimeout(TIMEOUT_MS);
      await p.setViewport({ width: 1280, height: 900, deviceScaleFactor: 1 });
    }

    console.log("[step] logins");
    await login(ownerPage, BASE_URL, USERS.owner, true);
    await login(editorPage, BASE_URL, USERS.editor, true);
    await login(legacyPage, BASE_URL, USERS.owner, false);

    console.log("[step] owner creates and shares a list");
    const worldData = await api(ownerPage, "GET", "/api/v1/world_data");
    const worldId = worldData.body.regions[0].datacenters[0].worlds[0].id;
    const name = `ListSync E2E ${Date.now()}`;
    const create = await api(ownerPage, "POST", "/api/v1/list/create", {
      name,
      wdr_filter: { World: worldId },
    });
    if (create.status !== 200) throw new Error(`create list: ${create.status}`);
    const lists = await api(ownerPage, "GET", "/api/v1/list");
    listId = lists.body.find((e) => e.list.name === name).list.id;
    for (const itemId of [5, 6]) {
      const add = await api(ownerPage, "POST", `/api/v1/list/${listId}/add/item`, {
        id: 0,
        item_id: itemId,
        list_id: listId,
        hq: null,
        quantity: 1,
        acquired: 0,
      });
      if (add.status !== 200) fail(failures, `add item ${itemId}: ${add.status}`);
    }
    const share = await api(ownerPage, "POST", `/api/v1/list/${listId}/share/user`, {
      user_id: USERS.editor.id,
      permission: "Write",
    });
    if (share.status !== 200) fail(failures, `share: ${share.status}`);
    pass(`list ${listId} ready with two rows`);

    console.log("[step] both open the list under Labs");
    const listUrl = new URL(`/list/${listId}`, BASE_URL).toString();
    await ownerPage.goto(listUrl, { waitUntil: "domcontentloaded" });
    await editorPage.goto(listUrl, { waitUntil: "domcontentloaded" });
    await waitForHydration(ownerPage, TIMEOUT_MS);
    await waitForHydration(editorPage, TIMEOUT_MS);
    const initial = await waitForState(ownerPage, (s) => s.unacquired === 2, TIMEOUT_MS);
    if (initial.unacquired !== 2) fail(failures, `owner expected 2 rows, saw ${JSON.stringify(initial)}`);
    const live = await ownerPage
      .waitForFunction(
        () => document.querySelector('[data-testid="realtime-status-indicator"]')?.dataset.status === "live",
        { timeout: TIMEOUT_MS },
      )
      .then(() => true)
      .catch(() => false);
    if (!live) fail(failures, "owner never reached live status");
    else pass("owner is live");

    console.log("[step] owner marks a row acquired; editor sees it");
    await ownerPage.click('button[aria-label="Mark as acquired"]');
    const ownerAfter = await waitForState(ownerPage, (s) => s.acquired === 1, 10000);
    if (ownerAfter.acquired !== 1) fail(failures, "owner's own edit did not render");
    const editorAfter = await waitForState(editorPage, (s) => s.acquired === 1, 15000);
    if (editorAfter.acquired !== 1) fail(failures, `editor did not converge: ${JSON.stringify(editorAfter)}`);
    else pass("editor converged without a reload");

    console.log("[step] owner edits offline, then reconnects");
    await ownerPage.setOfflineMode(true);
    await ownerPage.click('button[aria-label="Mark unacquired"]');
    const offline = await waitForState(ownerPage, (s) => s.acquired === 0, 10000);
    if (offline.acquired !== 0) fail(failures, "offline edit did not apply locally");
    else pass("offline edit applied locally");
    const editorStill = await rowState(editorPage);
    if (editorStill.acquired !== 1) fail(failures, "editor changed while the owner was offline");
    await ownerPage.setOfflineMode(false);
    const editorSynced = await waitForState(editorPage, (s) => s.acquired === 0, 20000);
    if (editorSynced.acquired !== 0) fail(failures, `editor did not receive the offline edit: ${JSON.stringify(editorSynced)}`);
    else pass("offline edit converged after reconnect");
    const server = await serverAcquired(ownerPage, listId);
    if (server.some((a) => a !== 0)) fail(failures, `server rows not all unacquired: ${server}`);
    else pass("server agrees");

    console.log("[step] Ctrl+Z on the owner");
    await ownerPage.keyboard.down("Control");
    await ownerPage.keyboard.press("z");
    await ownerPage.keyboard.up("Control");
    const undone = await waitForState(ownerPage, (s) => s.acquired === 1, 10000);
    if (undone.acquired !== 1) fail(failures, "undo did not revert the last edit");
    const editorUndone = await waitForState(editorPage, (s) => s.acquired === 1, 15000);
    if (editorUndone.acquired !== 1) fail(failures, "undo did not reach the editor");
    else pass("undo reverted for everyone");
    const serverUndone = await serverAcquired(ownerPage, listId);
    if (!serverUndone.includes(1)) fail(failures, `server did not record the undo: ${serverUndone}`);

    console.log("[step] a REST add appears on both pages");
    const add = await api(ownerPage, "POST", `/api/v1/list/${listId}/add/item`, {
      id: 0,
      item_id: 7,
      list_id: listId,
      hq: null,
      quantity: 2,
      acquired: 0,
    });
    if (add.status !== 200) fail(failures, `REST add: ${add.status}`);
    const ownerThree = await waitForState(ownerPage, (s) => s.unacquired + s.acquired === 3, 15000);
    const editorThree = await waitForState(editorPage, (s) => s.unacquired + s.acquired === 3, 15000);
    if (ownerThree.unacquired + ownerThree.acquired !== 3) fail(failures, "owner missed the REST add");
    if (editorThree.unacquired + editorThree.acquired !== 3) fail(failures, "editor missed the REST add");
    else pass("REST write reached both documents");

    console.log("[step] the legacy page agrees");
    await legacyPage.goto(listUrl, { waitUntil: "domcontentloaded" });
    await waitForHydration(legacyPage, TIMEOUT_MS);
    const legacy = await waitForState(legacyPage, (s) => s.unacquired + s.acquired === 3, TIMEOUT_MS);
    if (legacy.acquired !== 1 || legacy.unacquired !== 2) fail(failures, `legacy page disagrees: ${JSON.stringify(legacy)}`);
    else pass("legacy page shows the same rows");
  } catch (e) {
    fail(failures, `uncaught: ${e && e.stack ? e.stack : e}`);
  } finally {
    if (ownerPage && listId) {
      await api(ownerPage, "DELETE", `/api/v1/list/${listId}/delete`).catch(() => {});
    }
    await browser.close();
  }

  if (failures.length) {
    console.error(`\n${failures.length} failure(s)`);
    process.exit(1);
  }
  console.log("\nlist-sync: all checks passed");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
```

The two fixed item ids (5, 6, 7) are real game items; `add/item` posts the same `ListItem` shape the page posts.

- [ ] **Step 2: Wire the script**

In `integration/package.json` add `"test:list-sync": "node ./list-sync.cjs",` after the `test:list-flow` line. In `scripts/run_e2e.sh`, inside the `test-auth` block right after the `list-flow` invocation, add:

```bash
        log "running list-sync E2E (test-auth feature detected)"
        list_sync_exit=0
        ( cd integration && BASE_URL="$BASE_URL" npm run test:list-sync ) || list_sync_exit=$?
```

and include `list_sync_exit` wherever the script sums or reports the other `*_exit` codes (mirror the `list_flow_exit` handling exactly).

- [ ] **Step 3: Run it, and run the list flow both ways**

Against a test-auth server:

```bash
cd integration && BASE_URL=http://127.0.0.1:8080 npm run test:list-sync
cd integration && BASE_URL=http://127.0.0.1:8080 npm run test:list-flow
cd integration && BASE_URL=http://127.0.0.1:8080 LABS_COOKIE=lists-sync npm run test:list-flow
```

Expected: all three pass. The most likely first failure is the acquired count after `Mark as acquired` under Labs: the row's toggle sets `acquired = quantity` through `edit_item`, which the adapter applies as `set_acquired`; if the count stays 0, check that `Edit::Edit` finds the row by `row_id`.

- [ ] **Step 4: Commit**

```bash
git add integration/list-sync.cjs integration/package.json scripts/run_e2e.sh
git commit -m "test(e2e): two-browser list sync, offline convergence and undo

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 10: Full verification and the spec's deviations

- [ ] **Step 1: CI**

Run: `./check_ci.sh > "$SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"`
Expected: `REAL_EXIT=0`.

- [ ] **Step 2: Record the deviations in the spec**

In `docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md` section 3.2, replace the `AutoMarkPurchases` bullet with: "`AutoMarkPurchases` gains an optional `on_purchase` callback; the Labs page passes one that adds one acquired unit to the first row of that item with room, through the document." Replace the sentence about actions being replaced with: "The Labs page keeps a `Resource` of today's result type, built from the document plus a per-list listings cache, so `AutoMarkPurchases` and the page body are untouched; only the actions' bodies and the realtime effects differ."

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md
git commit -m "docs(spec): record the client adapter as built

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Self-review

- Spec coverage: 3.1 store, sync status, handle, page-open sequence including the offline path (Tasks 2, 4, 5, 8); 3.2 adapter, ids, actions, bulk paths on REST, auto-mark (Tasks 3, 7, 8); 3.3 undo manager settings live in `ListUndo` (Phase 2), keys and the editable/modal guard (Task 6), no toast (none added); 8's client-side tests: key classifier, adapter mapping, LRU with a fake storage (Tasks 2, 3, 6), the two-browser suite with offline, undo, REST and legacy checks (Task 9), and `list-flow` both ways (Task 9). The `LabsSettings` snapshot test from section 8 belongs to Phase 1's component and is added there if `insta` coverage is wanted; it is not blocking.
- Placeholders: none. Every step has code or an exact command.
- Type consistency: `ListDocHandle::apply(Edit) -> Result<(), DocError>` with `AppError::from(DocError)` in every action; `handle.status: RwSignal<String>` converts into the `Signal<String>` the existing `RealtimeStatus` prop takes; `load_view` returns `ListViewResult` as the `Resource` expects; `start(...)` returns `RealtimeSubscription` stored like the existing subscriptions; `ListPermission as i16` round-trips through `ListPermission::from(i16)`.
