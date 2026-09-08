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
        Some(ValueOrContainer::Value(LoroValue::String(s))) => Some(s.to_string()),
        _ => None,
    }
}

impl ListDocument {
    /// An empty document with Loro's default random peer id. Two tabs are two
    /// peers: a peer id is never stored or shared between concurrent writers.
    pub fn new() -> Self {
        Self {
            doc: LoroDoc::new(),
        }
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
        meta_map
            .insert(NAME, meta.name.as_str())
            .expect("fresh map insert");
        if let Some(scope) = meta.scope {
            meta_map
                .insert(SCOPE, encode_scope(scope).as_str())
                .expect("fresh map insert");
        }
        meta_map
            .insert(SCHEMA, SCHEMA_VERSION)
            .expect("fresh map insert");
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
}
