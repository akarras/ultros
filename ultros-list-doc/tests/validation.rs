//! Boundary values and malformed peer data must not corrupt typed row identity
//! or leave half of a rejected operation in the document.
use ultros_list_doc::{DocError, ListDocument, MetaSnapshot, Quality, RowKey, RowSnapshot};

fn seeded() -> ListDocument {
    let doc = ListDocument::new();
    doc.rename("Safe list").unwrap();
    doc.add_row(RowKey::new(1, None), 5, Some(10)).unwrap();
    doc
}

#[test]
fn supported_legacy_rows_keep_defaults_without_rewriting_history() {
    let raw = loro::LoroDoc::new();
    raw.get_map("meta").insert("name", "Legacy").unwrap();
    let row = raw
        .get_map("rows")
        .insert_container("1:any", loro::LoroMap::new())
        .unwrap();
    row.insert("need", 4_i64).unwrap();
    raw.commit();
    let version = raw.oplog_vv().encode();
    let loaded =
        ListDocument::from_snapshot(&raw.export(loro::ExportMode::Snapshot).unwrap()).unwrap();
    assert_eq!(loaded.schema(), None);
    assert_eq!(loaded.version(), version);
    let receiver = ListDocument::empty_peer();
    assert!(
        !receiver
            .import(&loaded.export_snapshot().unwrap())
            .unwrap()
            .pending
    );
    assert_eq!(receiver.schema(), None);
    assert_eq!(receiver.rows(), loaded.rows());
    assert_eq!(
        loaded.rows(),
        vec![RowSnapshot {
            key: RowKey::new(1, None),
            need: 4,
            acquired: 0,
            target: None
        }]
    );
    loaded.add_acquired(&RowKey::new(1, None), 2).unwrap();
    assert_eq!(
        ListDocument::from_snapshot(&loaded.export_snapshot().unwrap())
            .unwrap()
            .rows()[0]
            .acquired,
        2
    );
}

#[test]
fn unsupported_schema_and_bad_typed_fields_reject_snapshot_and_import_atomically() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    // Every case also includes a valid rename: no valid subset may leak out.
    let mutations: &[fn(&ListDocument)] = &[
        |d| {
            d.inner().get_map("meta").insert("schema", 999_i64).unwrap();
        },
        |d| {
            d.inner().get_map("meta").insert("schema", "1").unwrap();
        },
        |d| {
            d.inner().get_map("meta").insert("schema", 1.0).unwrap();
        },
        |d| {
            d.inner()
                .get_map("meta")
                .insert("scope", "world:-1")
                .unwrap();
        },
        |d| {
            d.inner()
                .get_map("meta")
                .insert("scope", "world:01")
                .unwrap();
        },
        |d| {
            d.inner().get_map("meta").insert("name", 9_i64).unwrap();
        },
        |d| {
            d.inner()
                .get_map("rows")
                .insert("2:any", "not a map")
                .unwrap();
        },
        |d| {
            d.inner()
                .get_map("rows")
                .insert_container("01:any", loro::LoroMap::new())
                .unwrap();
        },
        |d| {
            d.inner()
                .get_map("rows")
                .insert_container("0:any", loro::LoroMap::new())
                .unwrap();
        },
        |d| {
            typed_row(d).insert("item", 2_i64).unwrap();
        },
        |d| {
            typed_row(d).insert("quality", "hq").unwrap();
        },
        |d| {
            typed_row(d).insert("need", "5").unwrap();
        },
        |d| {
            typed_row(d).insert("target", 3.5).unwrap();
        },
        |d| {
            typed_row(d).delete("need").unwrap();
        },
        |d| {
            typed_row(d).insert("acquired", 2_i64).unwrap();
        },
        |d| {
            typed_row(d)
                .insert_container("acquired", loro::LoroMap::new())
                .unwrap();
        },
        |d| {
            let c = typed_row(d)
                .insert_container("acquired", loro::LoroCounter::new())
                .unwrap();
            c.increment(0.5).unwrap();
        },
        |d| {
            typed_row(d).insert("future-field", true).unwrap();
        },
    ];
    for (index, mutate) in mutations.iter().enumerate() {
        let live = seeded();
        let before = live.export_snapshot().unwrap();
        let version = live.version();
        let peer = ListDocument::from_snapshot(&before).unwrap();
        peer.rename("Must not leak").unwrap();
        mutate(&peer);
        peer.commit();
        let snapshot = peer.export_snapshot().unwrap();
        assert!(
            ListDocument::from_snapshot(&snapshot).is_err(),
            "snapshot case {index}"
        );
        let events = Arc::new(AtomicUsize::new(0));
        let count = events.clone();
        let _subscription = live.on_change(move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        assert!(
            live.import(&peer.export_since(&version).unwrap()).is_err(),
            "import case {index}"
        );
        assert_eq!(live.version(), version, "version case {index}");
        assert_eq!(live.meta().name, "Safe list", "meta case {index}");
        assert_eq!(live.rows(), seeded().rows(), "rows case {index}");
        assert_eq!(events.load(Ordering::SeqCst), 0, "events case {index}");
    }
}

fn typed_row(doc: &ListDocument) -> loro::LoroMap {
    let Some(loro::ValueOrContainer::Container(loro::Container::Map(row))) =
        doc.inner().get_map("rows").get("1:any")
    else {
        panic!("fixture row")
    };
    row
}

#[test]
fn malformed_root_types_are_rejected_before_default_maps_can_hide_them() {
    for root in ["meta", "rows", "unknown"] {
        let raw = loro::LoroDoc::new();
        raw.get_list(root).push("malformed").unwrap();
        raw.commit();
        assert!(matches!(
            ListDocument::from_snapshot(&raw.export(loro::ExportMode::Snapshot).unwrap()),
            Err(DocError::InvalidStructure(_))
        ));
    }
}

#[test]
fn dependency_incomplete_update_cannot_later_activate_unvalidated_operations() {
    let live = seeded();
    let version = live.version();
    let peer = ListDocument::from_snapshot(&live.export_snapshot().unwrap()).unwrap();
    peer.rename("Dependency").unwrap();
    let dependency = peer.export_since(&version).unwrap();
    let after_dependency = peer.version();
    peer.inner()
        .get_map("meta")
        .insert("schema", 999_i64)
        .unwrap();
    peer.commit();
    let invalid = peer.export_since(&after_dependency).unwrap();
    assert!(live.import(&invalid).unwrap().pending);
    assert_eq!(live.version(), version);
    assert_eq!(live.meta().name, "Safe list");
    assert!(matches!(
        ListDocument::from_snapshot(&invalid),
        Err(DocError::IncompleteSnapshot)
    ));
    live.import(&dependency).unwrap();
    assert_eq!(live.meta().name, "Dependency");
    assert_eq!(live.schema(), Some(1));
    assert!(matches!(
        live.import(&invalid),
        Err(DocError::UnsupportedSchema(999))
    ));
    assert_eq!(live.schema(), Some(1));
}

#[test]
fn signed_quantity_extremes_and_negative_counter_totals_round_trip() {
    let doc = seeded();
    typed_row(&doc).insert("need", i64::MIN).unwrap();
    typed_row(&doc).insert("target", i64::MAX).unwrap();
    doc.add_acquired(&RowKey::new(1, None), -10).unwrap();
    let restored = ListDocument::from_snapshot(&doc.export_snapshot().unwrap()).unwrap();
    assert_eq!(restored.rows(), doc.rows());
    assert_eq!(restored.rows()[0].acquired, -10);
}

#[test]
fn explicit_versions_other_than_current_are_never_treated_as_legacy() {
    for version in [-1_i64, 0, 2, 999, i64::MAX] {
        let doc = seeded();
        doc.inner()
            .get_map("meta")
            .insert("schema", version)
            .unwrap();
        doc.commit();
        assert!(
            matches!(ListDocument::from_snapshot(&doc.export_snapshot().unwrap()), Err(DocError::UnsupportedSchema(found)) if found == version)
        );
    }
}

#[test]
fn row_identity_bounds_and_out_of_range_counter_values_are_rejected() {
    for item in [-1, 0, i32::MAX] {
        let doc = ListDocument::new();
        doc.add_row(RowKey::new(item, None), 1, None).unwrap();
        assert!(matches!(
            ListDocument::from_snapshot(&doc.export_snapshot().unwrap()),
            Err(DocError::InvalidStructure(_))
        ));
    }
    let doc = seeded();
    let counter = typed_row(&doc)
        .insert_container("acquired", loro::LoroCounter::new())
        .unwrap();
    counter.increment(1e20).unwrap();
    doc.commit();
    assert!(matches!(
        ListDocument::from_snapshot(&doc.export_snapshot().unwrap()),
        Err(DocError::InvalidStructure(_))
    ));
}

#[test]
fn imported_aliases_reject_the_entire_document_instead_of_dropping_rows() {
    let source = ListDocument::new();
    let rows = source.inner().get_map("rows");
    for key in ["1:any", "01:any", "+1:any", "-0:any", "junk:any"] {
        let row = rows.insert_container(key, loro::LoroMap::new()).unwrap();
        row.insert("need", 1_i64).unwrap();
    }
    source.commit();
    assert!(matches!(
        ListDocument::from_snapshot(&source.export_snapshot().unwrap()),
        Err(DocError::InvalidStructure(_))
    ));
    for alias in ["01:any", "+1:any", "-0:any"] {
        assert!(alias.parse::<RowKey>().is_err());
    }
}

#[test]
fn add_overflow_rejects_the_entire_edit_including_target() {
    let doc = ListDocument::new();
    let key = RowKey::new(1, None);
    doc.add_row(key, i64::MAX, Some(5)).unwrap();
    let before = doc.rows();
    let version = doc.version();
    assert!(matches!(
        doc.add_row(key, 1, Some(10)),
        Err(DocError::QuantityOverflow)
    ));
    doc.commit();
    assert_eq!(doc.rows(), before);
    assert_eq!(doc.version(), version);
}

#[test]
fn acquired_difference_overflow_leaves_the_counter_unchanged() {
    for (current, requested) in [(i64::MIN, 1), (i64::MAX, -2)] {
        let key = RowKey::new(1, None);
        let doc = ListDocument::from_rows(
            MetaSnapshot {
                name: String::new(),
                scope: None,
            },
            &[RowSnapshot {
                key,
                need: 1,
                acquired: current,
                target: None,
            }],
        );
        let before = doc.rows();
        let version = doc.version();
        assert!(matches!(
            doc.set_acquired(&key, requested),
            Err(DocError::QuantityOverflow)
        ));
        doc.commit();
        assert_eq!(doc.rows(), before);
        assert_eq!(doc.version(), version);
    }
}

#[test]
fn quality_merge_overflow_preserves_both_rows() {
    let key = RowKey::new(1, None);
    let doc = ListDocument::new();
    doc.add_row(key, i64::MAX, Some(12)).unwrap();
    doc.add_row(RowKey::new(1, Some(true)), 1, Some(34))
        .unwrap();
    let before = doc.rows();
    let version = doc.version();
    assert!(matches!(
        doc.set_quality(&key, Quality::Hq),
        Err(DocError::QuantityOverflow)
    ));
    doc.commit();
    assert_eq!(doc.rows(), before);
    assert_eq!(doc.version(), version);
}

#[test]
fn full_row_keys_keep_known_hash_collisions_independent() {
    let first = RowKey::new(21482, None);
    let second = RowKey::new(41373, Some(true));
    let doc = ListDocument::new();
    doc.add_row(first, 2, None).unwrap();
    doc.add_row(second, 3, None).unwrap();
    doc.set_need(&first, 9).unwrap();
    assert_eq!(doc.row(&second).unwrap().need, 3);
    let mut undo = ultros_list_doc::ListUndo::with_merge_interval(&doc, 0);
    doc.remove_row(&first).unwrap();
    assert!(doc.row(&first).is_none());
    assert_eq!(doc.row(&second).unwrap().need, 3);
    undo.undo().unwrap();
    assert_eq!(doc.row(&first).unwrap().need, 9);
    assert_eq!(doc.row(&second).unwrap().need, 3);
}
