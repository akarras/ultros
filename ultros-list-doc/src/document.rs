//! `ListDocument`: the typed wrapper around one Loro document (spec section 1).
//!
//! Layout:
//!
//! ```text
//! root map "meta": { name: string, scope: "world:79" | "datacenter:5" | "region:1", schema: 1 }
//! root map "rows": { "{item_id}:{quality}": { item: i64, quality: string, need: i64,
//!                                            target: i64 (absent = none), acquired: Counter } }
//! ```

use std::collections::BTreeSet;
use std::sync::Arc;

use loro::{
    Container, ExportMode, LoroCounter, LoroDoc, LoroMap, LoroValue, Subscription,
    ValueOrContainer, VersionVector,
};
use ultros_api_types::world_helper::AnySelector;

use crate::key::{Quality, RowKey};
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
    #[error("the update depends on history this document has compacted away")]
    OutdatedDependency,
    #[error("row `{0}` is not in the document")]
    MissingRow(RowKey),
    #[error("quantity arithmetic exceeds the supported i64 range")]
    QuantityOverflow,
}

/// What an import did. `pending` means the bytes depend on history this
/// document has not seen; Loro applies them once that history arrives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportReport {
    pub pending: bool,
}

/// The server's answer to a subscribe handshake (spec section 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncPayload {
    Snapshot(Vec<u8>),
    Updates(Vec<u8>),
    UpToDate,
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

/// `None` for empty or undecodable bytes; both mean "this peer has nothing
/// we can diff against".
fn decode_version(bytes: &[u8]) -> Option<VersionVector> {
    if bytes.is_empty() {
        return None;
    }
    VersionVector::decode(bytes).ok()
}

fn value_string(value: Option<ValueOrContainer>) -> Option<String> {
    match value {
        Some(ValueOrContainer::Value(LoroValue::String(s))) => Some(s.to_string()),
        _ => None,
    }
}

impl ListDocument {
    /// A document with Loro's default random peer id, carrying nothing but
    /// `meta.schema`. Two tabs are two peers: a peer id is never stored or
    /// shared between concurrent writers.
    ///
    /// The schema write is one local commit, so a "fresh" document already has
    /// history; `from_snapshot` deliberately does not go through here, or every
    /// loaded copy would invent a redundant local operation of its own.
    pub fn new() -> Self {
        let document = Self::blank();
        document
            .meta_map()
            .insert(SCHEMA, SCHEMA_VERSION)
            .expect("fresh map insert");
        document.doc.commit();
        document
    }

    /// No schema, no commits: the base for `from_snapshot`.
    fn blank() -> Self {
        Self {
            doc: LoroDoc::new(),
        }
    }

    pub fn from_snapshot(bytes: &[u8]) -> Result<Self, DocError> {
        let document = Self::blank();
        document.doc.import(bytes)?;
        Ok(document)
    }

    /// Build a document from relational rows: the server's first-touch path.
    pub fn from_rows(meta: MetaSnapshot, rows: &[RowSnapshot]) -> Self {
        debug_assert_eq!(
            rows.iter().map(|r| r.key).collect::<BTreeSet<_>>().len(),
            rows.len(),
            "from_rows was given duplicate row keys; the later row would silently overwrite the earlier one"
        );
        let document = Self::new();
        let meta_map = document.meta_map();
        // Inserting plain values into a fresh attached map cannot fail.
        meta_map
            .insert(NAME, meta.name.as_str())
            .expect("fresh map insert");
        if let Some(scope) = meta.scope {
            meta_map
                .insert(SCOPE, encode_scope(scope).as_str())
                .expect("fresh map insert");
        }
        // `new()` already wrote `meta.schema`.
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
        // `item` and `quality` are written for external readers of the raw
        // document; this crate derives both from the map key.
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

    /// The schema version the document was written with, or `None` for a
    /// document from before `meta.schema` existed.
    pub fn schema(&self) -> Option<i64> {
        value_i64(self.meta_map().get(SCHEMA))
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
    ///
    /// Under concurrency the merged `need` is last-writer-wins, not additive:
    /// two devices each adding 3 to a row of 2 converge to 5, not 8.
    /// `acquired` is the only additive field.
    pub fn add_row(&self, key: RowKey, need: i64, target: Option<i64>) -> Result<(), DocError> {
        match self.row_container(&key) {
            Some(row) => {
                let current = value_i64(row.get(NEED)).unwrap_or(0);
                let combined = current
                    .checked_add(need)
                    .ok_or(DocError::QuantityOverflow)?;
                row.insert(NEED, combined.max(0))?;
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
        // Validate arithmetic before deleting the source: a rejected move must
        // leave both rows intact, including any pending document transaction.
        let destination = self.row_container(&new_key);
        let combined_need = destination
            .as_ref()
            .map(|existing| {
                value_i64(existing.get(NEED))
                    .unwrap_or(0)
                    .checked_add(snapshot.need)
                    .ok_or(DocError::QuantityOverflow)
            })
            .transpose()?;
        self.rows_map().delete(&key.to_string())?;
        match destination {
            Some(existing) => {
                existing.insert(NEED, combined_need.expect("destination was validated"))?;
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

    /// Values are exact up to 2^53; the counter is an f64 underneath.
    pub fn add_acquired(&self, key: &RowKey, delta: i64) -> Result<(), DocError> {
        let row = self.row_container(key).ok_or(DocError::MissingRow(*key))?;
        if delta != 0 {
            Self::counter(&row)?.increment(delta as f64)?;
        }
        self.doc.commit();
        Ok(())
    }

    /// Set an absolute value by incrementing the counter by the difference.
    ///
    /// Two peers each setting the same absolute value concurrently from the
    /// same base double it: `set_acquired(5)` on both sides of a 0 converges
    /// to 10. Consumers that display `acquired` should clamp to `need` for
    /// display, never in the document.
    pub fn set_acquired(&self, key: &RowKey, value: i64) -> Result<(), DocError> {
        let current = self.row(key).ok_or(DocError::MissingRow(*key))?.acquired;
        let delta = value
            .checked_sub(current)
            .ok_or(DocError::QuantityOverflow)?;
        self.add_acquired(key, delta)
    }

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
        // `state_frontiers()` equals `oplog_frontiers()` here: this crate
        // never detaches or checks out.
        let frontiers = self.doc.state_frontiers();
        Ok(self.doc.export(ExportMode::shallow_snapshot(&frontiers))?)
    }

    pub fn export_all(&self) -> Result<Vec<u8>, DocError> {
        Ok(self.doc.export(ExportMode::all_updates())?)
    }

    /// Updates the holder of `version` has not seen. An empty `version` means
    /// everything.
    ///
    /// On a document loaded from a shallow snapshot, a `version` older than the
    /// shallow root yields a truncated update with no error; call
    /// `can_export_since` first, or use `sync_payload`, which does.
    pub fn export_since(&self, version: &[u8]) -> Result<Vec<u8>, DocError> {
        let vv = if version.is_empty() {
            VersionVector::default()
        } else {
            VersionVector::decode(version).map_err(|_| DocError::Version)?
        };
        Ok(self.doc.export(ExportMode::updates(&vv))?)
    }

    pub fn import(&self, bytes: &[u8]) -> Result<ImportReport, DocError> {
        let status = self.doc.import(bytes).map_err(|error| match error {
            // The bytes are well formed but hang off history a shallow
            // snapshot dropped. The caller answers with a fresh snapshot.
            loro::LoroError::ImportUpdatesThatDependsOnOutdatedVersion => {
                DocError::OutdatedDependency
            }
            other => DocError::Loro(other),
        })?;
        Ok(ImportReport {
            pending: status.pending.is_some(),
        })
    }

    /// True when `version` decodes and is at or after this document's shallow
    /// root, so `export_since(version)` would be complete rather than silently
    /// truncated. A document loaded from a full snapshot has an empty shallow
    /// root, for which every decodable version qualifies.
    pub fn can_export_since(&self, version: &[u8]) -> bool {
        let Some(client) = decode_version(version) else {
            return false;
        };
        self.doc
            .shallow_since_vv()
            .iter()
            .all(|(peer, counter)| *counter <= client.get(peer).copied().unwrap_or(0))
    }

    /// What to send a peer that reports `client_version`: a snapshot when it
    /// has nothing usable or sits behind our shallow root, nothing when it is
    /// level, otherwise the updates it lacks. A peer that is ahead still gets
    /// `Updates`, possibly carrying nothing new; it then sends its own diff.
    pub fn sync_payload(&self, client_version: &[u8]) -> Result<SyncPayload, DocError> {
        let Some(client) = decode_version(client_version) else {
            return Ok(SyncPayload::Snapshot(self.export_snapshot()?));
        };
        if !self.can_export_since(client_version) {
            return Ok(SyncPayload::Snapshot(self.export_snapshot()?));
        }
        if client == self.doc.oplog_vv() {
            return Ok(SyncPayload::UpToDate);
        }
        Ok(SyncPayload::Updates(self.export_since(client_version)?))
    }

    /// True when this document holds operations the holder of `version` has
    /// not seen: there is something to send it. An undecodable or empty
    /// `version` gets everything.
    pub fn is_ahead_of(&self, version: &[u8]) -> bool {
        let Some(theirs) = decode_version(version) else {
            return true;
        };
        self.doc
            .oplog_vv()
            .iter()
            .any(|(peer, counter)| *counter > theirs.get(peer).copied().unwrap_or(0))
    }

    /// Bytes for every local commit, ready to send to other peers. Remote
    /// imports do not fire this.
    pub fn on_local_update(&self, f: impl Fn(&[u8]) + Send + Sync + 'static) -> Subscription {
        self.doc
            .subscribe_local_update(Box::new(move |bytes: &Vec<u8>| {
                f(bytes.as_slice());
                true
            }))
    }

    /// Any change to the document, local or imported.
    pub fn on_change(&self, f: impl Fn() + Send + Sync + 'static) -> Subscription {
        self.doc.subscribe_root(Arc::new(move |_event| f()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Quality;
    use ultros_api_types::world_helper::AnySelector;

    pub(crate) fn meta() -> MetaSnapshot {
        MetaSnapshot {
            name: "Housing".to_string(),
            scope: Some(AnySelector::Datacenter(5)),
        }
    }

    pub(crate) fn row(item: i32, quality: Quality, need: i64, acquired: i64) -> RowSnapshot {
        RowSnapshot {
            key: RowKey {
                item_id: item,
                quality,
            },
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
        assert_eq!(
            doc.row(&RowKey::new(10, Some(false))).unwrap().target,
            Some(500)
        );
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

    #[test]
    fn add_row_creates_then_merges_need_into_the_same_key() {
        let doc = ListDocument::from_rows(meta(), &[]);
        let key = RowKey::new(10, None);
        doc.add_row(key, 2, None).unwrap();
        doc.add_row(key, 3, Some(90)).unwrap();
        assert_eq!(
            doc.rows(),
            vec![RowSnapshot {
                key,
                need: 5,
                acquired: 0,
                target: Some(90)
            }]
        );
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
        assert_eq!(
            doc.row(&key).unwrap(),
            RowSnapshot {
                key,
                need: 7,
                acquired: 5,
                target: Some(120)
            }
        );
        doc.set_acquired(&key, 1).unwrap();
        doc.set_target(&key, None).unwrap();
        doc.set_need(&key, -4).unwrap();
        assert_eq!(
            doc.row(&key).unwrap(),
            RowSnapshot {
                key,
                need: 0,
                acquired: 1,
                target: None
            }
        );
    }

    #[test]
    fn set_quality_moves_the_row_with_every_field() {
        let key = RowKey::new(10, None);
        let doc = ListDocument::from_rows(
            meta(),
            &[RowSnapshot {
                target: Some(50),
                ..row(10, Quality::Any, 4, 2)
            }],
        );
        let moved = doc.set_quality(&key, Quality::Hq).unwrap();
        assert_eq!(moved, RowKey::new(10, Some(true)));
        assert_eq!(doc.row(&key), None);
        assert_eq!(
            doc.row(&moved).unwrap(),
            RowSnapshot {
                key: moved,
                need: 4,
                acquired: 2,
                target: Some(50)
            }
        );
        assert_eq!(
            doc.set_quality(&moved, Quality::Hq).unwrap(),
            moved,
            "no-op move keeps the key"
        );
    }

    #[test]
    fn set_quality_merges_into_an_existing_row_of_that_quality() {
        let doc = ListDocument::from_rows(
            meta(),
            &[row(10, Quality::Any, 2, 1), row(10, Quality::Hq, 3, 0)],
        );
        let moved = doc
            .set_quality(&RowKey::new(10, None), Quality::Hq)
            .unwrap();
        assert_eq!(
            doc.rows(),
            vec![RowSnapshot {
                key: moved,
                need: 5,
                acquired: 1,
                target: None
            }]
        );
    }

    #[test]
    fn rename_and_scope_write_meta() {
        let doc = ListDocument::from_rows(meta(), &[]);
        doc.rename("Glamour").unwrap();
        doc.set_scope(AnySelector::World(79)).unwrap();
        assert_eq!(
            doc.meta(),
            MetaSnapshot {
                name: "Glamour".into(),
                scope: Some(AnySelector::World(79))
            }
        );
    }

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
        assert_eq!(
            a.export_since(&[]).unwrap().len(),
            a.export_all().unwrap().len()
        );
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
        a.import(&fresh.export_since(&a.version()).unwrap())
            .unwrap();
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
        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "imports are not local updates"
        );
        // The captured bytes are a valid update for another peer.
        let c = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
        c.import(&seen.lock().unwrap()[0]).unwrap();
    }

    #[test]
    fn undo_fires_a_local_update_that_round_trips() {
        use crate::undo::ListUndo;
        use std::sync::{Arc, Mutex};

        let a = ListDocument::from_rows(meta(), &[]);
        let seen: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let _sub = a.on_local_update(move |bytes| sink.lock().unwrap().push(bytes.to_vec()));
        let mut undo = ListUndo::with_merge_interval(&a, 0);

        a.add_row(RowKey::new(1, None), 2, None).unwrap();
        assert!(undo.undo().unwrap());

        let updates = seen.lock().unwrap();
        assert_eq!(
            updates.len(),
            2,
            "one local update for the add, one for the undo"
        );

        // Importing both into a fresh document round-trips to no rows: the
        // undo's ops apply cleanly on top of the add.
        let fresh = ListDocument::new();
        for update in updates.iter() {
            fresh.import(update).unwrap();
        }
        assert!(fresh.rows().is_empty());
    }

    #[test]
    fn on_change_fires_for_local_and_remote_changes() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
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

    #[test]
    fn a_new_document_carries_the_schema_version() {
        assert_eq!(ListDocument::new().schema(), Some(SCHEMA_VERSION));
        assert_eq!(
            ListDocument::from_rows(meta(), &[]).schema(),
            Some(SCHEMA_VERSION)
        );
        let loaded =
            ListDocument::from_snapshot(&ListDocument::new().export_snapshot().unwrap()).unwrap();
        assert_eq!(loaded.schema(), Some(SCHEMA_VERSION));
        // A document from before `meta.schema` existed reads as `None`.
        let raw = loro::LoroDoc::new();
        raw.get_map(META).insert(NAME, "old").unwrap();
        raw.commit();
        let legacy =
            ListDocument::from_snapshot(&raw.export(loro::ExportMode::Snapshot).unwrap()).unwrap();
        assert_eq!(legacy.schema(), None);
        assert_eq!(legacy.meta().name, "old");
    }

    /// A shallow snapshot drops history, so an update that hangs off a version
    /// older than the shallow root has nothing to attach to.
    #[test]
    fn importing_an_update_that_predates_the_shallow_root_is_outdated() {
        let key = RowKey::new(1, None);
        let a = ListDocument::from_rows(meta(), &[row(1, Quality::Any, 1, 0)]);
        let old = a.version();
        // A third peer that only ever knew the old history.
        let stale = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
        for need in 2..42 {
            a.set_need(&key, need).unwrap();
        }
        let b = ListDocument::from_snapshot(&a.export_shallow().unwrap()).unwrap();
        stale.set_need(&key, 500).unwrap();
        let update = stale.export_since(&old).unwrap();
        match b.import(&update) {
            Err(DocError::OutdatedDependency) => {}
            other => panic!("expected OutdatedDependency, got {other:?}"),
        }
    }

    #[test]
    fn sync_payload_picks_snapshot_updates_or_up_to_date() {
        let key = RowKey::new(1, None);
        let server = ListDocument::from_rows(meta(), &[row(1, Quality::Any, 1, 0)]);
        assert!(matches!(
            server.sync_payload(&[]).unwrap(),
            SyncPayload::Snapshot(_)
        ));
        assert!(matches!(
            server.sync_payload(b"junk").unwrap(),
            SyncPayload::Snapshot(_)
        ));
        let client = ListDocument::from_snapshot(&server.export_snapshot().unwrap()).unwrap();
        assert_eq!(
            server.sync_payload(&client.version()).unwrap(),
            SyncPayload::UpToDate
        );
        server.set_need(&key, 3).unwrap();
        let SyncPayload::Updates(bytes) = server.sync_payload(&client.version()).unwrap() else {
            panic!("expected updates");
        };
        client.import(&bytes).unwrap();
        assert_eq!(client.row(&key).unwrap().need, 3);
        // A client that is ahead gets nothing to import; it sends its own diff.
        client.set_need(&key, 4).unwrap();
        assert!(matches!(
            server.sync_payload(&client.version()).unwrap(),
            SyncPayload::Updates(_)
        ));
    }

    #[test]
    fn sync_payload_sends_a_snapshot_to_a_client_behind_the_shallow_root() {
        let key = RowKey::new(1, None);
        let full = ListDocument::from_rows(meta(), &[row(1, Quality::Any, 1, 0)]);
        let old = full.version();
        for need in 2..42 {
            full.set_need(&key, need).unwrap();
        }
        let server = ListDocument::from_snapshot(&full.export_shallow().unwrap()).unwrap();
        assert!(!server.can_export_since(&old));
        assert!(server.can_export_since(&server.version()));
        assert!(matches!(
            server.sync_payload(&old).unwrap(),
            SyncPayload::Snapshot(_)
        ));
        // A full-history document has an empty shallow root: the same old
        // version is diffable there.
        assert!(full.can_export_since(&old));
    }

    #[test]
    fn is_ahead_of_reports_unsent_local_work() {
        let a = ListDocument::from_rows(meta(), &[row(1, Quality::Any, 1, 0)]);
        let b = ListDocument::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
        assert!(!a.is_ahead_of(&b.version()));
        a.set_need(&RowKey::new(1, None), 2).unwrap();
        assert!(a.is_ahead_of(&b.version()));
        b.import(&a.export_since(&b.version()).unwrap()).unwrap();
        assert!(!a.is_ahead_of(&b.version()));
        assert!(
            a.is_ahead_of(b"junk"),
            "an unreadable version gets everything"
        );
        assert!(a.is_ahead_of(&[]));
    }
}
