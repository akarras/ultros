//! Boundary values and malformed peer data must not corrupt typed row identity
//! or leave half of a rejected operation in the document.
use ultros_list_doc::{DocError, ListDocument, MetaSnapshot, Quality, RowKey, RowSnapshot};

#[test]
fn imported_aliases_do_not_produce_duplicate_typed_rows() {
    let source = ListDocument::new();
    let rows = source.inner().get_map("rows");
    for key in ["1:any", "01:any", "+1:any", "-0:any", "junk:any"] {
        let row = rows.insert_container(key, loro::LoroMap::new()).unwrap();
        row.insert("need", 1_i64).unwrap();
    }
    source.commit();
    let imported = ListDocument::from_snapshot(&source.export_snapshot().unwrap()).unwrap();
    assert_eq!(imported.rows().len(), 1);
    let key = RowKey::new(1, None);
    assert_eq!(imported.rows()[0], imported.row(&key).unwrap());
    imported.remove_row(&key).unwrap();
    assert!(imported.rows().is_empty());
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
