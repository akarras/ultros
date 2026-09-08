# Lists Sync Phase 2: `ultros-list-doc` Crate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A shared workspace crate wrapping one Loro document per list with typed operations, snapshot and update export, local undo, and property tests that prove three peers converge. No consumers yet.

**Architecture:** `ListDocument` owns a `loro::LoroDoc` with two root maps, `meta` and `rows`. Rows are nested `LoroMap`s keyed by `"{item_id}:{quality}"`, with `acquired` as a `LoroCounter`. Every public mutation commits its own transaction so Loro's undo manager and the local-update subscription see one operation per user action. `ListUndo` wraps `loro::UndoManager`. Reading goes through container accessors, never through JSON, so the row shape is explicit.

**Tech Stack:** Rust 2024 edition, `loro = "=1.16.0"` (default features include `counter`), `thiserror`, `ultros-api-types` for `AnySelector`.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md`, sections 1, 2, 3.3 and 8. The spec already records three as-built decisions this plan follows: peer ids are Loro's default random id per document (never stored or shared, per Loro's `set_peer_id` guidance); `MetaSnapshot.scope` is `Option<AnySelector>` so a document with no scope written reads as "leave the list's scope alone"; export methods return `Result` because Loro's encoder can fail.
- Pin `loro = "=1.16.0"`. Bumps are deliberate.
- No Leptos, no database, no network in this crate.
- Run `./check_ci.sh` before every commit; it runs fmt, clippy `-D warnings`, and tests.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

---

## File structure

| File | Responsibility |
|---|---|
| `Cargo.toml` (workspace, modify) | add `"ultros-list-doc"` to `members` |
| `ultros-list-doc/Cargo.toml` (create) | manifest |
| `ultros-list-doc/src/lib.rs` (create) | module list and re-exports |
| `ultros-list-doc/src/key.rs` (create) | `Quality`, `RowKey`, `KeyError`, parse and display |
| `ultros-list-doc/src/snapshot.rs` (create) | `RowSnapshot`, `MetaSnapshot`, `RowChange`, `diff_rows`, scope encoding |
| `ultros-list-doc/src/document.rs` (create) | `ListDocument`, `DocError`, `ImportReport` |
| `ultros-list-doc/src/undo.rs` (create) | `ListUndo` |
| `ultros-list-doc/tests/convergence.rs` (create) | three-peer property test |

---

### Task 1: Crate scaffold and row keys

**Files:**
- Modify: `Cargo.toml` (workspace `members`)
- Create: `ultros-list-doc/Cargo.toml`, `ultros-list-doc/src/lib.rs`, `ultros-list-doc/src/key.rs`

**Interfaces:**
- Produces: `Quality { Any, Hq, Nq }` with `as_str()`, `From<Option<bool>>`, `Into<Option<bool>>`, `FromStr`; `RowKey { item_id: i32, quality: Quality }` with `RowKey::new(item_id, hq: Option<bool>)`, `hq()`, `Display` as `"{item_id}:{quality}"`, `FromStr`; `KeyError`.

- [ ] **Step 1: Add the workspace member**

In the root `Cargo.toml` `members` list, add `"ultros-list-doc",` after `"ultros-api-types",`.

- [ ] **Step 2: Write the manifest**

`ultros-list-doc/Cargo.toml`:

```toml
[package]
name = "ultros-list-doc"
version = "0.1.0"
edition = "2024"
license = "MIT"
description = "The local-first list document: one Loro CRDT per list, shared by the server peer and the browser."

[dependencies]
# Pinned exactly: the convergence tests are the gate for any bump.
loro = "=1.16.0"
thiserror = { workspace = true }
ultros-api-types = { path = "../ultros-api-types" }
```

- [ ] **Step 3: Write `lib.rs`**

```rust
//! The list document: one Loro CRDT per list, shared by the server peer and
//! the browser. No Leptos, no database, no network (spec section 1).

pub mod key;

pub use key::{KeyError, Quality, RowKey};
```

- [ ] **Step 4: Write the failing key tests and the module**

`ultros-list-doc/src/key.rs`:

```rust
//! Row identity: the natural key the server already dedupes on.

use std::fmt;
use std::str::FromStr;

/// Mirrors `ListItem.hq`: `None` is "any", `Some(true)` HQ, `Some(false)` NQ.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Quality {
    Any,
    Hq,
    Nq,
}

impl Quality {
    pub fn as_str(self) -> &'static str {
        match self {
            Quality::Any => "any",
            Quality::Hq => "hq",
            Quality::Nq => "nq",
        }
    }
}

impl From<Option<bool>> for Quality {
    fn from(hq: Option<bool>) -> Self {
        match hq {
            None => Quality::Any,
            Some(true) => Quality::Hq,
            Some(false) => Quality::Nq,
        }
    }
}

impl From<Quality> for Option<bool> {
    fn from(quality: Quality) -> Self {
        match quality {
            Quality::Any => None,
            Quality::Hq => Some(true),
            Quality::Nq => Some(false),
        }
    }
}

impl FromStr for Quality {
    type Err = KeyError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "any" => Ok(Quality::Any),
            "hq" => Ok(Quality::Hq),
            "nq" => Ok(Quality::Nq),
            other => Err(KeyError::Quality(other.to_string())),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowKey {
    pub item_id: i32,
    pub quality: Quality,
}

impl RowKey {
    pub fn new(item_id: i32, hq: Option<bool>) -> Self {
        Self {
            item_id,
            quality: Quality::from(hq),
        }
    }

    pub fn hq(&self) -> Option<bool> {
        self.quality.into()
    }
}

impl fmt::Display for RowKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.item_id, self.quality.as_str())
    }
}

impl FromStr for RowKey {
    type Err = KeyError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (item, quality) = s.split_once(':').ok_or_else(|| KeyError::Shape(s.to_string()))?;
        let item_id = item
            .parse::<i32>()
            .map_err(|_| KeyError::ItemId(item.to_string()))?;
        Ok(Self {
            item_id,
            quality: quality.parse()?,
        })
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeyError {
    #[error("row key `{0}` is not `item:quality`")]
    Shape(String),
    #[error("row key item id `{0}` is not a number")]
    ItemId(String),
    #[error("row key quality `{0}` is not any, hq or nq")]
    Quality(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_round_trips_through_display_for_every_quality() {
        for hq in [None, Some(true), Some(false)] {
            let key = RowKey::new(4567, hq);
            let text = key.to_string();
            let parsed: RowKey = text.parse().unwrap();
            assert_eq!(parsed, key, "{text}");
            assert_eq!(parsed.hq(), hq);
        }
        assert_eq!(RowKey::new(1, None).to_string(), "1:any");
        assert_eq!(RowKey::new(1, Some(true)).to_string(), "1:hq");
        assert_eq!(RowKey::new(1, Some(false)).to_string(), "1:nq");
    }

    #[test]
    fn keys_order_by_item_then_quality() {
        let mut keys = vec![
            RowKey::new(2, None),
            RowKey::new(1, Some(false)),
            RowKey::new(1, Some(true)),
            RowKey::new(1, None),
        ];
        keys.sort();
        assert_eq!(
            keys,
            vec![
                RowKey::new(1, None),
                RowKey::new(1, Some(true)),
                RowKey::new(1, Some(false)),
                RowKey::new(2, None),
            ]
        );
    }

    #[test]
    fn junk_keys_are_rejected_with_the_right_error() {
        assert_eq!(
            "nocolon".parse::<RowKey>(),
            Err(KeyError::Shape("nocolon".into()))
        );
        assert_eq!("x:hq".parse::<RowKey>(), Err(KeyError::ItemId("x".into())));
        assert_eq!(
            "5:shiny".parse::<RowKey>(),
            Err(KeyError::Quality("shiny".into()))
        );
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ultros-list-doc`
Expected: 3 passed. (This also downloads and compiles `loro` for the first time; it takes a minute.)

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock ultros-list-doc
git commit -m "feat(list-doc): new crate with typed row keys

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Snapshots, row diff and scope encoding

**Files:**
- Create: `ultros-list-doc/src/snapshot.rs`
- Modify: `ultros-list-doc/src/lib.rs`

**Interfaces:**
- Consumes: `RowKey` (Task 1); `ultros_api_types::world_helper::AnySelector` (`Copy`, `Eq`, `Ord`).
- Produces: `RowSnapshot { key, need: i64, acquired: i64, target: Option<i64> }` (`Copy`), `MetaSnapshot { name: String, scope: Option<AnySelector> }`, `RowChange::{Added, Removed, Updated { before, after }}` with `key()`, `diff_rows(&[RowSnapshot], &[RowSnapshot]) -> Vec<RowChange>` sorted by key, `encode_scope(AnySelector) -> String`, `parse_scope(&str) -> Option<AnySelector>`.

- [ ] **Step 1: Write the module with tests**

```rust
//! Plain-value views of a document, and the diff between two of them.

use std::collections::BTreeMap;

use ultros_api_types::world_helper::AnySelector;

use crate::key::RowKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowSnapshot {
    pub key: RowKey,
    pub need: i64,
    pub acquired: i64,
    pub target: Option<i64>,
}

/// `scope` is `None` only for a document nobody wrote a scope into; consumers
/// treat that as "leave the list's scope alone".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetaSnapshot {
    pub name: String,
    pub scope: Option<AnySelector>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowChange {
    Added(RowSnapshot),
    Removed(RowSnapshot),
    Updated { before: RowSnapshot, after: RowSnapshot },
}

impl RowChange {
    pub fn key(&self) -> RowKey {
        match self {
            RowChange::Added(row) | RowChange::Removed(row) => row.key,
            RowChange::Updated { after, .. } => after.key,
        }
    }
}

/// Every row that differs between two snapshots, ordered by key. Inputs may be
/// in any order.
pub fn diff_rows(before: &[RowSnapshot], after: &[RowSnapshot]) -> Vec<RowChange> {
    let before: BTreeMap<RowKey, RowSnapshot> = before.iter().map(|r| (r.key, *r)).collect();
    let after: BTreeMap<RowKey, RowSnapshot> = after.iter().map(|r| (r.key, *r)).collect();
    let mut changes = Vec::new();
    for (key, b) in &before {
        match after.get(key) {
            None => changes.push(RowChange::Removed(*b)),
            Some(a) if a != b => changes.push(RowChange::Updated {
                before: *b,
                after: *a,
            }),
            Some(_) => {}
        }
    }
    for (key, a) in &after {
        if !before.contains_key(key) {
            changes.push(RowChange::Added(*a));
        }
    }
    changes.sort_by_key(|c| c.key());
    changes
}

pub fn encode_scope(scope: AnySelector) -> String {
    match scope {
        AnySelector::World(id) => format!("world:{id}"),
        AnySelector::Datacenter(id) => format!("datacenter:{id}"),
        AnySelector::Region(id) => format!("region:{id}"),
    }
}

pub fn parse_scope(text: &str) -> Option<AnySelector> {
    let (kind, id) = text.split_once(':')?;
    let id: i32 = id.parse().ok()?;
    match kind {
        "world" => Some(AnySelector::World(id)),
        "datacenter" => Some(AnySelector::Datacenter(id)),
        "region" => Some(AnySelector::Region(id)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(item: i32, need: i64, acquired: i64, target: Option<i64>) -> RowSnapshot {
        RowSnapshot {
            key: RowKey::new(item, None),
            need,
            acquired,
            target,
        }
    }

    #[test]
    fn diff_classifies_added_removed_and_updated_in_key_order() {
        let before = [row(3, 1, 0, None), row(1, 2, 0, None), row(2, 5, 5, Some(10))];
        let after = [row(2, 5, 6, Some(10)), row(1, 2, 0, None), row(4, 1, 0, None)];
        let changes = diff_rows(&before, &after);
        assert_eq!(
            changes,
            vec![
                RowChange::Updated {
                    before: row(2, 5, 5, Some(10)),
                    after: row(2, 5, 6, Some(10)),
                },
                RowChange::Removed(row(3, 1, 0, None)),
                RowChange::Added(row(4, 1, 0, None)),
            ]
        );
    }

    #[test]
    fn diff_is_empty_for_equal_snapshots_in_different_orders() {
        let a = [row(1, 1, 0, None), row(2, 1, 0, None)];
        let b = [row(2, 1, 0, None), row(1, 1, 0, None)];
        assert!(diff_rows(&a, &b).is_empty());
    }

    #[test]
    fn scope_encoding_round_trips_every_variant_and_rejects_junk() {
        for scope in [
            AnySelector::World(79),
            AnySelector::Datacenter(5),
            AnySelector::Region(1),
        ] {
            assert_eq!(parse_scope(&encode_scope(scope)), Some(scope));
        }
        assert_eq!(encode_scope(AnySelector::World(79)), "world:79");
        assert_eq!(parse_scope("planet:1"), None);
        assert_eq!(parse_scope("world:x"), None);
        assert_eq!(parse_scope("world"), None);
    }
}
```

- [ ] **Step 2: Register and re-export**

`lib.rs` becomes:

```rust
//! The list document: one Loro CRDT per list, shared by the server peer and
//! the browser. No Leptos, no database, no network (spec section 1).

pub mod key;
pub mod snapshot;

pub use key::{KeyError, Quality, RowKey};
pub use snapshot::{MetaSnapshot, RowChange, RowSnapshot, diff_rows, encode_scope, parse_scope};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p ultros-list-doc`
Expected: 6 passed.

- [ ] **Step 4: Commit**

```bash
git add ultros-list-doc
git commit -m "feat(list-doc): snapshots, row diff and scope encoding

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: `ListDocument` construction and reads

**Files:**
- Create: `ultros-list-doc/src/document.rs`
- Modify: `ultros-list-doc/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 and Task 2 types; `loro::{LoroDoc, LoroMap, LoroCounter, LoroValue, ValueOrContainer, Container}`.
- Produces: `ListDocument::new()`, `from_snapshot(&[u8]) -> Result<Self, DocError>`, `from_rows(MetaSnapshot, &[RowSnapshot]) -> Self`, `peer_id() -> u64`, `inner() -> &LoroDoc`, `meta() -> MetaSnapshot`, `rows() -> Vec<RowSnapshot>` (sorted by key), `row(&RowKey) -> Option<RowSnapshot>`; `DocError`; `SCHEMA_VERSION: i64 = 1`. Later tasks add methods to this same `impl` block.

- [ ] **Step 1: Write the module**

```rust
//! `ListDocument`: the typed wrapper around one Loro document (spec section 1).
//!
//! Layout:
//!
//! ```text
//! root map "meta": { name: string, scope: "world:79" | "datacenter:5" | "region:1", schema: 1 }
//! root map "rows": { "{item_id}:{quality}": { item: i64, quality: string, need: i64,
//!                                            target: i64 (absent = none), acquired: Counter } }
//! ```

use loro::{Container, LoroCounter, LoroDoc, LoroMap, LoroValue, ValueOrContainer};
use ultros_api_types::world_helper::AnySelector;

use crate::key::RowKey;
use crate::snapshot::{MetaSnapshot, RowSnapshot, encode_scope, parse_scope};

pub const SCHEMA_VERSION: i64 = 1;

const META: &str = "meta";
const ROWS: &str = "rows";
const NAME: &str = "name";
const SCOPE: &str = "scope";
const SCHEMA: &str = "schema";
const ITEM: &str = "item";
const QUALITY: &str = "quality";
const NEED: &str = "need";
const TARGET: &str = "target";
const ACQUIRED: &str = "acquired";

#[derive(Debug, thiserror::Error)]
pub enum DocError {
    #[error("loro: {0}")]
    Loro(#[from] loro::LoroError),
    #[error("loro encode: {0}")]
    Encode(#[from] loro::LoroEncodeError),
    #[error("version vector bytes are invalid")]
    Version,
    #[error("row `{0}` is not in the document")]
    MissingRow(RowKey),
}

/// What an import did. `pending` means the bytes depend on history this
/// document has not seen; Loro applies them once that history arrives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportReport {
    pub pending: bool,
}

/// Cheap to clone: clones share the same underlying document.
#[derive(Clone)]
pub struct ListDocument {
    doc: LoroDoc,
}

impl Default for ListDocument {
    fn default() -> Self {
        Self::new()
    }
}

fn value_i64(value: Option<ValueOrContainer>) -> Option<i64> {
    match value {
        Some(ValueOrContainer::Value(LoroValue::I64(n))) => Some(n),
        Some(ValueOrContainer::Value(LoroValue::Double(d))) => Some(d.round() as i64),
        _ => None,
    }
}

fn value_string(value: Option<ValueOrContainer>) -> Option<String> {
    match value {
        Some(ValueOrContainer::Value(LoroValue::String(s))) => Some(s.as_str().to_string()),
        _ => None,
    }
}

impl ListDocument {
    /// An empty document with Loro's default random peer id. Two tabs are two
    /// peers: a peer id is never stored or shared between concurrent writers.
    pub fn new() -> Self {
        Self { doc: LoroDoc::new() }
    }

    pub fn from_snapshot(bytes: &[u8]) -> Result<Self, DocError> {
        let document = Self::new();
        document.doc.import(bytes)?;
        Ok(document)
    }

    /// Build a document from relational rows: the server's first-touch path.
    pub fn from_rows(meta: MetaSnapshot, rows: &[RowSnapshot]) -> Self {
        let document = Self::new();
        let meta_map = document.meta_map();
        // Inserting plain values into a fresh attached map cannot fail.
        meta_map.insert(NAME, meta.name.as_str()).expect("fresh map insert");
        if let Some(scope) = meta.scope {
            meta_map
                .insert(SCOPE, encode_scope(scope).as_str())
                .expect("fresh map insert");
        }
        meta_map.insert(SCHEMA, SCHEMA_VERSION).expect("fresh map insert");
        for row in rows {
            document
                .insert_row(row.key, row.need, row.target, row.acquired)
                .expect("fresh row insert");
        }
        document.doc.commit();
        document
    }

    pub fn peer_id(&self) -> u64 {
        self.doc.peer_id()
    }

    /// The raw document, for the undo manager.
    pub fn inner(&self) -> &LoroDoc {
        &self.doc
    }

    fn meta_map(&self) -> LoroMap {
        self.doc.get_map(META)
    }

    fn rows_map(&self) -> LoroMap {
        self.doc.get_map(ROWS)
    }

    fn row_container(&self, key: &RowKey) -> Option<LoroMap> {
        match self.rows_map().get(&key.to_string())? {
            ValueOrContainer::Container(Container::Map(map)) => Some(map),
            _ => None,
        }
    }

    fn read_row(key: RowKey, row: &LoroMap) -> RowSnapshot {
        let need = value_i64(row.get(NEED)).unwrap_or(0);
        let target = value_i64(row.get(TARGET));
        let acquired = match row.get(ACQUIRED) {
            Some(ValueOrContainer::Container(Container::Counter(counter))) => {
                counter.get_value().round() as i64
            }
            _ => 0,
        };
        RowSnapshot {
            key,
            need,
            acquired,
            target,
        }
    }

    fn insert_row(
        &self,
        key: RowKey,
        need: i64,
        target: Option<i64>,
        acquired: i64,
    ) -> Result<(), DocError> {
        let row = self
            .rows_map()
            .insert_container(&key.to_string(), LoroMap::new())?;
        row.insert(ITEM, key.item_id as i64)?;
        row.insert(QUALITY, key.quality.as_str())?;
        row.insert(NEED, need)?;
        if let Some(target) = target {
            row.insert(TARGET, target)?;
        }
        let counter = row.insert_container(ACQUIRED, LoroCounter::new())?;
        if acquired != 0 {
            counter.increment(acquired as f64)?;
        }
        Ok(())
    }

    pub fn meta(&self) -> MetaSnapshot {
        let meta = self.meta_map();
        MetaSnapshot {
            name: value_string(meta.get(NAME)).unwrap_or_default(),
            scope: value_string(meta.get(SCOPE)).and_then(|s| parse_scope(&s)),
        }
    }

    /// Every row, sorted by key.
    pub fn rows(&self) -> Vec<RowSnapshot> {
        let rows = self.rows_map();
        let mut out: Vec<RowSnapshot> = rows
            .keys()
            .filter_map(|k| {
                let text = k.to_string();
                let key: RowKey = text.parse().ok()?;
                match rows.get(&text)? {
                    ValueOrContainer::Container(Container::Map(map)) => {
                        Some(Self::read_row(key, &map))
                    }
                    _ => None,
                }
            })
            .collect();
        out.sort_by_key(|r| r.key);
        out
    }

    pub fn row(&self, key: &RowKey) -> Option<RowSnapshot> {
        self.row_container(key)
            .map(|row| Self::read_row(*key, &row))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Quality;

    pub(crate) fn meta() -> MetaSnapshot {
        MetaSnapshot {
            name: "Housing".to_string(),
            scope: Some(AnySelector::Datacenter(5)),
        }
    }

    pub(crate) fn row(item: i32, quality: Quality, need: i64, acquired: i64) -> RowSnapshot {
        RowSnapshot {
            key: RowKey { item_id: item, quality },
            need,
            acquired,
            target: None,
        }
    }

    #[test]
    fn from_rows_reads_back_meta_and_sorted_rows() {
        let rows = [
            row(20, Quality::Hq, 3, 1),
            row(10, Quality::Any, 2, 0),
            RowSnapshot {
                target: Some(500),
                ..row(10, Quality::Nq, 1, 1)
            },
        ];
        let doc = ListDocument::from_rows(meta(), &rows);
        assert_eq!(doc.meta(), meta());
        let mut expected = rows.to_vec();
        expected.sort_by_key(|r| r.key);
        assert_eq!(doc.rows(), expected);
        assert_eq!(doc.row(&RowKey::new(10, Some(false))).unwrap().target, Some(500));
        assert_eq!(doc.row(&RowKey::new(99, None)), None);
    }

    #[test]
    fn an_empty_document_has_no_scope_and_no_rows() {
        let doc = ListDocument::new();
        assert_eq!(
            doc.meta(),
            MetaSnapshot {
                name: String::new(),
                scope: None
            }
        );
        assert!(doc.rows().is_empty());
    }

    #[test]
    fn two_documents_get_different_peer_ids() {
        assert_ne!(ListDocument::new().peer_id(), ListDocument::new().peer_id());
    }

    #[test]
    fn a_snapshot_round_trips_into_a_fresh_document() {
        let doc = ListDocument::from_rows(meta(), &[row(1, Quality::Any, 4, 2)]);
        let bytes = doc.inner().export(loro::ExportMode::Snapshot).unwrap();
        let copy = ListDocument::from_snapshot(&bytes).unwrap();
        assert_eq!(copy.rows(), doc.rows());
        assert_eq!(copy.meta(), doc.meta());
        assert!(ListDocument::from_snapshot(b"not a snapshot").is_err());
    }
}
```

- [ ] **Step 2: Register and re-export**

Add `pub mod document;` to `lib.rs` and `pub use document::{DocError, ImportReport, ListDocument, SCHEMA_VERSION};`.

- [ ] **Step 3: Run the tests**

Run: `cargo test -p ultros-list-doc`
Expected: 10 passed.

- [ ] **Step 4: Commit**

```bash
git add ultros-list-doc
git commit -m "feat(list-doc): ListDocument construction and reads

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: `ListDocument` mutations

**Files:**
- Modify: `ultros-list-doc/src/document.rs` (add to the `impl ListDocument` block and its tests)

**Interfaces:**
- Produces: `rename(&str)`, `set_scope(AnySelector)`, `add_row(RowKey, need, target)` (merges `need` into an existing key), `remove_row(&RowKey)`, `set_need(&RowKey, i64)`, `set_target(&RowKey, Option<i64>)`, `set_quality(&RowKey, Quality) -> Result<RowKey, DocError>`, `add_acquired(&RowKey, i64)`, `set_acquired(&RowKey, i64)`, `commit()`. All return `Result<_, DocError>` and commit their own transaction.

- [ ] **Step 1: Add the failing tests**

Inside `mod tests`:

```rust
    #[test]
    fn add_row_creates_then_merges_need_into_the_same_key() {
        let doc = ListDocument::from_rows(meta(), &[]);
        let key = RowKey::new(10, None);
        doc.add_row(key, 2, None).unwrap();
        doc.add_row(key, 3, Some(90)).unwrap();
        assert_eq!(doc.rows(), vec![RowSnapshot { key, need: 5, acquired: 0, target: Some(90) }]);
    }

    #[test]
    fn remove_row_deletes_and_reports_a_missing_key() {
        let key = RowKey::new(10, None);
        let doc = ListDocument::from_rows(meta(), &[row(10, Quality::Any, 1, 0)]);
        doc.remove_row(&key).unwrap();
        assert!(doc.rows().is_empty());
        assert!(matches!(doc.remove_row(&key), Err(DocError::MissingRow(k)) if k == key));
    }

    #[test]
    fn need_target_and_acquired_edits_land_on_the_row() {
        let key = RowKey::new(10, None);
        let doc = ListDocument::from_rows(meta(), &[row(10, Quality::Any, 1, 0)]);
        doc.set_need(&key, 7).unwrap();
        doc.set_target(&key, Some(120)).unwrap();
        doc.add_acquired(&key, 2).unwrap();
        doc.add_acquired(&key, 3).unwrap();
        assert_eq!(doc.row(&key).unwrap(), RowSnapshot { key, need: 7, acquired: 5, target: Some(120) });
        doc.set_acquired(&key, 1).unwrap();
        doc.set_target(&key, None).unwrap();
        doc.set_need(&key, -4).unwrap();
        assert_eq!(doc.row(&key).unwrap(), RowSnapshot { key, need: 0, acquired: 1, target: None });
    }

    #[test]
    fn set_quality_moves_the_row_with_every_field() {
        let key = RowKey::new(10, None);
        let doc = ListDocument::from_rows(
            meta(),
            &[RowSnapshot { target: Some(50), ..row(10, Quality::Any, 4, 2) }],
        );
        let moved = doc.set_quality(&key, Quality::Hq).unwrap();
        assert_eq!(moved, RowKey::new(10, Some(true)));
        assert_eq!(doc.row(&key), None);
        assert_eq!(doc.row(&moved).unwrap(), RowSnapshot { key: moved, need: 4, acquired: 2, target: Some(50) });
        assert_eq!(doc.set_quality(&moved, Quality::Hq).unwrap(), moved, "no-op move keeps the key");
    }

    #[test]
    fn set_quality_merges_into_an_existing_row_of_that_quality() {
        let doc = ListDocument::from_rows(
            meta(),
            &[row(10, Quality::Any, 2, 1), row(10, Quality::Hq, 3, 0)],
        );
        let moved = doc.set_quality(&RowKey::new(10, None), Quality::Hq).unwrap();
        assert_eq!(doc.rows(), vec![RowSnapshot { key: moved, need: 5, acquired: 1, target: None }]);
    }

    #[test]
    fn rename_and_scope_write_meta() {
        let doc = ListDocument::from_rows(meta(), &[]);
        doc.rename("Glamour").unwrap();
        doc.set_scope(AnySelector::World(79)).unwrap();
        assert_eq!(doc.meta(), MetaSnapshot { name: "Glamour".into(), scope: Some(AnySelector::World(79)) });
    }
```

- [ ] **Step 2: Run them to see the compile failure**

Run: `cargo test -p ultros-list-doc`
Expected: FAIL, `no method named add_row`.

- [ ] **Step 3: Add the mutations**

Inside `impl ListDocument`, after `row()`:

```rust
    fn counter(row: &LoroMap) -> Result<LoroCounter, DocError> {
        match row.get(ACQUIRED) {
            Some(ValueOrContainer::Container(Container::Counter(counter))) => Ok(counter),
            // A row written by an older schema without a counter heals itself.
            _ => Ok(row.insert_container(ACQUIRED, LoroCounter::new())?),
        }
    }

    pub fn commit(&self) {
        self.doc.commit();
    }

    pub fn rename(&self, name: &str) -> Result<(), DocError> {
        self.meta_map().insert(NAME, name)?;
        self.doc.commit();
        Ok(())
    }

    pub fn set_scope(&self, scope: AnySelector) -> Result<(), DocError> {
        self.meta_map()
            .insert(SCOPE, encode_scope(scope).as_str())?;
        self.doc.commit();
        Ok(())
    }

    /// Add a row, or add `need` to the row already under that key, which is
    /// what the legacy add path has always done.
    pub fn add_row(&self, key: RowKey, need: i64, target: Option<i64>) -> Result<(), DocError> {
        match self.row_container(&key) {
            Some(row) => {
                let current = value_i64(row.get(NEED)).unwrap_or(0);
                row.insert(NEED, current + need)?;
                if let Some(target) = target {
                    row.insert(TARGET, target)?;
                }
            }
            None => self.insert_row(key, need, target, 0)?,
        }
        self.doc.commit();
        Ok(())
    }

    pub fn remove_row(&self, key: &RowKey) -> Result<(), DocError> {
        if self.row_container(key).is_none() {
            return Err(DocError::MissingRow(*key));
        }
        self.rows_map().delete(&key.to_string())?;
        self.doc.commit();
        Ok(())
    }

    pub fn set_need(&self, key: &RowKey, need: i64) -> Result<(), DocError> {
        let row = self.row_container(key).ok_or(DocError::MissingRow(*key))?;
        row.insert(NEED, need.max(0))?;
        self.doc.commit();
        Ok(())
    }

    pub fn set_target(&self, key: &RowKey, target: Option<i64>) -> Result<(), DocError> {
        let row = self.row_container(key).ok_or(DocError::MissingRow(*key))?;
        match target {
            Some(target) => row.insert(TARGET, target)?,
            None => {
                if row.get(TARGET).is_some() {
                    row.delete(TARGET)?;
                }
            }
        }
        self.doc.commit();
        Ok(())
    }

    /// Move the row to another quality, carrying every field. Merges into a
    /// row that already has the target quality. Returns the new key.
    pub fn set_quality(&self, key: &RowKey, quality: Quality) -> Result<RowKey, DocError> {
        let snapshot = self.row(key).ok_or(DocError::MissingRow(*key))?;
        let new_key = RowKey {
            item_id: key.item_id,
            quality,
        };
        if new_key == *key {
            return Ok(new_key);
        }
        self.rows_map().delete(&key.to_string())?;
        match self.row_container(&new_key) {
            Some(existing) => {
                let current = value_i64(existing.get(NEED)).unwrap_or(0);
                existing.insert(NEED, current + snapshot.need)?;
                if let Some(target) = snapshot.target {
                    existing.insert(TARGET, target)?;
                }
                if snapshot.acquired != 0 {
                    Self::counter(&existing)?.increment(snapshot.acquired as f64)?;
                }
            }
            None => self.insert_row(new_key, snapshot.need, snapshot.target, snapshot.acquired)?,
        }
        self.doc.commit();
        Ok(new_key)
    }

    pub fn add_acquired(&self, key: &RowKey, delta: i64) -> Result<(), DocError> {
        let row = self.row_container(key).ok_or(DocError::MissingRow(*key))?;
        if delta != 0 {
            Self::counter(&row)?.increment(delta as f64)?;
        }
        self.doc.commit();
        Ok(())
    }

    /// Set an absolute value by incrementing the counter by the difference.
    pub fn set_acquired(&self, key: &RowKey, value: i64) -> Result<(), DocError> {
        let current = self.row(key).ok_or(DocError::MissingRow(*key))?.acquired;
        self.add_acquired(key, value - current)
    }
```

Add `use crate::key::Quality;` to the module imports (it is already imported inside `mod tests`; keep both).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-list-doc`
Expected: 16 passed.

- [ ] **Step 5: Commit**

```bash
git add ultros-list-doc
git commit -m "feat(list-doc): typed row and meta mutations

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Sync surface — versions, export, import, subscriptions

**Files:**
- Modify: `ultros-list-doc/src/document.rs`

**Interfaces:**
- Consumes: `loro::{ExportMode, VersionVector, Subscription}`.
- Produces: `version() -> Vec<u8>`, `export_snapshot() -> Result<Vec<u8>, DocError>`, `export_shallow() -> Result<Vec<u8>, DocError>`, `export_all() -> Result<Vec<u8>, DocError>`, `export_since(&[u8]) -> Result<Vec<u8>, DocError>` (empty slice means "everything"), `import(&[u8]) -> Result<ImportReport, DocError>`, `on_local_update(impl Fn(&[u8]) + Send + Sync + 'static) -> Subscription`, `on_change(impl Fn() + Send + Sync + 'static) -> Subscription`. A returned `Subscription` must be kept alive; dropping it unsubscribes.

- [ ] **Step 1: Add the failing tests**

Inside `mod tests`:

```rust
    #[test]
    fn export_since_carries_only_what_the_other_side_lacks() {
        let a = ListDocument::from_rows(meta(), &[row(1, Quality::Any, 1, 0)]);
        let b = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
        let synced_at = b.version();
        assert!(a.export_since(&synced_at).unwrap().len() < a.export_all().unwrap().len());
        a.set_need(&RowKey::new(1, None), 9).unwrap();
        let delta = a.export_since(&synced_at).unwrap();
        let report = b.import(&delta).unwrap();
        assert_eq!(report, ImportReport { pending: false });
        assert_eq!(b.row(&RowKey::new(1, None)).unwrap().need, 9);
        assert_eq!(b.version(), a.version());
        assert!(a.export_since(b"garbage").is_err());
        assert_eq!(a.export_since(&[]).unwrap().len(), a.export_all().unwrap().len());
    }

    #[test]
    fn a_shallow_snapshot_loads_into_a_fresh_document() {
        let key = RowKey::new(1, None);
        let a = ListDocument::from_rows(meta(), &[row(1, Quality::Any, 1, 0)]);
        for need in 2..40 {
            a.set_need(&key, need).unwrap();
        }
        let shallow = a.export_shallow().unwrap();
        assert!(shallow.len() < a.export_snapshot().unwrap().len());
        let fresh = ListDocument::from_snapshot(&shallow).unwrap();
        assert_eq!(fresh.rows(), a.rows());
        // A peer that starts from the shallow snapshot still syncs both ways.
        fresh.set_need(&key, 100).unwrap();
        a.import(&fresh.export_since(&a.version()).unwrap()).unwrap();
        assert_eq!(a.row(&key).unwrap().need, 100);
    }

    #[test]
    fn local_updates_fire_for_local_commits_only() {
        use std::sync::{Arc, Mutex};
        let a = ListDocument::from_rows(meta(), &[row(1, Quality::Any, 1, 0)]);
        let b = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
        let seen: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let _sub = a.on_local_update(move |bytes| sink.lock().unwrap().push(bytes.to_vec()));
        a.set_need(&RowKey::new(1, None), 2).unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1, "one commit, one update");
        b.set_need(&RowKey::new(1, None), 3).unwrap();
        a.import(&b.export_since(&a.version()).unwrap()).unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1, "imports are not local updates");
        // The captured bytes are a valid update for another peer.
        let c = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
        c.import(&seen.lock().unwrap()[0]).unwrap();
    }

    #[test]
    fn on_change_fires_for_local_and_remote_changes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let a = ListDocument::from_rows(meta(), &[row(1, Quality::Any, 1, 0)]);
        let b = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let sink = count.clone();
        let _sub = a.on_change(move || {
            sink.fetch_add(1, Ordering::SeqCst);
        });
        a.set_need(&RowKey::new(1, None), 2).unwrap();
        b.rename("remote").unwrap();
        a.import(&b.export_since(&a.version()).unwrap()).unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
```

- [ ] **Step 2: Run them to see the compile failure**

Run: `cargo test -p ultros-list-doc`
Expected: FAIL, `no method named version`.

- [ ] **Step 3: Add the sync surface**

Add to the imports at the top of `document.rs`:

```rust
use std::sync::Arc;

use loro::{ExportMode, Subscription, VersionVector};
```

(merge with the existing `use loro::{...}` line). Inside `impl ListDocument`:

```rust
    /// The encoded version vector: what this document has seen.
    pub fn version(&self) -> Vec<u8> {
        self.doc.oplog_vv().encode()
    }

    pub fn export_snapshot(&self) -> Result<Vec<u8>, DocError> {
        Ok(self.doc.export(ExportMode::Snapshot)?)
    }

    /// A snapshot that drops history before the current state. Cheaper to
    /// store; a fresh peer loads it and syncs both ways from there.
    pub fn export_shallow(&self) -> Result<Vec<u8>, DocError> {
        let frontiers = self.doc.state_frontiers();
        Ok(self.doc.export(ExportMode::shallow_snapshot(&frontiers))?)
    }

    pub fn export_all(&self) -> Result<Vec<u8>, DocError> {
        Ok(self.doc.export(ExportMode::all_updates())?)
    }

    /// Updates the holder of `version` has not seen. An empty `version` means
    /// everything.
    pub fn export_since(&self, version: &[u8]) -> Result<Vec<u8>, DocError> {
        let vv = if version.is_empty() {
            VersionVector::default()
        } else {
            VersionVector::decode(version).map_err(|_| DocError::Version)?
        };
        Ok(self.doc.export(ExportMode::updates(&vv))?)
    }

    pub fn import(&self, bytes: &[u8]) -> Result<ImportReport, DocError> {
        let status = self.doc.import(bytes)?;
        Ok(ImportReport {
            pending: status.pending.is_some(),
        })
    }

    /// Bytes for every local commit, ready to send to other peers. Remote
    /// imports do not fire this.
    pub fn on_local_update(&self, f: impl Fn(&[u8]) + Send + Sync + 'static) -> Subscription {
        self.doc.subscribe_local_update(Box::new(move |bytes: &Vec<u8>| {
            f(bytes.as_slice());
            true
        }))
    }

    /// Any change to the document, local or imported.
    pub fn on_change(&self, f: impl Fn() + Send + Sync + 'static) -> Subscription {
        self.doc.subscribe_root(Arc::new(move |_event| f()))
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultros-list-doc`
Expected: 20 passed.

- [ ] **Step 5: Commit**

```bash
git add ultros-list-doc
git commit -m "feat(list-doc): version, export, import and subscriptions

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: `ListUndo`

**Files:**
- Create: `ultros-list-doc/src/undo.rs`
- Modify: `ultros-list-doc/src/lib.rs`

**Interfaces:**
- Consumes: `ListDocument::inner()`, `loro::UndoManager` (`new`, `set_merge_interval`, `set_max_undo_steps`, `undo`, `redo`, `can_undo`, `can_redo`, `group_start`, `group_end`).
- Produces: `ListUndo::new(&ListDocument)`, `with_merge_interval(&ListDocument, i64)`, `undo() -> Result<bool, DocError>`, `redo() -> Result<bool, DocError>`, `can_undo()`, `can_redo()`, `group(FnOnce() -> Result<R, DocError>) -> Result<R, DocError>`; constants `MERGE_INTERVAL_MS = 1000`, `MAX_STEPS = 100`.

- [ ] **Step 1: Write the module with tests**

```rust
//! Local undo over one document (spec section 3.3). Remote imports are not
//! local operations, so they never enter the stack.

use loro::UndoManager;

use crate::document::{DocError, ListDocument};

pub struct ListUndo {
    inner: UndoManager,
}

impl ListUndo {
    /// Edits closer together than this merge into one undo step.
    pub const MERGE_INTERVAL_MS: i64 = 1000;
    pub const MAX_STEPS: usize = 100;

    pub fn new(doc: &ListDocument) -> Self {
        Self::with_merge_interval(doc, Self::MERGE_INTERVAL_MS)
    }

    /// Tests pass `0` so consecutive edits stay separate steps.
    pub fn with_merge_interval(doc: &ListDocument, interval_ms: i64) -> Self {
        let mut inner = UndoManager::new(doc.inner());
        inner.set_merge_interval(interval_ms);
        inner.set_max_undo_steps(Self::MAX_STEPS);
        Self { inner }
    }

    /// `Ok(false)` when there was nothing to undo.
    pub fn undo(&mut self) -> Result<bool, DocError> {
        Ok(self.inner.undo()?)
    }

    pub fn redo(&mut self) -> Result<bool, DocError> {
        Ok(self.inner.redo()?)
    }

    pub fn can_undo(&self) -> bool {
        self.inner.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.inner.can_redo()
    }

    /// Run several document edits as one undo step (bulk HQ, imports).
    pub fn group<R>(&mut self, f: impl FnOnce() -> Result<R, DocError>) -> Result<R, DocError> {
        self.inner.group_start()?;
        let result = f();
        self.inner.group_end();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{Quality, RowKey};
    use crate::snapshot::{MetaSnapshot, RowSnapshot};
    use ultros_api_types::world_helper::AnySelector;

    /// What Loro does when this device undoes a write that another peer
    /// overwrote in the meantime. Pinned by `undo_after_remote_overwrite`
    /// on first run; the spec adopts whichever value Loro produces, and this
    /// constant documents it. If a Loro bump changes it, update both.
    const REMOTE_OVERWRITE_UNDO_RESULT: i64 = 7;

    fn doc() -> (ListDocument, RowKey) {
        let key = RowKey::new(10, None);
        let doc = ListDocument::from_rows(
            MetaSnapshot {
                name: "t".into(),
                scope: Some(AnySelector::World(1)),
            },
            &[RowSnapshot {
                key,
                need: 1,
                acquired: 0,
                target: None,
            }],
        );
        (doc, key)
    }

    #[test]
    fn undo_and_redo_walk_need_edits() {
        let (doc, key) = doc();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        assert!(!undo.can_undo());
        doc.set_need(&key, 5).unwrap();
        doc.set_need(&key, 9).unwrap();
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key).unwrap().need, 5);
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key).unwrap().need, 1);
        assert!(!undo.undo().unwrap());
        assert!(undo.redo().unwrap());
        assert_eq!(doc.row(&key).unwrap().need, 5);
    }

    #[test]
    fn undo_of_a_removal_restores_every_field() {
        let (doc, key) = doc();
        doc.set_target(&key, Some(40)).unwrap();
        doc.add_acquired(&key, 1).unwrap();
        let before = doc.row(&key).unwrap();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        doc.remove_row(&key).unwrap();
        assert!(doc.row(&key).is_none());
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key), Some(before));
    }

    #[test]
    fn undo_of_an_add_removes_the_row() {
        let (doc, _) = doc();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        let new_key = RowKey::new(11, Some(true));
        doc.add_row(new_key, 2, None).unwrap();
        assert!(undo.undo().unwrap());
        assert!(doc.row(&new_key).is_none());
        assert!(undo.redo().unwrap());
        assert_eq!(doc.row(&new_key).unwrap().need, 2);
    }

    #[test]
    fn undo_of_a_counter_increment_subtracts_it() {
        let (doc, key) = doc();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        doc.add_acquired(&key, 3).unwrap();
        doc.add_acquired(&key, 2).unwrap();
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 3);
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key).unwrap().acquired, 0);
    }

    #[test]
    fn undo_of_a_quality_move_puts_the_row_back() {
        let (doc, key) = doc();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        let moved = doc.set_quality(&key, Quality::Hq).unwrap();
        assert!(undo.undo().unwrap());
        assert!(doc.row(&moved).is_none());
        assert_eq!(doc.row(&key).unwrap().need, 1);
    }

    #[test]
    fn a_group_undoes_as_one_step() {
        let (doc, key) = doc();
        let other = RowKey::new(11, None);
        doc.add_row(other, 1, None).unwrap();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        undo.group(|| {
            doc.set_quality(&key, Quality::Hq)?;
            doc.set_quality(&other, Quality::Hq)?;
            Ok(())
        })
        .unwrap();
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&key).unwrap().key.quality, Quality::Any);
        assert_eq!(doc.row(&other).unwrap().key.quality, Quality::Any);
        assert!(!undo.can_undo());
    }

    #[test]
    fn remote_imports_never_enter_the_stack() {
        let (a, key) = doc();
        let b = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
        let mut undo = ListUndo::with_merge_interval(&a, 0);
        b.set_need(&key, 8).unwrap();
        a.import(&b.export_since(&a.version()).unwrap()).unwrap();
        assert_eq!(a.row(&key).unwrap().need, 8);
        assert!(!undo.can_undo());
        assert!(!undo.undo().unwrap());
        assert_eq!(a.row(&key).unwrap().need, 8);
    }

    /// Pins how Loro resolves "undo my write that someone else overwrote".
    /// The one invariant that must hold either way: the undone value never
    /// comes back.
    #[test]
    fn undo_after_remote_overwrite() {
        let (a, key) = doc();
        let b = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
        let mut undo = ListUndo::with_merge_interval(&a, 0);
        a.set_need(&key, 5).unwrap();
        b.import(&a.export_since(&b.version()).unwrap()).unwrap();
        b.set_need(&key, 7).unwrap();
        a.import(&b.export_since(&a.version()).unwrap()).unwrap();
        assert_eq!(a.row(&key).unwrap().need, 7, "the remote write landed last");
        undo.undo().unwrap();
        let after = a.row(&key).unwrap().need;
        assert_ne!(after, 5, "undo must not resurrect the undone value");
        assert_eq!(
            after, REMOTE_OVERWRITE_UNDO_RESULT,
            "Loro's behaviour changed; update the constant and the spec's section 3.3 note"
        );
    }
}
```

- [ ] **Step 2: Register and re-export**

Add `pub mod undo;` and `pub use undo::ListUndo;` to `lib.rs`.

- [ ] **Step 3: Run the tests, then pin the overwrite result**

Run: `cargo test -p ultros-list-doc undo`
Expected: 7 of the 8 pass on the first run. If `undo_after_remote_overwrite` fails on the `REMOTE_OVERWRITE_UNDO_RESULT` assertion with `left: 1`, Loro restored this device's prior value over the remote write: change the constant to `1` and add this sentence to the constant's doc comment: "Loro restores the local prior value even over a later remote write." If it passes with `7`, add: "Loro yields to the later remote write; the undo is a no-op for that key." Either way, record the outcome in the spec, section 3.3, replacing "and this spec adopts whatever it is" with the sentence you added.

- [ ] **Step 4: Commit**

```bash
git add ultros-list-doc docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md
git commit -m "feat(list-doc): ListUndo over Loro's undo manager, remote-overwrite behaviour pinned

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Three-peer convergence property test

**Files:**
- Create: `ultros-list-doc/tests/convergence.rs`

**Interfaces:**
- Consumes: the whole public API from Tasks 1-6.

- [ ] **Step 1: Write the test**

```rust
//! Three peers apply seeded random edits and exchange updates in random
//! orders. After every round they must agree on rows and meta. No `rand`
//! dependency: a xorshift keeps the crate's dependency list to Loro.

use ultros_api_types::world_helper::AnySelector;
use ultros_list_doc::{ListDocument, MetaSnapshot, Quality, RowKey, RowSnapshot};

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const QUALITIES: [Quality; 3] = [Quality::Any, Quality::Hq, Quality::Nq];

fn seed_doc() -> ListDocument {
    ListDocument::from_rows(
        MetaSnapshot {
            name: "seed".to_string(),
            scope: Some(AnySelector::Datacenter(5)),
        },
        &[RowSnapshot {
            key: RowKey::new(10, None),
            need: 2,
            acquired: 0,
            target: None,
        }],
    )
}

fn random_op(doc: &ListDocument, rng: &mut XorShift) {
    let keys: Vec<RowKey> = doc.rows().into_iter().map(|r| r.key).collect();
    let pick = |rng: &mut XorShift| keys[rng.below(keys.len() as u64) as usize];
    match rng.below(8) {
        0 => {
            let key = RowKey {
                item_id: 10 + rng.below(6) as i32,
                quality: QUALITIES[rng.below(3) as usize],
            };
            doc.add_row(key, 1 + rng.below(5) as i64, None).unwrap();
        }
        1 if !keys.is_empty() => {
            let _ = doc.remove_row(&pick(rng));
        }
        2 if !keys.is_empty() => {
            let _ = doc.set_need(&pick(rng), rng.below(20) as i64);
        }
        3 if !keys.is_empty() => {
            let _ = doc.add_acquired(&pick(rng), 1 + rng.below(3) as i64);
        }
        4 if !keys.is_empty() => {
            let _ = doc.set_target(&pick(rng), Some(rng.below(1000) as i64));
        }
        5 if !keys.is_empty() => {
            let _ = doc.set_quality(&pick(rng), QUALITIES[rng.below(3) as usize]);
        }
        6 => doc.rename(&format!("name-{}", rng.below(100))).unwrap(),
        _ => doc
            .set_scope(AnySelector::World(rng.below(200) as i32))
            .unwrap(),
    }
}

#[test]
fn three_peers_converge_after_random_edits_in_every_order() {
    for seed in 1..=32u64 {
        let mut rng = XorShift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let snapshot = seed_doc().export_snapshot().unwrap();
        let peers: Vec<ListDocument> = (0..3)
            .map(|_| ListDocument::from_snapshot(&snapshot).unwrap())
            .collect();
        for round in 0..4 {
            for peer in &peers {
                for _ in 0..(1 + rng.below(4)) {
                    random_op(peer, &mut rng);
                }
            }
            let updates: Vec<Vec<u8>> = peers.iter().map(|p| p.export_all().unwrap()).collect();
            for peer in &peers {
                let mut order: Vec<usize> = (0..updates.len()).collect();
                for i in (1..order.len()).rev() {
                    let j = rng.below((i + 1) as u64) as usize;
                    order.swap(i, j);
                }
                for i in order {
                    peer.import(&updates[i]).unwrap();
                }
            }
            let rows = peers[0].rows();
            let meta = peers[0].meta();
            for (i, peer) in peers.iter().enumerate().skip(1) {
                assert_eq!(peer.rows(), rows, "seed {seed} round {round}: peer {i} rows diverged");
                assert_eq!(peer.meta(), meta, "seed {seed} round {round}: peer {i} meta diverged");
            }
        }
    }
}

#[test]
fn concurrent_acquired_ticks_both_count() {
    let key = RowKey::new(10, None);
    let a = seed_doc();
    let b = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
    a.add_acquired(&key, 2).unwrap();
    b.add_acquired(&key, 3).unwrap();
    a.import(&b.export_since(&a.version()).unwrap()).unwrap();
    b.import(&a.export_since(&b.version()).unwrap()).unwrap();
    assert_eq!(a.row(&key).unwrap().acquired, 5);
    assert_eq!(b.row(&key).unwrap().acquired, 5);
}

#[test]
fn a_concurrent_remove_wins_over_an_edit_inside_the_row() {
    let key = RowKey::new(10, None);
    let a = seed_doc();
    let b = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
    a.remove_row(&key).unwrap();
    b.set_need(&key, 50).unwrap();
    a.import(&b.export_since(&a.version()).unwrap()).unwrap();
    b.import(&a.export_since(&b.version()).unwrap()).unwrap();
    assert_eq!(a.rows(), b.rows());
    assert!(a.row(&key).is_none(), "spec section 1: remove wins");
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p ultros-list-doc --test convergence`
Expected: 3 passed. If `a_concurrent_remove_wins_over_an_edit_inside_the_row` fails because the row survives with `need: 50`, Loro's map resurrects a deleted container on a concurrent child edit; then change the assertion to `assert!(a.row(&key).is_some())`, and update spec section 1's bullet on removal to "an edit inside a row that someone concurrently removed brings the row back with that edit" so the spec states what the code does.

- [ ] **Step 3: Full CI check and commit**

Run: `./check_ci.sh > "$SCRATCHPAD/ci.log" 2>&1; echo "REAL_EXIT=$?"`
Expected: `REAL_EXIT=0`.

```bash
git add ultros-list-doc docs/superpowers/specs/2026-09-07-lists-local-first-sync-design.md
git commit -m "test(list-doc): three-peer convergence and concurrency semantics

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Self-review

- Spec coverage: section 1 schema and API (Tasks 3-5; the spec's API block already records the as-built signatures: no peer parameters, `Option<AnySelector>` scope, `Result` exports), section 2 peers (Task 3; the spec already states that peer ids are Loro's default per document), section 3.3 undo semantics and the pinned test (Task 6), section 8's crate-level tests: key round trips, every operation, `set_quality` carries fields, `add_row` merges, three-peer convergence in every order, `diff_rows` including counters, snapshot and shallow round trips, undo cases, remote imports never enter the stack (Tasks 2-7). Counter concurrency and remove-wins are pinned by Task 7.
- Placeholders: none. Two tests carry explicit "if this fails, do X" instructions with the exact edit to make.
- Type consistency: `ListDocument::inner() -> &LoroDoc` is what `ListUndo::with_merge_interval` consumes; `export_since(&[u8])` and `version() -> Vec<u8>` pair everywhere; `RowSnapshot` is `Copy` so `diff_rows` and tests copy it freely; `MetaSnapshot.scope` is `Option<AnySelector>` in Tasks 2, 3 and 7 alike.
