//! Browser persistence for list documents (spec section 3.1): one snapshot
//! per list under `ultros.listdoc.v1.{user_id}.{id}`, an index of last use
//! and last known permission scoped per user. Snapshots are primary offline
//! data: no automatic eviction is safe without a durable server acknowledgement.
//!
//! Snapshots are keyed by user id (in addition to list id) so a shared
//! browser never lets one account read another account's cached list.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

#[cfg(test)]
const MAX_LISTS: usize = 20;
const DOC_PREFIX: &str = "ultros.listdoc.v1.";
const INDEX_PREFIX: &str = "ultros.listdoc.index.v1.";
const GENERATION_PREFIX: &str = "ultros.listdoc.generation.v1.";

pub trait Storage {
    fn get(&self, key: &str) -> Option<String>;
    /// A failed read must not be mistaken for an absent snapshot during save.
    fn get_checked(&self, key: &str) -> Result<Option<String>, &'static str> {
        Ok(self.get(key))
    }
    /// False when the write did not stick (quota, private mode).
    fn set(&self, key: &str, value: &str) -> bool;
    fn remove(&self, key: &str);
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub last_used_ms: f64,
    pub permission: i16,
    /// Whether `ultros.listdoc.v1.{user}.{list}` actually holds a snapshot for
    /// this entry. `remember_permission` creates permission-only entries with
    /// this `false`; they do not represent locally saved document contents.
    #[serde(default)]
    pub has_snapshot: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub lists: BTreeMap<i32, IndexEntry>,
}

pub struct Loaded {
    pub snapshot: Vec<u8>,
    pub permission: i16,
}

fn doc_key(user_id: i64, list_id: i32) -> String {
    format!("{DOC_PREFIX}{user_id}.{list_id}")
}

fn index_key(user_id: i64) -> String {
    format!("{INDEX_PREFIX}{user_id}")
}

fn generation_key(user_id: i64, list_id: i32) -> String {
    format!("{GENERATION_PREFIX}{user_id}.{list_id}")
}

pub fn read_index(storage: &impl Storage, user_id: i64) -> Index {
    storage
        .get(&index_key(user_id))
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_index(storage: &impl Storage, user_id: i64, index: &Index) -> bool {
    serde_json::to_string(index)
        .map(|text| storage.set(&index_key(user_id), &text))
        .unwrap_or(false)
}

pub fn load(storage: &impl Storage, user_id: i64, list_id: i32) -> Option<Loaded> {
    let text = storage.get(&doc_key(user_id, list_id))?;
    let snapshot = STANDARD.decode(text).ok()?;
    let permission = read_index(storage, user_id)
        .lists
        .get(&list_id)
        .map(|entry| entry.permission)
        .unwrap_or(0);
    Some(Loaded {
        snapshot,
        permission,
    })
}

/// Save a snapshot and touch the index without evicting other documents.
/// Browser callers must serialize the complete read/merge/write transaction.
pub fn save(
    storage: &impl Storage,
    user_id: i64,
    list_id: i32,
    snapshot: &[u8],
    permission: i16,
    now_ms: f64,
) -> bool {
    let Ok(index_text) = storage.get_checked(&index_key(user_id)) else {
        return false;
    };
    let mut index: Index = index_text
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    if !storage.set(&doc_key(user_id, list_id), &STANDARD.encode(snapshot)) {
        return false;
    }
    index.lists.insert(
        list_id,
        IndexEntry {
            last_used_ms: now_ms,
            permission,
            has_snapshot: true,
        },
    );
    // Never evict primary offline data to make room for another list. A quota
    // failure is surfaced to the player with retry/export recovery instead.
    write_index(storage, user_id, &index)
}

/// Revocation generation. Queued saves captured before a purge cannot recreate
/// the discarded snapshot, even after their reactive owner has been disposed.
pub fn generation(storage: &impl Storage, user: i64, list: i32) -> String {
    storage.get(&generation_key(user, list)).unwrap_or_default()
}

/// Called under the browser's per-account Web Lock. Merge into a temporary
/// document first: a failed/parked import never overwrites the durable copy.
pub fn merge_save(
    storage: &impl Storage,
    user: i64,
    list: i32,
    snapshot: &[u8],
    permission: i16,
    expected_generation: &str,
) -> Result<Vec<u8>, String> {
    if generation(storage, user, list) != expected_generation {
        return Err("revoked".into());
    }
    let doc = ultros_list_doc::ListDocument::from_snapshot(snapshot).map_err(|_| "export")?;
    // Read the raw key so corrupt bytes fail safely instead of looking absent.
    if let Some(text) = storage.get_checked(&doc_key(user, list))? {
        let previous = STANDARD.decode(text).map_err(|_| "corrupt")?;
        ultros_list_doc::ListDocument::from_snapshot(&previous).map_err(|_| "corrupt")?;
        let report = doc.import(&previous).map_err(|_| "history")?;
        if report.pending {
            return Err("history".into());
        }
    }
    let merged = doc.export_snapshot().map_err(|_| "export")?;
    if !save(storage, user, list, &merged, permission, now_ms()) {
        return Err("storage".into());
    }
    // Purging is synchronous and can run in another tab while this tab owns
    // the save lock. Check again after writing before acknowledging durability.
    if generation(storage, user, list) != expected_generation {
        storage.remove(&doc_key(user, list));
        return Err("revoked".into());
    }
    Ok(merged)
}

/// Replace server-rejected history without importing it back from disk. Refuse
/// replacement if another tab persisted edits absent from the source document.
pub fn replace_save(
    storage: &impl Storage,
    user: i64,
    list: i32,
    snapshot: &[u8],
    permission: i16,
    expected_generation: &str,
    source_version: &[u8],
) -> (String, Result<Vec<u8>, String>) {
    let unchanged = || expected_generation.to_string();
    if generation(storage, user, list) != expected_generation {
        return (unchanged(), Err("revoked".into()));
    }
    let previous_text = match storage.get_checked(&doc_key(user, list)) {
        Ok(text) => text,
        Err(error) => return (unchanged(), Err(error.into())),
    };
    if let Some(text) = previous_text {
        let previous = STANDARD
            .decode(text)
            .ok()
            .and_then(|bytes| ultros_list_doc::ListDocument::from_snapshot(&bytes).ok());
        if previous.is_none_or(|doc| doc.is_ahead_of(source_version)) {
            return (unchanged(), Err("history".into()));
        }
    }
    let next = expected_generation
        .parse::<u64>()
        .unwrap_or(0)
        .wrapping_add(1)
        .to_string();
    if !storage.set(&generation_key(user, list), &next) {
        return (unchanged(), Err("storage".into()));
    }
    // Fence stale writers before writing. On failure the caller retains the
    // new token for retry; the previous snapshot is not deleted to free space.
    let result = if save(storage, user, list, snapshot, permission, now_ms()) {
        if generation(storage, user, list) == next {
            Ok(snapshot.to_vec())
        } else {
            storage.remove(&doc_key(user, list));
            Err("revoked".into())
        }
    } else {
        Err("storage".into())
    };
    (next, result)
}

pub fn remember_permission(
    storage: &impl Storage,
    user_id: i64,
    list_id: i32,
    permission: i16,
    now_ms: f64,
) -> bool {
    let mut index = read_index(storage, user_id);
    let entry = index.lists.entry(list_id).or_default();
    entry.permission = permission;
    // Only a backed entry has a meaningful snapshot access timestamp.
    if entry.has_snapshot {
        entry.last_used_ms = now_ms;
    }
    write_index(storage, user_id, &index)
}

/// Drop one list's cached snapshot and index entry, e.g. once the server
/// says forbidden / not found / deleted.
pub fn purge(storage: &impl Storage, user_id: i64, list_id: i32) {
    let token = generation(storage, user_id, list_id)
        .parse::<u64>()
        .unwrap_or(0)
        .wrapping_add(1);
    let token_key = generation_key(user_id, list_id);
    let fenced = storage.set(&token_key, &token.to_string());
    storage.remove(&doc_key(user_id, list_id));
    // A full origin may not have room for the tombstone until the revoked
    // snapshot is removed. Retry before any queued save can use that space.
    if !fenced {
        storage.set(&token_key, &token.to_string());
    }
    let mut index = read_index(storage, user_id);
    if index.lists.remove(&list_id).is_some() {
        write_index(storage, user_id, &index);
    }
}

pub struct BrowserStorage;

impl Storage for BrowserStorage {
    fn get(&self, key: &str) -> Option<String> {
        self.get_checked(key).ok().flatten()
    }

    fn get_checked(&self, key: &str) -> Result<Option<String>, &'static str> {
        #[cfg(feature = "hydrate")]
        {
            web_sys::window()
                .ok_or("storage")?
                .local_storage()
                .map_err(|_| "storage")?
                .ok_or("storage")?
                .get_item(key)
                .map_err(|_| "storage")
        }
        #[cfg(not(feature = "hydrate"))]
        {
            let _ = key;
            Err("storage")
        }
    }

    fn set(&self, key: &str, value: &str) -> bool {
        #[cfg(feature = "hydrate")]
        {
            web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
                .map(|s| s.set_item(key, value).is_ok())
                .unwrap_or(false)
        }
        #[cfg(not(feature = "hydrate"))]
        {
            let _ = (key, value);
            false
        }
    }

    fn remove(&self, key: &str) {
        #[cfg(feature = "hydrate")]
        {
            if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
                let _ = s.remove_item(key);
            }
        }
        #[cfg(not(feature = "hydrate"))]
        {
            let _ = key;
        }
    }
}

pub fn now_ms() -> f64 {
    #[cfg(feature = "hydrate")]
    {
        js_sys::Date::now()
    }
    #[cfg(not(feature = "hydrate"))]
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
            self.0
                .borrow_mut()
                .insert(key.to_string(), value.to_string());
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
        assert!(load(&storage, 1, 7).is_none());
        assert!(save(&storage, 1, 7, &[1, 2, 3], 2, 1000.0));
        let loaded = load(&storage, 1, 7).unwrap();
        assert_eq!(loaded.snapshot, vec![1, 2, 3]);
        assert_eq!(loaded.permission, 2);
        assert!(remember_permission(&storage, 1, 7, 3, 2000.0));
        assert_eq!(load(&storage, 1, 7).unwrap().permission, 3);
    }

    #[test]
    fn offline_snapshots_are_retained_past_the_old_cache_cap() {
        let storage = MemoryStorage::default();
        for id in 1..=MAX_LISTS as i32 {
            assert!(save(&storage, 1, id, &[id as u8], 1, id as f64));
        }
        assert!(
            save(&storage, 1, 5, &[5], 1, 100.0),
            "list 5 becomes the newest"
        );
        assert!(save(&storage, 1, 99, &[99], 1, 101.0), "one past the cap");
        assert!(load(&storage, 1, 1).is_some(), "list 1 remains protected");
        assert!(load(&storage, 1, 5).is_some());
        assert!(load(&storage, 1, 99).is_some());
        assert_eq!(read_index(&storage, 1).lists.len(), MAX_LISTS + 1);
    }

    #[test]
    fn a_full_store_reports_failure_without_panicking() {
        assert!(!save(&FullStorage, 1, 1, &[1], 1, 0.0));
        assert!(load(&FullStorage, 1, 1).is_none());
    }

    #[test]
    fn a_snapshot_saved_under_one_user_is_not_loadable_under_another() {
        let storage = MemoryStorage::default();
        assert!(save(&storage, 1, 7, &[9, 9], 2, 1000.0));
        assert!(load(&storage, 2, 7).is_none());
        assert!(load(&storage, 1, 7).is_some());
    }

    #[test]
    fn purge_removes_the_doc_and_index_entry() {
        let storage = MemoryStorage::default();
        assert!(save(&storage, 1, 7, &[1], 1, 1000.0));
        purge(&storage, 1, 7);
        assert!(load(&storage, 1, 7).is_none());
        assert!(!read_index(&storage, 1).lists.contains_key(&7));
    }

    #[test]
    fn permission_only_entries_do_not_displace_saved_work() {
        let storage = MemoryStorage::default();
        for id in 1..=MAX_LISTS as i32 {
            assert!(save(&storage, 1, id, &[id as u8], 1, id as f64));
        }
        assert!(
            remember_permission(&storage, 1, 21, 1, 9999.0),
            "a permission-only entry for a list never saved locally"
        );
        assert!(save(&storage, 1, 22, &[22], 1, 1000.0), "one past the cap");
        assert!(
            load(&storage, 1, 1).is_some(),
            "the oldest saved list remains protected"
        );
        assert!(
            read_index(&storage, 1).lists.contains_key(&21),
            "the permission-only phantom entry is never evicted in place of a real list"
        );
        assert!(load(&storage, 1, 22).is_some());
        assert_eq!(
            read_index(&storage, 1)
                .lists
                .values()
                .filter(|e| e.has_snapshot)
                .count(),
            MAX_LISTS + 1
        );
    }

    #[test]
    fn legacy_index_entries_do_not_make_snapshots_evictable() {
        let storage = MemoryStorage::default();
        for id in 1..=MAX_LISTS as i32 {
            assert!(save(&storage, 1, id, &[id as u8], 1, id as f64));
        }
        // Simulate an index entry written before `has_snapshot` existed: the
        // flag defaults to `false`, but the blob under `doc_key` is untouched.
        let mut index = read_index(&storage, 1);
        index.lists.get_mut(&1).unwrap().has_snapshot = false;
        assert!(write_index(&storage, 1, &index));
        assert!(save(&storage, 1, 99, &[99], 1, 1000.0), "one past the cap");
        assert!(
            load(&storage, 1, 1).is_some(),
            "legacy snapshot remains protected"
        );
    }

    #[test]
    fn remember_permission_does_not_bump_last_used_without_a_snapshot() {
        let storage = MemoryStorage::default();
        assert!(remember_permission(&storage, 1, 7, 2, 500.0));
        let entry = read_index(&storage, 1).lists.get(&7).cloned().unwrap();
        assert_eq!(entry.last_used_ms, 0.0);
        assert!(!entry.has_snapshot);
        assert_eq!(entry.permission, 2);
    }

    #[test]
    fn garbage_index_json_degrades_to_empty() {
        let storage = MemoryStorage::default();
        storage.set(&index_key(1), "not json");
        assert!(read_index(&storage, 1).lists.is_empty());
    }

    #[test]
    fn saving_under_one_user_never_touches_another_users_entries() {
        let storage = MemoryStorage::default();
        for id in 1..=MAX_LISTS as i32 {
            assert!(save(&storage, 1, id, &[id as u8], 1, id as f64));
        }
        assert!(save(&storage, 2, 1, &[42], 1, 1.0));
        assert!(
            save(&storage, 1, 99, &[99], 1, 1000.0),
            "adds another list without eviction"
        );
        assert!(
            load(&storage, 1, 1).is_some(),
            "user 1's list 1 remains protected"
        );
        assert!(
            load(&storage, 2, 1).is_some(),
            "user 2's list 1 must be untouched"
        );
        assert_eq!(read_index(&storage, 1).lists.len(), MAX_LISTS + 1);
        assert_eq!(read_index(&storage, 2).lists.len(), 1);
    }
    fn snapshot(item: i32) -> Vec<u8> {
        let doc = ultros_list_doc::ListDocument::new();
        doc.add_row(ultros_list_doc::RowKey::new(item, None), 1, None)
            .unwrap();
        doc.export_snapshot().unwrap()
    }

    #[test]
    fn divergent_tabs_merge_and_a_stale_close_cannot_erase_either_edit() {
        let storage = MemoryStorage::default();
        let base = snapshot(99);
        let first = ultros_list_doc::ListDocument::from_snapshot(&base).unwrap();
        let second = ultros_list_doc::ListDocument::from_snapshot(&base).unwrap();
        let mut undo = ultros_list_doc::ListUndo::new(&first);
        first
            .add_row(ultros_list_doc::RowKey::new(10, None), 1, None)
            .unwrap();
        second
            .add_row(ultros_list_doc::RowKey::new(20, None), 1, None)
            .unwrap();
        let a = first.export_snapshot().unwrap();
        let b = second.export_snapshot().unwrap();
        merge_save(&storage, 1, 7, &a, 2, "").unwrap();
        merge_save(&storage, 1, 7, &b, 2, "").unwrap();
        let merged = merge_save(&storage, 1, 7, &a, 2, "").unwrap();
        let doc = ultros_list_doc::ListDocument::from_snapshot(&merged).unwrap();
        assert_eq!(doc.rows().len(), 3);
        assert_eq!(
            ultros_list_doc::ListDocument::from_snapshot(&load(&storage, 1, 7).unwrap().snapshot)
                .unwrap()
                .rows(),
            doc.rows()
        );
        first.import(&merged).unwrap();
        assert!(undo.undo().unwrap());
        assert!(first.row(&ultros_list_doc::RowKey::new(10, None)).is_none());
        assert!(
            first.row(&ultros_list_doc::RowKey::new(20, None)).is_some(),
            "peer edits do not enter local undo"
        );
    }

    #[test]
    fn export_or_import_failure_preserves_the_existing_snapshot() {
        let storage = MemoryStorage::default();
        let a = snapshot(10);
        merge_save(&storage, 1, 7, &a, 2, "").unwrap();
        let saved = load(&storage, 1, 7).unwrap().snapshot;
        assert!(merge_save(&storage, 1, 7, b"invalid", 2, "").is_err());
        assert_eq!(load(&storage, 1, 7).unwrap().snapshot, saved);
        storage.set(&doc_key(1, 7), "bad base64");
        assert!(merge_save(&storage, 1, 7, &a, 2, "").is_err());
        assert_eq!(storage.get(&doc_key(1, 7)).unwrap(), "bad base64");
    }

    #[test]
    fn unsupported_schema_preserves_both_existing_bytes_and_index() {
        let storage = MemoryStorage::default();
        let supported = snapshot(10);
        let future = ultros_list_doc::ListDocument::from_snapshot(&supported).unwrap();
        future
            .inner()
            .get_map("meta")
            .insert("schema", 999_i64)
            .unwrap();
        future.commit();
        let future = future.export_snapshot().unwrap();
        save(&storage, 1, 7, &supported, 2, 123.0);
        let before = storage.0.borrow().clone();
        assert!(merge_save(&storage, 1, 7, &future, 3, "").is_err());
        assert_eq!(*storage.0.borrow(), before);
        save(&storage, 1, 7, &future, 2, 456.0);
        let before = storage.0.borrow().clone();
        assert!(merge_save(&storage, 1, 7, &supported, 3, "").is_err());
        assert_eq!(*storage.0.borrow(), before);
    }

    #[test]
    fn failed_reads_never_overwrite_an_existing_snapshot() {
        struct ReadFailure(MemoryStorage);
        impl Storage for ReadFailure {
            fn get(&self, key: &str) -> Option<String> {
                self.0.get(key)
            }
            fn get_checked(&self, _key: &str) -> Result<Option<String>, &'static str> {
                Err("storage")
            }
            fn set(&self, key: &str, value: &str) -> bool {
                self.0.set(key, value)
            }
            fn remove(&self, key: &str) {
                self.0.remove(key);
            }
        }
        let storage = ReadFailure(MemoryStorage::default());
        let original = snapshot(10);
        assert!(save(&storage.0, 1, 7, &original, 2, 0.0));
        assert!(merge_save(&storage, 1, 7, &snapshot(20), 2, "").is_err());
        assert_eq!(load(&storage.0, 1, 7).unwrap().snapshot, original);
    }

    #[test]
    fn purge_invalidates_a_queued_save_and_new_open_uses_new_generation() {
        let storage = MemoryStorage::default();
        let a = snapshot(10);
        merge_save(&storage, 1, 7, &a, 2, "").unwrap();
        purge(&storage, 1, 7);
        assert!(merge_save(&storage, 1, 7, &a, 2, "").is_err());
        assert!(load(&storage, 1, 7).is_none());
        let token = generation(&storage, 1, 7);
        assert!(merge_save(&storage, 1, 7, &a, 2, &token).is_ok());
    }

    #[test]
    fn purge_fences_queued_writes_when_the_snapshot_must_free_tombstone_space() {
        struct QuotaStorage(MemoryStorage);
        impl Storage for QuotaStorage {
            fn get(&self, key: &str) -> Option<String> {
                self.0.get(key)
            }
            fn set(&self, key: &str, value: &str) -> bool {
                if key.starts_with(GENERATION_PREFIX) && self.0.get(&doc_key(1, 7)).is_some() {
                    false
                } else {
                    self.0.set(key, value)
                }
            }
            fn remove(&self, key: &str) {
                self.0.remove(key);
            }
        }
        let storage = QuotaStorage(MemoryStorage::default());
        let snapshot = snapshot(10);
        merge_save(&storage, 1, 7, &snapshot, 2, "").unwrap();
        purge(&storage, 1, 7);
        assert!(merge_save(&storage, 1, 7, &snapshot, 2, "").is_err());
        assert!(load(&storage, 1, 7).is_none());
    }

    #[test]
    fn index_failure_is_reported_and_retry_recovers_without_losing_snapshot() {
        struct IndexFailure {
            inner: MemoryStorage,
            fail: std::cell::Cell<bool>,
        }
        impl Storage for IndexFailure {
            fn get(&self, key: &str) -> Option<String> {
                self.inner.get(key)
            }
            fn set(&self, key: &str, value: &str) -> bool {
                if key.starts_with(INDEX_PREFIX) && self.fail.get() {
                    false
                } else {
                    self.inner.set(key, value)
                }
            }
            fn remove(&self, key: &str) {
                self.inner.remove(key);
            }
        }
        let storage = IndexFailure {
            inner: MemoryStorage::default(),
            fail: std::cell::Cell::new(true),
        };
        let a = snapshot(10);
        assert!(merge_save(&storage, 1, 7, &a, 2, "").is_err());
        assert!(load(&storage, 1, 7).is_some());
        storage.fail.set(false);
        assert!(merge_save(&storage, 1, 7, &a, 2, "").is_ok());
        assert_eq!(load(&storage, 1, 7).unwrap().permission, 2);
        assert!(merge_save(&FullStorage, 1, 7, &a, 2, "").is_err());
    }

    #[test]
    fn replacement_does_not_reimport_server_rejected_metadata() {
        use ultros_list_doc::ListDocument;
        let storage = MemoryStorage::default();
        let base = ListDocument::new();
        base.rename("server name").unwrap();
        let original = base.export_snapshot().unwrap();
        let old = ListDocument::from_snapshot(&original).unwrap();
        old.rename("rejected name").unwrap();
        let stale = old.export_snapshot().unwrap();
        merge_save(&storage, 1, 7, &stale, 2, "").unwrap();
        let (token, result) = replace_save(&storage, 1, 7, &original, 2, "", &old.version());
        result.unwrap();
        assert_ne!(token, "");
        assert!(merge_save(&storage, 1, 7, &stale, 2, "").is_err());
        assert_eq!(
            ListDocument::from_snapshot(&load(&storage, 1, 7).unwrap().snapshot)
                .unwrap()
                .meta()
                .name,
            "server name"
        );
    }

    #[test]
    fn replacement_refuses_to_discard_another_tabs_newer_durable_edit() {
        use ultros_list_doc::{ListDocument, RowKey};
        let storage = MemoryStorage::default();
        let original = snapshot(10);
        let source = ListDocument::from_snapshot(&original).unwrap();
        let other = ListDocument::from_snapshot(&original).unwrap();
        other.add_row(RowKey::new(20, None), 1, None).unwrap();
        merge_save(&storage, 1, 7, &other.export_snapshot().unwrap(), 2, "").unwrap();
        let (token, result) = replace_save(&storage, 1, 7, &original, 2, "", &source.version());
        assert!(result.is_err());
        assert_eq!(token, "");
        assert!(
            ListDocument::from_snapshot(&load(&storage, 1, 7).unwrap().snapshot)
                .unwrap()
                .row(&RowKey::new(20, None))
                .is_some()
        );
    }
}
