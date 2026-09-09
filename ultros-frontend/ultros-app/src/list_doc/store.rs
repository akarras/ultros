//! Browser persistence for list documents (spec section 3.1): one snapshot
//! per list under `ultros.listdoc.v1.{user_id}.{id}`, an index of last use
//! and last known permission scoped per user, and eviction beyond
//! `MAX_LISTS`.
//!
//! Snapshots are keyed by user id (in addition to list id) so a shared
//! browser never lets one account read another account's cached list.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

pub const MAX_LISTS: usize = 20;
const DOC_PREFIX: &str = "ultros.listdoc.v1.";
const INDEX_PREFIX: &str = "ultros.listdoc.index.v1.";

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
    /// Whether `ultros.listdoc.v1.{user}.{list}` actually holds a snapshot for
    /// this entry. `remember_permission` creates permission-only entries with
    /// this `false`; eviction must never treat those as LRU candidates, since
    /// there is no stored snapshot to make room for and nothing to remove.
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

/// Save a snapshot, touch the index, and evict the least recently used lists
/// beyond `MAX_LISTS` (per user). The document keeps working in memory when
/// this fails.
pub fn save(
    storage: &impl Storage,
    user_id: i64,
    list_id: i32,
    snapshot: &[u8],
    permission: i16,
    now_ms: f64,
) -> bool {
    if !storage.set(&doc_key(user_id, list_id), &STANDARD.encode(snapshot)) {
        return false;
    }
    let mut index = read_index(storage, user_id);
    index.lists.insert(
        list_id,
        IndexEntry {
            last_used_ms: now_ms,
            permission,
            has_snapshot: true,
        },
    );
    // Only entries backed by an actual stored snapshot count toward the cap
    // or are eligible for eviction: a permission-only entry from
    // `remember_permission` has no `doc_key` to remove and evicting it would
    // free nothing, so it must never be picked over a real cached list.
    while index.lists.values().filter(|e| e.has_snapshot).count() > MAX_LISTS {
        let Some((&oldest, _)) = index
            .lists
            .iter()
            .filter(|(_, e)| e.has_snapshot)
            .min_by(|a, b| a.1.last_used_ms.total_cmp(&b.1.last_used_ms))
        else {
            break;
        };
        index.lists.remove(&oldest);
        storage.remove(&doc_key(user_id, oldest));
    }
    write_index(storage, user_id, &index)
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
    // Do not bump `last_used_ms` on a snapshot-less entry: it must stay out
    // of eviction's LRU ordering (it is already excluded by `has_snapshot`,
    // but a stale/zero timestamp keeps the intent clear if that ever
    // changes) rather than accumulating a real recency the entry never earned.
    if entry.has_snapshot {
        entry.last_used_ms = now_ms;
    }
    write_index(storage, user_id, &index)
}

/// Drop one list's cached snapshot and index entry, e.g. once the server
/// says forbidden / not found / deleted.
pub fn purge(storage: &impl Storage, user_id: i64, list_id: i32) {
    storage.remove(&doc_key(user_id, list_id));
    let mut index = read_index(storage, user_id);
    if index.lists.remove(&list_id).is_some() {
        write_index(storage, user_id, &index);
    }
}

/// Drop every cached list for a user, e.g. on logout or account switch.
pub fn purge_user(storage: &impl Storage, user_id: i64) {
    let index = read_index(storage, user_id);
    for &list_id in index.lists.keys() {
        storage.remove(&doc_key(user_id, list_id));
    }
    storage.remove(&index_key(user_id));
}

pub struct BrowserStorage;

impl Storage for BrowserStorage {
    fn get(&self, key: &str) -> Option<String> {
        #[cfg(feature = "hydrate")]
        {
            web_sys::window()?
                .local_storage()
                .ok()??
                .get_item(key)
                .ok()?
        }
        #[cfg(not(feature = "hydrate"))]
        {
            let _ = key;
            None
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
    fn the_least_recently_used_list_is_evicted_past_the_cap() {
        let storage = MemoryStorage::default();
        for id in 1..=MAX_LISTS as i32 {
            assert!(save(&storage, 1, id, &[id as u8], 1, id as f64));
        }
        assert!(
            save(&storage, 1, 5, &[5], 1, 100.0),
            "list 5 becomes the newest"
        );
        assert!(save(&storage, 1, 99, &[99], 1, 101.0), "one past the cap");
        assert!(load(&storage, 1, 1).is_none(), "list 1 was the oldest");
        assert!(load(&storage, 1, 5).is_some());
        assert!(load(&storage, 1, 99).is_some());
        assert_eq!(read_index(&storage, 1).lists.len(), MAX_LISTS);
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
    fn purge_user_removes_all_docs_for_that_user_and_spares_others() {
        let storage = MemoryStorage::default();
        assert!(save(&storage, 1, 7, &[1], 1, 1000.0));
        assert!(save(&storage, 1, 8, &[2], 1, 1001.0));
        assert!(save(&storage, 2, 7, &[3], 1, 1002.0));
        purge_user(&storage, 1);
        assert!(load(&storage, 1, 7).is_none());
        assert!(load(&storage, 1, 8).is_none());
        assert!(read_index(&storage, 1).lists.is_empty());
        assert!(load(&storage, 2, 7).is_some());
    }

    #[test]
    fn eviction_skips_permission_only_entries_and_takes_the_oldest_saved_list() {
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
            load(&storage, 1, 1).is_none(),
            "list 1 was the oldest SAVED list, not the phantom entry"
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
            MAX_LISTS
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
    fn eviction_under_one_user_never_touches_another_users_entries() {
        let storage = MemoryStorage::default();
        for id in 1..=MAX_LISTS as i32 {
            assert!(save(&storage, 1, id, &[id as u8], 1, id as f64));
        }
        assert!(save(&storage, 2, 1, &[42], 1, 1.0));
        assert!(
            save(&storage, 1, 99, &[99], 1, 1000.0),
            "evicts user 1's oldest"
        );
        assert!(
            load(&storage, 1, 1).is_none(),
            "user 1's list 1 was evicted"
        );
        assert!(
            load(&storage, 2, 1).is_some(),
            "user 2's list 1 must be untouched"
        );
        assert_eq!(read_index(&storage, 1).lists.len(), MAX_LISTS);
        assert_eq!(read_index(&storage, 2).lists.len(), 1);
    }
}
