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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    readiness: Option<ReadinessStage>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub lists: BTreeMap<i32, IndexEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ReadinessProof {
    version: String,
    generations: Vec<String>,
    ready: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ReadinessStage {
    incoming: ReadinessProof,
    previous: Option<ReadinessProof>,
}

pub struct Loaded {
    pub snapshot: Vec<u8>,
    pub permission: i16,
    /// None denotes legacy provenance; explicit false forbids causal fallback.
    pub content_ready: Option<bool>,
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

#[cfg(test)]
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
    let index = checked_index(storage, user_id);
    let permission = index
        .as_ref()
        .ok()
        .and_then(|index| index.lists.get(&list_id))
        .map(|entry| entry.permission)
        .unwrap_or(0);
    let content_ready = match (
        &index,
        ultros_list_doc::ListDocument::from_snapshot(&snapshot),
    ) {
        (Ok(index), Ok(doc)) => checked_generation(storage, user_id, list_id)
            .map(|generation| readiness(index, list_id, &doc, &generation))
            .unwrap_or(Some(false)),
        _ => Some(false),
    };
    Some(Loaded {
        snapshot,
        permission,
        content_ready,
    })
}

/// Save a snapshot and touch the index without evicting other documents.
/// Browser callers must serialize the complete read/merge/write transaction.
#[cfg(test)]
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
            readiness: None,
        },
    );
    // Never evict primary offline data to make room for another list. A quota
    // failure is surfaced to the player with retry/export recovery instead.
    write_index(storage, user_id, &index)
}

fn checked_index(storage: &impl Storage, user: i64) -> Result<Index, &'static str> {
    storage
        .get_checked(&index_key(user))?
        .map(|text| serde_json::from_str(&text).map_err(|_| "corrupt"))
        .unwrap_or_else(|| Ok(Index::default()))
}

pub fn causal_content_ready(doc: &ultros_list_doc::ListDocument) -> bool {
    doc.inner()
        .oplog_vv()
        .iter()
        .any(|(_, counter)| *counter > 0)
}

fn readiness(
    index: &Index,
    list: i32,
    doc: &ultros_list_doc::ListDocument,
    generation: &str,
) -> Option<bool> {
    let stage = index.lists.get(&list)?.readiness.as_ref()?;
    Some(
        std::iter::once(&stage.incoming)
            .chain(stage.previous.iter())
            .find(|proof| {
                proof
                    .generations
                    .iter()
                    .any(|candidate| candidate == generation)
                    && STANDARD.decode(&proof.version).is_ok_and(|version| {
                        matches!(
                            doc.sync_payload(&version),
                            Ok(ultros_list_doc::SyncPayload::UpToDate)
                        )
                    })
            })
            .is_some_and(|proof| proof.ready),
    )
}

/// Both proofs precede new bytes: failed writes preserve the previous offline
/// copy, and failed finalization leaves a bounded proof for the new copy.
fn save_ready(
    storage: &impl Storage,
    user: i64,
    list: i32,
    snapshot: (&[u8], bool),
    permission: i16,
    generations: (&str, &str),
) -> Result<(), String> {
    let (bytes, ready) = snapshot;
    let (before, after) = generations;
    let doc = ultros_list_doc::ListDocument::from_snapshot(bytes).map_err(|_| "export")?;
    let mut index = checked_index(storage, user)?;
    let previous = storage
        .get_checked(&doc_key(user, list))?
        .map(|text| {
            let bytes = STANDARD.decode(text).map_err(|_| "corrupt")?;
            let doc =
                ultros_list_doc::ListDocument::from_snapshot(&bytes).map_err(|_| "corrupt")?;
            let ready =
                readiness(&index, list, &doc, before).unwrap_or_else(|| causal_content_ready(&doc));
            Ok::<_, &'static str>(ReadinessProof {
                version: STANDARD.encode(doc.version()),
                generations: if before == after {
                    vec![before.to_string()]
                } else {
                    vec![before.to_string(), after.to_string()]
                },
                ready,
            })
        })
        .transpose()?;
    if checked_generation(storage, user, list)? != before {
        return Err("revoked".into());
    }
    let incoming = ReadinessProof {
        version: STANDARD.encode(doc.version()),
        generations: vec![after.to_string()],
        ready,
    };
    index.lists.insert(
        list,
        IndexEntry {
            last_used_ms: now_ms(),
            permission,
            has_snapshot: true,
            readiness: Some(ReadinessStage {
                incoming: incoming.clone(),
                previous,
            }),
        },
    );
    if !write_index(storage, user, &index) {
        return Err("storage".into());
    }
    if checked_generation(storage, user, list)? != before {
        return Err("revoked".into());
    }
    if before != after && !storage.set(&generation_key(user, list), after) {
        return Err("storage".into());
    }
    if checked_generation(storage, user, list)? != after {
        return Err("revoked".into());
    }
    if !storage.set(&doc_key(user, list), &STANDARD.encode(bytes)) {
        return Err("storage".into());
    }
    if checked_generation(storage, user, list).as_deref() != Ok(after) {
        storage.remove(&doc_key(user, list));
        return Err("revoked".into());
    }
    // A failed final index write leaves the staged incoming+previous pair.
    if let Some(entry) = index.lists.get_mut(&list) {
        entry.readiness = Some(ReadinessStage {
            incoming,
            previous: None,
        });
    }
    if !write_index(storage, user, &index) {
        return Err("storage".into());
    }
    if checked_generation(storage, user, list).as_deref() != Ok(after) {
        storage.remove(&doc_key(user, list));
        return Err("revoked".into());
    }
    Ok(())
}

pub fn checked_generation(
    storage: &impl Storage,
    user: i64,
    list: i32,
) -> Result<String, &'static str> {
    Ok(storage
        .get_checked(&generation_key(user, list))?
        .unwrap_or_default())
}

/// Revocation generation. Queued saves captured before a purge cannot recreate
/// the discarded snapshot, even after their reactive owner has been disposed.
pub fn generation(storage: &impl Storage, user: i64, list: i32) -> String {
    storage.get(&generation_key(user, list)).unwrap_or_default()
}

/// Called under the browser's per-account Web Lock. Merge into a temporary
/// document first: a failed/parked import never overwrites the durable copy.
pub fn merge_save_with_readiness(
    storage: &impl Storage,
    user: i64,
    list: i32,
    snapshot: (&[u8], bool),
    permission: i16,
    expected_generation: &str,
) -> Result<Vec<u8>, String> {
    let (snapshot, mut ready) = snapshot;
    if checked_generation(storage, user, list)? != expected_generation {
        return Err("revoked".into());
    }
    let doc = ultros_list_doc::ListDocument::from_snapshot(snapshot).map_err(|_| "export")?;
    // Read the raw key so corrupt bytes fail safely instead of looking absent.
    if let Some(text) = storage.get_checked(&doc_key(user, list))? {
        let previous = STANDARD.decode(text).map_err(|_| "corrupt")?;
        let previous_doc =
            ultros_list_doc::ListDocument::from_snapshot(&previous).map_err(|_| "corrupt")?;
        let previous_ready = readiness(
            &checked_index(storage, user)?,
            list,
            &previous_doc,
            expected_generation,
        )
        .unwrap_or_else(|| causal_content_ready(&previous_doc));
        let report = doc.import(&previous).map_err(|_| "history")?;
        if report.pending {
            return Err("history".into());
        }
        ready |= previous_ready;
    }
    let merged = doc.export_snapshot().map_err(|_| "export")?;
    save_ready(
        storage,
        user,
        list,
        (&merged, ready),
        permission,
        (expected_generation, expected_generation),
    )?;
    // Purging is synchronous and can run in another tab while this tab owns
    // the save lock. Check again after writing before acknowledging durability.
    if checked_generation(storage, user, list).as_deref() != Ok(expected_generation) {
        storage.remove(&doc_key(user, list));
        return Err("revoked".into());
    }
    Ok(merged)
}

/// Replace server-rejected history without importing it back from disk. Refuse
/// replacement if another tab persisted edits absent from the source document.
pub fn replace_save_with_readiness(
    storage: &impl Storage,
    user: i64,
    list: i32,
    snapshot: (&[u8], bool),
    permission: i16,
    expected_generation: &str,
    source_version: &[u8],
) -> (String, Result<Vec<u8>, String>) {
    let unchanged = || expected_generation.to_string();
    if checked_generation(storage, user, list).as_deref() != Ok(expected_generation) {
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
    let result = save_ready(
        storage,
        user,
        list,
        snapshot,
        permission,
        (expected_generation, &next),
    )
    .map(|()| snapshot.0.to_vec());
    (
        checked_generation(storage, user, list).unwrap_or_else(|_| unchanged()),
        result,
    )
}

pub fn remember_permission(
    storage: &impl Storage,
    user_id: i64,
    list_id: i32,
    permission: i16,
    now_ms: f64,
) -> bool {
    let Ok(mut index) = checked_index(storage, user_id) else {
        return false;
    };
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
pub fn purge_snapshot(storage: &impl Storage, user_id: i64, list_id: i32) {
    let Ok(current) = checked_generation(storage, user_id, list_id) else {
        storage.remove(&doc_key(user_id, list_id));
        return;
    };
    let token = current.parse::<u64>().unwrap_or(0).wrapping_add(1);
    let token_key = generation_key(user_id, list_id);
    let fenced = storage.set(&token_key, &token.to_string());
    storage.remove(&doc_key(user_id, list_id));
    // A full origin may not have room for the tombstone until the revoked
    // snapshot is removed. Retry before any queued save can use that space.
    if !fenced {
        storage.set(&token_key, &token.to_string());
    }
}

/// Index cleanup shares the account lock with saves and permission updates.
/// Never remove a newly saved copy opened after the synchronous revocation.
pub fn purge_index(storage: &impl Storage, user_id: i64, list_id: i32) -> bool {
    if !matches!(storage.get_checked(&doc_key(user_id, list_id)), Ok(None)) {
        return false;
    }
    let Ok(mut index) = checked_index(storage, user_id) else {
        return false;
    };
    index.lists.remove(&list_id);
    write_index(storage, user_id, &index)
}

#[cfg(test)]
fn purge(storage: &impl Storage, user_id: i64, list_id: i32) {
    purge_snapshot(storage, user_id, list_id);
    purge_index(storage, user_id, list_id);
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
fn merge_save(
    storage: &impl Storage,
    user: i64,
    list: i32,
    snapshot: &[u8],
    permission: i16,
    generation: &str,
) -> Result<Vec<u8>, String> {
    let ready = ultros_list_doc::ListDocument::from_snapshot(snapshot)
        .is_ok_and(|doc| causal_content_ready(&doc));
    merge_save_with_readiness(
        storage,
        user,
        list,
        (snapshot, ready),
        permission,
        generation,
    )
}

#[cfg(test)]
fn replace_save(
    storage: &impl Storage,
    user: i64,
    list: i32,
    snapshot: &[u8],
    permission: i16,
    generation: &str,
    source: &[u8],
) -> (String, Result<Vec<u8>, String>) {
    let ready = ultros_list_doc::ListDocument::from_snapshot(snapshot)
        .is_ok_and(|doc| causal_content_ready(&doc));
    replace_save_with_readiness(
        storage,
        user,
        list,
        (snapshot, ready),
        permission,
        generation,
        source,
    )
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

    /// Fail exactly one write in a transaction, keeping all previously durable keys.
    struct WriteFailure {
        inner: MemoryStorage,
        at: std::cell::Cell<usize>,
        writes: std::cell::Cell<usize>,
    }
    impl WriteFailure {
        fn new() -> Self {
            Self {
                inner: MemoryStorage::default(),
                at: std::cell::Cell::new(usize::MAX),
                writes: std::cell::Cell::new(0),
            }
        }
        fn fail_at(&self, at: usize) {
            self.writes.set(0);
            self.at.set(at);
        }
    }
    impl Storage for WriteFailure {
        fn get(&self, key: &str) -> Option<String> {
            self.inner.get(key)
        }
        fn set(&self, key: &str, value: &str) -> bool {
            let writes = self.writes.get() + 1;
            self.writes.set(writes);
            writes != self.at.get() && self.inner.set(key, value)
        }
        fn remove(&self, key: &str) {
            self.inner.remove(key);
        }
    }

    #[test]
    fn readiness_proof_preserves_confirmed_empty_and_unconfirmed_nonempty() {
        let storage = MemoryStorage::default();
        let empty = ultros_list_doc::ListDocument::empty_peer()
            .export_snapshot()
            .unwrap();
        merge_save_with_readiness(&storage, 1, 7, (&empty, true), 3, "").unwrap();
        assert_eq!(load(&storage, 1, 7).unwrap().content_ready, Some(true));
        let partial = snapshot(5056);
        merge_save_with_readiness(&storage, 1, 8, (&partial, false), 3, "").unwrap();
        assert_eq!(load(&storage, 1, 8).unwrap().content_ready, Some(false));
        assert!(remember_permission(&storage, 1, 8, 3, 100.0));
        assert_eq!(load(&storage, 1, 8).unwrap().content_ready, Some(false));
        // A stale unready save can merge complete contents, but cannot downgrade them.
        merge_save_with_readiness(&storage, 1, 7, (&partial, false), 3, "").unwrap();
        assert_eq!(load(&storage, 1, 7).unwrap().content_ready, Some(true));
    }

    #[test]
    fn every_first_save_failure_keeps_unconfirmed_bytes_unreadable() {
        for fail_at in 1..=3 {
            let storage = WriteFailure::new();
            storage.fail_at(fail_at);
            assert!(
                merge_save_with_readiness(&storage, 1, 7, (&snapshot(5056), false), 3, "").is_err()
            );
            match load(&storage, 1, 7) {
                None => assert!(fail_at <= 2),
                Some(loaded) => assert_eq!(loaded.content_ready, Some(false)),
            }
        }
    }

    #[test]
    fn every_merge_write_failure_keeps_a_readable_durable_copy() {
        for fail_at in 1..=3 {
            let storage = WriteFailure::new();
            let base = snapshot(5056);
            merge_save_with_readiness(&storage, 1, 7, (&base, true), 3, "").unwrap();
            let previous = load(&storage, 1, 7).unwrap().snapshot;
            let edited = ultros_list_doc::ListDocument::from_snapshot(&previous).unwrap();
            edited
                .add_row(ultros_list_doc::RowKey::new(5057, None), 2, None)
                .unwrap();
            storage.fail_at(fail_at);
            assert!(
                merge_save_with_readiness(
                    &storage,
                    1,
                    7,
                    (&edited.export_snapshot().unwrap(), true),
                    3,
                    ""
                )
                .is_err()
            );
            let loaded = load(&storage, 1, 7).unwrap();
            assert_eq!(loaded.content_ready, Some(true), "write {fail_at}");
            if fail_at <= 2 {
                assert_eq!(loaded.snapshot, previous);
            }
            let rows = ultros_list_doc::ListDocument::from_snapshot(&loaded.snapshot)
                .unwrap()
                .rows();
            assert_eq!(rows.len(), if fail_at <= 2 { 1 } else { 2 });
        }
    }

    #[test]
    fn every_replacement_write_failure_preserves_old_proof_without_blessing_new_bytes() {
        for fail_at in 1..=4 {
            let storage = WriteFailure::new();
            let original = snapshot(5056);
            merge_save_with_readiness(&storage, 1, 7, (&original, true), 3, "").unwrap();
            let old = load(&storage, 1, 7).unwrap().snapshot;
            let source = ultros_list_doc::ListDocument::from_snapshot(&old)
                .unwrap()
                .version();
            storage.fail_at(fail_at);
            let (generation, result) = replace_save_with_readiness(
                &storage,
                1,
                7,
                (&snapshot(5057), false),
                3,
                "",
                &source,
            );
            assert!(result.is_err());
            assert_eq!(generation, if fail_at <= 2 { "" } else { "1" });
            let loaded = load(&storage, 1, 7).unwrap();
            assert_eq!(loaded.content_ready, Some(fail_at <= 3), "write {fail_at}");
            if fail_at <= 3 {
                assert_eq!(loaded.snapshot, old);
            }
        }
    }

    #[test]
    fn retained_proofs_cannot_bless_different_versions_or_revoked_generations() {
        let storage = MemoryStorage::default();
        let original = snapshot(5056);
        merge_save_with_readiness(&storage, 1, 7, (&original, true), 3, "").unwrap();
        let index = storage.get(&index_key(1)).unwrap();
        // Simulate bytes succeeding with a stale index; explicit mismatch is not legacy.
        storage.set(&doc_key(1, 7), &STANDARD.encode(snapshot(5057)));
        assert_eq!(load(&storage, 1, 7).unwrap().content_ready, Some(false));
        purge(&storage, 1, 7);
        storage.set(&index_key(1), &index);
        storage.set(&doc_key(1, 7), &STANDARD.encode(original));
        assert_eq!(load(&storage, 1, 7).unwrap().content_ready, Some(false));
        // A permission update must preserve the mismatched proof, not erase it.
        remember_permission(&storage, 1, 7, 3, 1.0);
        assert_eq!(load(&storage, 1, 7).unwrap().content_ready, Some(false));
    }

    #[test]
    fn unreadable_generation_never_uses_the_initial_generation_proof() {
        struct GenerationFailure(MemoryStorage);
        impl Storage for GenerationFailure {
            fn get(&self, key: &str) -> Option<String> {
                self.get_checked(key).ok().flatten()
            }
            fn get_checked(&self, key: &str) -> Result<Option<String>, &'static str> {
                if key.starts_with(GENERATION_PREFIX) {
                    Err("storage")
                } else {
                    Ok(self.0.get(key))
                }
            }
            fn set(&self, key: &str, value: &str) -> bool {
                self.0.set(key, value)
            }
            fn remove(&self, key: &str) {
                self.0.remove(key);
            }
        }
        let storage = GenerationFailure(MemoryStorage::default());
        let bytes = snapshot(5056);
        merge_save_with_readiness(&storage.0, 1, 7, (&bytes, true), 3, "").unwrap();
        let before = storage.0.0.borrow().clone();
        assert_eq!(load(&storage, 1, 7).unwrap().content_ready, Some(false));
        assert!(merge_save_with_readiness(&storage, 1, 7, (&bytes, true), 3, "").is_err());
        let source = ultros_list_doc::ListDocument::from_snapshot(&bytes)
            .unwrap()
            .version();
        assert!(
            replace_save_with_readiness(&storage, 1, 7, (&bytes, true), 3, "", &source)
                .1
                .is_err()
        );
        assert_eq!(*storage.0.0.borrow(), before);
    }

    #[test]
    fn failed_generation_read_after_snapshot_write_removes_unacknowledged_bytes() {
        struct PostWriteFailure {
            inner: MemoryStorage,
            wrote: std::cell::Cell<bool>,
        }
        impl Storage for PostWriteFailure {
            fn get(&self, key: &str) -> Option<String> {
                self.get_checked(key).ok().flatten()
            }
            fn get_checked(&self, key: &str) -> Result<Option<String>, &'static str> {
                if key.starts_with(GENERATION_PREFIX) && self.wrote.get() {
                    Err("storage")
                } else {
                    Ok(self.inner.get(key))
                }
            }
            fn set(&self, key: &str, value: &str) -> bool {
                let saved = self.inner.set(key, value);
                if key.starts_with(DOC_PREFIX) {
                    self.wrote.set(true);
                }
                saved
            }
            fn remove(&self, key: &str) {
                self.inner.remove(key);
            }
        }
        let storage = PostWriteFailure {
            inner: MemoryStorage::default(),
            wrote: std::cell::Cell::new(false),
        };
        assert!(merge_save_with_readiness(&storage, 1, 7, (&snapshot(5056), true), 3, "").is_err());
        assert!(storage.inner.get(&doc_key(1, 7)).is_none());
    }

    #[test]
    fn index_cleanup_preserves_other_proofs_and_a_reopened_copy() {
        let storage = MemoryStorage::default();
        let bytes = snapshot(5056);
        merge_save_with_readiness(&storage, 1, 7, (&bytes, true), 3, "").unwrap();
        merge_save_with_readiness(&storage, 1, 8, (&bytes, false), 3, "").unwrap();
        purge_snapshot(&storage, 1, 7);
        assert!(purge_index(&storage, 1, 7));
        assert_eq!(load(&storage, 1, 8).unwrap().content_ready, Some(false));
        merge_save_with_readiness(&storage, 1, 7, (&bytes, true), 3, "1").unwrap();
        assert!(!purge_index(&storage, 1, 7));
        assert_eq!(load(&storage, 1, 7).unwrap().content_ready, Some(true));
        storage.set(&index_key(1), "corrupt index");
        purge_snapshot(&storage, 1, 7);
        assert!(!purge_index(&storage, 1, 7));
        assert!(!remember_permission(&storage, 1, 8, 3, 0.0));
        assert_eq!(storage.get(&index_key(1)).as_deref(), Some("corrupt index"));
        assert_eq!(load(&storage, 1, 8).unwrap().content_ready, Some(false));
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
        // Failed proof staging must not expose new bytes without provenance.
        assert!(load(&storage, 1, 7).is_none());
        storage.fail.set(false);
        assert!(merge_save(&storage, 1, 7, &a, 2, "").is_ok());
        let saved = load(&storage, 1, 7).unwrap().snapshot;
        storage.fail.set(true);
        let b = snapshot(20);
        assert!(merge_save(&storage, 1, 7, &b, 2, "").is_err());
        assert_eq!(load(&storage, 1, 7).unwrap().snapshot, saved);
        assert_eq!(load(&storage, 1, 7).unwrap().content_ready, Some(true));
        storage.fail.set(false);
        assert!(merge_save(&storage, 1, 7, &b, 2, "").is_ok());
        let loaded = load(&storage, 1, 7).unwrap();
        assert_eq!(loaded.permission, 2);
        assert_eq!(
            ultros_list_doc::ListDocument::from_snapshot(&loaded.snapshot)
                .unwrap()
                .rows()
                .len(),
            2
        );
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
