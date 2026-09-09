//! Today's row shapes from the document, so every existing list component
//! renders unchanged (spec section 3.2), and the edits those components
//! dispatch, each applied as one undo step.

use std::collections::HashMap;

use ultros_api_types::ActiveListing;
use ultros_api_types::list::{ListItem, ListWithPermission};
use ultros_api_types::world_helper::AnySelector;
use ultros_list_doc::{DocError, ListDocument, ListUndo, Quality, RowKey, RowSnapshot};

/// A stable, reversible `i32` id for `<For>` keys and row callbacks. FFXIV
/// item ids are far below `2^29`, so `item_id * 4 + code` fits comfortably in
/// `i32` and stays positive; `key_of` decodes it back exactly. It exists only
/// on the Labs page and is never sent to a REST endpoint.
fn quality_code(quality: Quality) -> i32 {
    match quality {
        Quality::Any => 0,
        Quality::Nq => 1,
        Quality::Hq => 2,
    }
}

fn code_quality(code: i32) -> Option<Quality> {
    match code {
        0 => Some(Quality::Any),
        1 => Some(Quality::Nq),
        2 => Some(Quality::Hq),
        _ => None,
    }
}

/// `None` when `item_id * 4 + code` overflows `i32`. FFXIV item ids sit far
/// below `2^29` today, so this is not expected to trigger in practice; it
/// exists so a future id range change fails a row closed rather than
/// wrapping into another row's id.
pub fn row_id(key: &RowKey) -> Option<i32> {
    key.item_id
        .checked_mul(4)?
        .checked_add(quality_code(key.quality))
}

/// Decodes an id produced by `row_id`. `None` for an id whose low two bits
/// don't name a `Quality` (code `3`) or that isn't positive.
pub fn key_of(id: i32) -> Option<RowKey> {
    if id <= 0 {
        return None;
    }
    let quality = code_quality(id & 0b11)?;
    Some(RowKey {
        item_id: id >> 2,
        quality,
    })
}

fn clamp_i32(value: i64) -> i32 {
    value.clamp(0, i32::MAX as i64) as i32
}

/// `None` when `row_id` overflows for this row's key; the row is skipped by
/// callers rather than rendered under a wrapped, possibly-colliding id.
pub fn to_list_item(list_id: i32, row: &RowSnapshot) -> Option<ListItem> {
    Some(ListItem {
        id: row_id(&row.key)?,
        item_id: row.key.item_id,
        list_id,
        hq: row.key.hq(),
        quantity: Some(clamp_i32(row.need)),
        acquired: Some(clamp_i32(row.acquired)),
        target_price: row.target,
    })
}

/// The row an id decodes to, if the document still has it. Decoding is
/// enough to reconstruct the key; the document lookup confirms the row
/// hasn't since been removed.
pub fn find_key(doc: &ListDocument, id: i32) -> Option<RowKey> {
    key_of(id).filter(|key| doc.row(key).is_some())
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
    // `to_list_item` skips a row whose id doesn't fit `i32`; see its doc
    // comment. Such a row is simply absent from the rendered list rather
    // than shown under a wrapped id that might collide with another row's.
    let rows = doc
        .rows()
        .iter()
        .filter_map(|row| {
            let item = to_list_item(list_id, row)?;
            Some((
                item,
                listings.get(&row.key.item_id).cloned().unwrap_or_default(),
            ))
        })
        .collect();
    (list, rows)
}

/// One user action. `Remove`, `Edit` and `SetQuality` ignore ids that are
/// already gone, so a double click or a stale row is harmless.
// `Edit::Edit` mirrors the interface this task's brief specifies verbatim
// (the enum names the operation, `Edit` is one such operation); renaming the
// variant to dodge the lint would just create a mismatch with the spec.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Debug)]
pub enum Edit {
    Add(ListItem),
    /// The caller must keep the ORIGINAL `id` on the mutated `ListItem`: ids
    /// are derived from the row key (see `row_id`/`find_key`), so an id that
    /// reflects the edit's new `hq`/`item_id` would fail to locate the row
    /// this edit is meant to apply to.
    Edit(ListItem),
    Remove(i32),
    RemoveMany(Vec<i32>),
    SetQuality(Vec<i32>, Option<bool>),
    /// Prefers the row whose quality matches `hq` (when it has room for
    /// more); falls back to any row of `item_id` with room. Mirrors the
    /// legacy auto-mark behaviour of "fill the row you're looking at first."
    AddAcquired {
        item_id: i32,
        hq: Option<bool>,
        delta: i64,
    },
    Rename {
        name: String,
        scope: AnySelector,
    },
}

/// Applies one full-row edit the same way the server's own PUT handler does
/// (`ultros/src/lists/edits.rs::apply_list_item_edit`): move quality first if
/// it changed (which merges into any existing row at the destination), then
/// write only the fields that actually differ from the row's value *before*
/// this edit. Writing every field unconditionally would clobber a quality
/// merge's summed `need`/`acquired` with the request's stale copy of them.
fn apply_edit(doc: &ListDocument, before: &RowSnapshot, item: &ListItem) -> Result<(), DocError> {
    let old_key = before.key;
    let new_quality = Quality::from(item.hq);
    let key = if new_quality != old_key.quality {
        doc.set_quality(&old_key, new_quality)?
    } else {
        old_key
    };

    let new_need = item.quantity.unwrap_or(1) as i64;
    if new_need != before.need {
        doc.set_need(&key, new_need)?;
    }

    let new_acquired = item.acquired.unwrap_or(0) as i64;
    if new_acquired != before.acquired {
        doc.set_acquired(&key, new_acquired)?;
    }

    if item.target_price != before.target {
        doc.set_target(&key, item.target_price)?;
    }
    Ok(())
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
            let before = doc.row(&key).expect("find_key confirmed the row exists");
            undo.group(|| apply_edit(doc, &before, &item))
        }
        Edit::Remove(id) => undo.group(|| match find_key(doc, id) {
            Some(key) => doc.remove_row(&key),
            None => Ok(()),
        }),
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
        Edit::AddAcquired { item_id, hq, delta } => undo.group(|| {
            let rows = doc.rows();
            let has_room = |r: &&RowSnapshot| r.key.item_id == item_id && r.acquired < r.need;
            // The row matching the quality the caller is looking at, if it
            // has room; otherwise any row of that item with room.
            let wanted = Quality::from(hq);
            let row = rows
                .iter()
                .find(|r| has_room(r) && r.key.quality == wanted)
                .or_else(|| rows.iter().find(has_room));
            let Some(row) = row else {
                return Ok(());
            };
            doc.add_acquired(&row.key, delta)
        }),
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
        // The pair that collided under the old FNV-1a sketch:
        // `21482:any` and `41373:hq` both hashed to 1708812798.
        let any = row_id(&RowKey::new(21482, None)).unwrap();
        let hq = row_id(&RowKey::new(41373, Some(true))).unwrap();
        assert!(any > 0 && hq > 0);
        assert_ne!(any, hq);
        assert_eq!(any, row_id(&RowKey::new(21482, None)).unwrap());
    }

    #[test]
    fn row_id_round_trips_through_key_of_for_every_quality() {
        for hq in [None, Some(true), Some(false)] {
            let key = RowKey::new(4567, hq);
            assert_eq!(key_of(row_id(&key).unwrap()), Some(key));
        }
    }

    #[test]
    fn row_id_reports_none_on_overflow_instead_of_wrapping() {
        // `i32::MAX / 4` is the largest `item_id` that still fits; one past
        // it must not silently wrap into a small, colliding id.
        let near_limit = RowKey::new(i32::MAX / 4, None);
        assert!(row_id(&near_limit).is_some());
        let overflowing = RowKey::new(i32::MAX, None);
        assert_eq!(row_id(&overflowing), None);
        assert!(
            to_list_item(
                1,
                &RowSnapshot {
                    key: overflowing,
                    need: 1,
                    acquired: 0,
                    target: None,
                }
            )
            .is_none()
        );
    }

    #[test]
    fn key_of_rejects_the_unused_code_and_non_positive_ids() {
        assert_eq!(key_of(3), None, "code 3 names no quality");
        assert_eq!(key_of(0), None);
        assert_eq!(key_of(-1), None);
    }

    #[test]
    fn distinct_rows_for_the_same_item_are_independently_addressable() {
        let doc = ListDocument::from_rows(
            MetaSnapshot {
                name: "Doc name".into(),
                scope: None,
            },
            &[
                RowSnapshot {
                    key: RowKey::new(10, Some(false)),
                    need: 2,
                    acquired: 0,
                    target: None,
                },
                RowSnapshot {
                    key: RowKey::new(10, Some(true)),
                    need: 3,
                    acquired: 0,
                    target: None,
                },
            ],
        );
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        let nq_id = row_id(&RowKey::new(10, Some(false))).unwrap();
        let hq_id = row_id(&RowKey::new(10, Some(true))).unwrap();

        // Editing the NQ row never touches the HQ row.
        apply(
            &doc,
            &mut undo,
            Edit::Edit(ListItem {
                id: nq_id,
                item_id: 10,
                list_id: 5,
                hq: Some(false),
                quantity: Some(9),
                acquired: Some(0),
                ..Default::default()
            }),
        )
        .unwrap();
        assert_eq!(doc.row(&RowKey::new(10, Some(false))).unwrap().need, 9);
        assert_eq!(doc.row(&RowKey::new(10, Some(true))).unwrap().need, 3);

        // Removing the HQ row never touches the NQ row.
        apply(&doc, &mut undo, Edit::Remove(hq_id)).unwrap();
        assert!(doc.row(&RowKey::new(10, Some(true))).is_none());
        assert_eq!(doc.row(&RowKey::new(10, Some(false))).unwrap().need, 9);

        // Undoing the removal restores only the HQ row.
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&RowKey::new(10, Some(true))).unwrap().need, 3);
        assert_eq!(doc.row(&RowKey::new(10, Some(false))).unwrap().need, 9);

        // Undoing the edit restores only the NQ row.
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&RowKey::new(10, Some(false))).unwrap().need, 2);
        assert_eq!(doc.row(&RowKey::new(10, Some(true))).unwrap().need, 3);
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
        assert_eq!(rows[0].0, to_list_item(5, &doc.rows()[0]).unwrap());
        assert_eq!(rows[0].0.list_id, 5);
    }

    #[test]
    fn edits_apply_and_each_is_one_undo_step() {
        let doc = doc();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        let existing = to_list_item(5, &doc.rows()[0]).unwrap();

        apply(
            &doc,
            &mut undo,
            Edit::Add(ListItem {
                item_id: 10,
                list_id: 5,
                quantity: Some(3),
                ..Default::default()
            }),
        )
        .unwrap();
        assert_eq!(
            doc.row(&RowKey::new(10, None)).unwrap().need,
            5,
            "add merges into the same key"
        );

        let mut edited = existing.clone();
        edited.hq = Some(true);
        edited.quantity = Some(4);
        edited.acquired = Some(1);
        apply(&doc, &mut undo, Edit::Edit(edited)).unwrap();
        let moved = doc.row(&RowKey::new(10, Some(true))).unwrap();
        assert_eq!((moved.need, moved.acquired), (4, 1));
        assert!(doc.row(&RowKey::new(10, None)).is_none());
        assert!(
            undo.undo().unwrap(),
            "the move plus the edits undo together"
        );
        assert_eq!(doc.row(&RowKey::new(10, None)).unwrap().need, 5);

        apply(&doc, &mut undo, Edit::Remove(999)).unwrap();
        apply(
            &doc,
            &mut undo,
            Edit::AddAcquired {
                item_id: 10,
                hq: None,
                delta: 1,
            },
        )
        .unwrap();
        assert_eq!(doc.row(&RowKey::new(10, None)).unwrap().acquired, 1);
        apply(
            &doc,
            &mut undo,
            Edit::Rename {
                name: "New".into(),
                scope: AnySelector::Region(1),
            },
        )
        .unwrap();
        assert_eq!(doc.meta().name, "New");
        assert!(undo.undo().unwrap());
        assert_eq!(doc.meta().name, "Doc name");

        let id = row_id(&RowKey::new(10, None)).unwrap();
        apply(&doc, &mut undo, Edit::SetQuality(vec![id], Some(false))).unwrap();
        assert!(doc.row(&RowKey::new(10, Some(false))).is_some());
        apply(
            &doc,
            &mut undo,
            Edit::RemoveMany(vec![row_id(&RowKey::new(10, Some(false))).unwrap()]),
        )
        .unwrap();
        assert!(doc.rows().is_empty());
    }

    /// Mirrors `apply_list_item_edit`'s `hq_toggle_onto_existing_row_keeps_merged_sum`:
    /// an HQ toggle onto a row that already exists at the destination quality
    /// keeps that row's merged sum, because the request's unchanged quantity
    /// must not overwrite what `set_quality`'s merge just produced.
    #[test]
    fn edit_toggling_quality_onto_an_existing_row_keeps_the_merged_sum() {
        let doc = ListDocument::new();
        doc.add_row(RowKey::new(1, Some(false)), 2, None).unwrap();
        doc.add_row(RowKey::new(1, Some(true)), 5, None).unwrap();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);

        let nq_id = row_id(&RowKey::new(1, Some(false))).unwrap();
        apply(
            &doc,
            &mut undo,
            Edit::Edit(ListItem {
                id: nq_id,
                item_id: 1,
                list_id: 5,
                hq: Some(true),
                quantity: Some(2),
                acquired: Some(0),
                ..Default::default()
            }),
        )
        .unwrap();

        let row = doc.row(&RowKey::new(1, Some(true))).unwrap();
        assert_eq!(row.need, 7, "merged need must be the sum, not overwritten");
    }

    #[test]
    fn add_acquired_prefers_the_row_matching_hq_then_falls_back_to_any_row_with_room() {
        let doc = ListDocument::from_rows(
            MetaSnapshot {
                name: "Doc name".into(),
                scope: None,
            },
            &[
                RowSnapshot {
                    key: RowKey::new(10, Some(false)),
                    need: 2,
                    acquired: 0,
                    target: None,
                },
                RowSnapshot {
                    key: RowKey::new(10, Some(true)),
                    need: 2,
                    acquired: 0,
                    target: None,
                },
            ],
        );
        let mut undo = ListUndo::with_merge_interval(&doc, 0);

        // Asking for HQ fills the HQ row, not the NQ row that happens to sort
        // first.
        apply(
            &doc,
            &mut undo,
            Edit::AddAcquired {
                item_id: 10,
                hq: Some(true),
                delta: 1,
            },
        )
        .unwrap();
        assert_eq!(doc.row(&RowKey::new(10, Some(true))).unwrap().acquired, 1);
        assert_eq!(doc.row(&RowKey::new(10, Some(false))).unwrap().acquired, 0);

        // Once the HQ row has no room left, asking for HQ again falls back to
        // any row of the item with room.
        doc.set_need(&RowKey::new(10, Some(true)), 1).unwrap();
        apply(
            &doc,
            &mut undo,
            Edit::AddAcquired {
                item_id: 10,
                hq: Some(true),
                delta: 1,
            },
        )
        .unwrap();
        assert_eq!(doc.row(&RowKey::new(10, Some(false))).unwrap().acquired, 1);
    }
}
