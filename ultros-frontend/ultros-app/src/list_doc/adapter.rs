//! Today's row shapes from the document, so every existing list component
//! renders unchanged (spec section 3.2), and the edits those components
//! dispatch, each applied as one undo step.

use std::collections::HashMap;

use ultros_api_types::ActiveListing;
use ultros_api_types::list::{AddCraftTargetsRequest, ListItem, ListWithPermission};
use ultros_api_types::world_helper::AnySelector;
use ultros_list_doc::{
    CraftMode, CraftOptionsSnapshot, DocError, ListDocument, ListUndo, Quality, RowKey,
    RowSnapshot, TargetKey,
};

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
    /// Recipe previews add their complete ingredient set in one undo step.
    AddMany(Vec<ListItem>),
    /// Shop purchases have their own reversible history, alongside Ctrl+Z.
    RecordPurchase {
        key: RowKey,
        quantity: i64,
        effective_need: Option<i64>,
    },
    UndoPurchase,
    AddTarget {
        recipe_id: i32,
        count: i64,
    },
    AddTargets(AddCraftTargetsRequest),
    SetTargetCount {
        recipe_id: i32,
        count: i64,
    },
    AddTargetCompleted {
        recipe_id: i32,
        delta: i64,
    },
    SetTargetCompleted {
        recipe_id: i32,
        completed: i64,
    },
    RemoveTarget(i32),
    SetCraftChoice {
        item_id: i32,
        mode: CraftMode,
        recipe_id: Option<i32>,
    },
    SetIntermediateOnHand {
        item_id: i32,
        on_hand: i64,
    },
    AddIntermediateCrafted {
        item_id: i32,
        delta: i64,
    },
    SetIntermediateCrafted {
        item_id: i32,
        crafted: i64,
    },
    SetCraftOptions {
        exclude_crystals: bool,
        include_vendors: bool,
    },
    ApplyOptimizedChoices(Vec<(i32, CraftMode, Option<i32>)>),
    /// The caller must keep the ORIGINAL `id` on the mutated `ListItem`: ids
    /// are derived from the row key (see `row_id`/`find_key`), so an id that
    /// reflects the edit's new `hq`/`item_id` would fail to locate the row
    /// this edit is meant to apply to.
    Edit(ListItem),
    Remove(i32),
    RemoveMany(Vec<i32>),
    SetQuality(Vec<i32>, Option<bool>),
    /// Bulk mark-acquired or reset-owned on selected rows as one undo step.
    /// Marking acquired fills each row up to `need.max(1)` without shrinking
    /// an already over-acquired row; resetting sets owned back to zero.
    SetAcquiredMany {
        ids: Vec<i32>,
        acquired: bool,
    },
    /// Reset every owned row back to zero in one undo step for recurring
    /// restock runs. No-op when nothing is owned.
    ResetAllAcquired,
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
    /// The price scope alone, from the inline picker on the list page.
    SetScope(AnySelector),
}

/// Applies one full-row edit the same way the server's own PUT handler does
/// (`ultros-lists/src/edits.rs::apply_list_item_edit`): move quality first if
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
        Edit::RecordPurchase {
            key,
            quantity,
            effective_need,
        } => match effective_need {
            Some(need) => undo.record_purchase_for_need(key, quantity, need),
            None => undo.record_purchase(key, quantity),
        },
        Edit::UndoPurchase => undo.undo_purchase().map(|_| ()),
        Edit::AddTarget { recipe_id, count } => {
            undo.group(|doc| doc.add_target(TargetKey::new(recipe_id), count.clamp(1, 9999)))
        }
        Edit::AddTargets(req) => {
            if req.targets.is_empty() {
                return Ok(());
            }
            undo.group(|doc| {
                for target in &req.targets {
                    doc.add_target(
                        TargetKey::new(target.recipe_id),
                        i64::from(target.count.clamp(1, 9999)),
                    )?;
                }
                for choice in &req.craft_choices {
                    let mode = if choice.buy {
                        CraftMode::Buy
                    } else {
                        CraftMode::Craft
                    };
                    doc.set_craft_choice(choice.item_id, mode, choice.recipe_id)?;
                    if choice.on_hand > 0 {
                        doc.set_intermediate_on_hand(choice.item_id, choice.on_hand)?;
                    }
                }
                if req.exclude_crystals.is_some() || req.include_vendors.is_some() {
                    let mut opts = doc.craft_options();
                    if let Some(exc) = req.exclude_crystals {
                        opts.exclude_crystals = exc;
                    }
                    if let Some(inc) = req.include_vendors {
                        opts.include_vendors = inc;
                    }
                    doc.set_craft_options(opts)?;
                }
                Ok(())
            })
        }
        Edit::SetTargetCount { recipe_id, count } => {
            let key = TargetKey::new(recipe_id);
            if doc.target(&key).is_none() {
                return Ok(());
            }
            undo.group(|doc| doc.set_target_count(&key, count.clamp(1, 9999)))
        }
        Edit::AddTargetCompleted { recipe_id, delta } => {
            let key = TargetKey::new(recipe_id);
            if doc.target(&key).is_none() || delta == 0 {
                return Ok(());
            }
            undo.group(|doc| doc.add_target_completed(&key, delta))
        }
        Edit::SetTargetCompleted {
            recipe_id,
            completed,
        } => {
            let key = TargetKey::new(recipe_id);
            let Some(current) = doc.target(&key) else {
                return Ok(());
            };
            let clamped = completed.max(0);
            if current.completed == clamped {
                return Ok(());
            }
            undo.group(|doc| doc.set_target_completed(&key, clamped))
        }
        Edit::RemoveTarget(recipe_id) => {
            let key = TargetKey::new(recipe_id);
            if doc.target(&key).is_none() {
                return Ok(());
            }
            undo.group(|doc| doc.remove_target(&key))
        }
        Edit::SetCraftChoice {
            item_id,
            mode,
            recipe_id,
        } => undo.group(|doc| doc.set_craft_choice(item_id, mode, recipe_id)),
        Edit::SetIntermediateOnHand { item_id, on_hand } => {
            let clamped = on_hand.clamp(0, 1_000_000);
            if doc.craft_choice(item_id).map_or(0, |c| c.on_hand) == clamped {
                return Ok(());
            }
            undo.group(|doc| doc.set_intermediate_on_hand(item_id, clamped))
        }
        Edit::AddIntermediateCrafted { item_id, delta } => {
            if delta == 0 {
                return Ok(());
            }
            undo.group(|doc| doc.add_intermediate_crafted(item_id, delta))
        }
        Edit::SetIntermediateCrafted { item_id, crafted } => {
            let clamped = crafted.max(0);
            if doc.craft_choice(item_id).map_or(0, |c| c.crafted) == clamped {
                return Ok(());
            }
            undo.group(|doc| doc.set_intermediate_crafted(item_id, clamped))
        }
        Edit::SetCraftOptions {
            exclude_crystals,
            include_vendors,
        } => {
            let next = CraftOptionsSnapshot {
                exclude_crystals,
                include_vendors,
            };
            if doc.craft_options() == next {
                return Ok(());
            }
            undo.group(|doc| doc.set_craft_options(next))
        }
        Edit::ApplyOptimizedChoices(changes) => {
            if changes.is_empty() {
                return Ok(());
            }
            undo.group(|doc| {
                for &(item_id, mode, recipe_id) in &changes {
                    doc.set_craft_choice(item_id, mode, recipe_id)?;
                }
                Ok(())
            })
        }
        Edit::Add(item) => {
            let key = RowKey::new(item.item_id, item.hq);
            let acquired = item.acquired.unwrap_or(0) as i64;
            undo.group(|doc| {
                doc.add_row(key, item.quantity.unwrap_or(1) as i64, item.target_price)?;
                if acquired != 0 {
                    doc.add_acquired(&key, acquired)?;
                }
                Ok(())
            })
        }
        Edit::AddMany(items) => undo.group(|doc| {
            for item in &items {
                let key = RowKey::new(item.item_id, item.hq);
                doc.add_row(key, item.quantity.unwrap_or(1) as i64, item.target_price)?;
                let acquired = item.acquired.unwrap_or(0) as i64;
                if acquired != 0 {
                    doc.add_acquired(&key, acquired)?;
                }
            }
            Ok(())
        }),
        Edit::Edit(item) => {
            let Some(key) = find_key(doc, item.id) else {
                return Ok(());
            };
            let before = doc.row(&key).expect("find_key confirmed the row exists");
            undo.group(|doc| apply_edit(doc, &before, &item))
        }
        Edit::Remove(id) => undo.group(|doc| match find_key(doc, id) {
            Some(key) => doc.remove_row(&key),
            None => Ok(()),
        }),
        Edit::RemoveMany(ids) => undo.group(|doc| {
            for id in &ids {
                if let Some(key) = find_key(doc, *id) {
                    doc.remove_row(&key)?;
                }
            }
            Ok(())
        }),
        Edit::SetQuality(ids, hq) => {
            let quality = Quality::from(hq);
            undo.group(|doc| {
                for id in &ids {
                    if let Some(key) = find_key(doc, *id)
                        && key.quality != quality
                    {
                        doc.set_quality(&key, quality)?;
                    }
                }
                Ok(())
            })
        }
        Edit::SetAcquiredMany { ids, acquired } => {
            let targets: Vec<(RowKey, i64)> = ids
                .iter()
                .filter_map(|id| {
                    let key = find_key(doc, *id)?;
                    let row = doc.row(&key)?;
                    if acquired {
                        let need = row.need.max(1);
                        (row.acquired < need).then_some((key, need))
                    } else {
                        (row.acquired != 0).then_some((key, 0))
                    }
                })
                .collect();
            if targets.is_empty() {
                return Ok(());
            }
            undo.group(|doc| {
                for (key, next) in &targets {
                    doc.set_acquired(key, *next)?;
                }
                Ok(())
            })
        }
        Edit::ResetAllAcquired => {
            let keys: Vec<RowKey> = doc
                .rows()
                .into_iter()
                .filter(|row| row.acquired != 0)
                .map(|row| row.key)
                .collect();
            let completed_targets: Vec<TargetKey> = doc
                .targets()
                .into_iter()
                .filter(|target| target.completed != 0)
                .map(|target| target.key)
                .collect();
            let crafted_choices: Vec<i32> = doc
                .craft_choices()
                .into_iter()
                .filter(|choice| choice.crafted != 0)
                .map(|choice| choice.item_id)
                .collect();
            if keys.is_empty() && completed_targets.is_empty() && crafted_choices.is_empty() {
                return Ok(());
            }
            undo.group(|doc| {
                for key in &keys {
                    doc.set_acquired(key, 0)?;
                }
                for target_key in &completed_targets {
                    doc.set_target_completed(target_key, 0)?;
                }
                for &item_id in &crafted_choices {
                    doc.set_intermediate_crafted(item_id, 0)?;
                }
                Ok(())
            })
        }
        Edit::AddAcquired { item_id, hq, delta } => undo.group(|doc| {
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
        Edit::Rename { name, scope } => undo.group(|doc| {
            doc.rename(&name)?;
            doc.set_scope(scope)
        }),
        Edit::SetScope(scope) => undo.group(|doc| doc.set_scope(scope)),
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
    fn adding_a_recipe_is_one_undo_step_and_merges_existing_need() {
        let doc = doc();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        apply(
            &doc,
            &mut undo,
            Edit::AddMany(vec![
                ListItem {
                    item_id: 10,
                    quantity: Some(3),
                    ..Default::default()
                },
                ListItem {
                    item_id: 11,
                    quantity: Some(7),
                    ..Default::default()
                },
            ]),
        )
        .unwrap();
        assert_eq!(doc.row(&RowKey::new(10, None)).unwrap().need, 5);
        assert_eq!(doc.row(&RowKey::new(11, None)).unwrap().need, 7);
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&RowKey::new(10, None)).unwrap().need, 2);
        assert!(doc.row(&RowKey::new(11, None)).is_none());
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

    #[test]
    fn set_acquired_many_marks_and_resets_selected_rows_in_one_undo_step() {
        let doc = ListDocument::from_rows(
            MetaSnapshot {
                name: "Restock".into(),
                scope: None,
            },
            &[
                RowSnapshot {
                    key: RowKey::new(10, None),
                    need: 5,
                    acquired: 1,
                    target: None,
                },
                RowSnapshot {
                    key: RowKey::new(20, Some(true)),
                    need: 3,
                    acquired: 7, // over-acquired: Mark acquired must not shrink it
                    target: None,
                },
                RowSnapshot {
                    key: RowKey::new(30, None),
                    need: 4,
                    acquired: 0,
                    target: None,
                },
            ],
        );
        let mut undo = ListUndo::with_merge_interval(&doc, 0);
        let id10 = row_id(&RowKey::new(10, None)).unwrap();
        let id20 = row_id(&RowKey::new(20, Some(true))).unwrap();
        let id30 = row_id(&RowKey::new(30, None)).unwrap();

        apply(
            &doc,
            &mut undo,
            Edit::SetAcquiredMany {
                ids: vec![id10, id20],
                acquired: true,
            },
        )
        .unwrap();
        assert_eq!(doc.row(&RowKey::new(10, None)).unwrap().acquired, 5);
        assert_eq!(
            doc.row(&RowKey::new(20, Some(true))).unwrap().acquired,
            7,
            "marking acquired preserves already over-acquired counts"
        );
        assert_eq!(doc.row(&RowKey::new(30, None)).unwrap().acquired, 0);

        // One undo reverts the bulk mark.
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&RowKey::new(10, None)).unwrap().acquired, 1);
        assert_eq!(doc.row(&RowKey::new(20, Some(true))).unwrap().acquired, 7);

        // Bulk reset clears selected rows to 0 in one undo step.
        apply(
            &doc,
            &mut undo,
            Edit::SetAcquiredMany {
                ids: vec![id10, id20, id30],
                acquired: false,
            },
        )
        .unwrap();
        assert_eq!(doc.row(&RowKey::new(10, None)).unwrap().acquired, 0);
        assert_eq!(doc.row(&RowKey::new(20, Some(true))).unwrap().acquired, 0);
        assert_eq!(doc.row(&RowKey::new(30, None)).unwrap().acquired, 0);

        // Repeating reset when already 0 does not push a no-op undo step.
        apply(
            &doc,
            &mut undo,
            Edit::SetAcquiredMany {
                ids: vec![id10, id20, id30],
                acquired: false,
            },
        )
        .unwrap();
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&RowKey::new(10, None)).unwrap().acquired, 1);
        assert_eq!(doc.row(&RowKey::new(20, Some(true))).unwrap().acquired, 7);
    }

    #[test]
    fn reset_all_acquired_clears_every_row_in_one_undo_step_for_restock() {
        let doc = ListDocument::from_rows(
            MetaSnapshot {
                name: "Raid supplies".into(),
                scope: None,
            },
            &[
                RowSnapshot {
                    key: RowKey::new(10, Some(true)),
                    need: 20,
                    acquired: 20,
                    target: None,
                },
                RowSnapshot {
                    key: RowKey::new(20, None),
                    need: 12,
                    acquired: 5,
                    target: None,
                },
                RowSnapshot {
                    key: RowKey::new(30, Some(false)),
                    need: 4,
                    acquired: 0,
                    target: None,
                },
            ],
        );
        let mut undo = ListUndo::with_merge_interval(&doc, 0);

        apply(&doc, &mut undo, Edit::ResetAllAcquired).unwrap();
        assert_eq!(doc.row(&RowKey::new(10, Some(true))).unwrap().acquired, 0);
        assert_eq!(doc.row(&RowKey::new(20, None)).unwrap().acquired, 0);
        assert_eq!(doc.row(&RowKey::new(30, Some(false))).unwrap().acquired, 0);
        // Needs stay intact for the restock run.
        assert_eq!(doc.row(&RowKey::new(10, Some(true))).unwrap().need, 20);
        assert_eq!(doc.row(&RowKey::new(20, None)).unwrap().need, 12);

        // No-op when nothing is owned: must not create an empty undo step.
        apply(&doc, &mut undo, Edit::ResetAllAcquired).unwrap();

        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&RowKey::new(10, Some(true))).unwrap().acquired, 20);
        assert_eq!(doc.row(&RowKey::new(20, None)).unwrap().acquired, 5);
        assert_eq!(doc.row(&RowKey::new(30, Some(false))).unwrap().acquired, 0);
    }

    #[test]
    fn craft_project_edits_and_reset_undo_cleanly() {
        use ultros_api_types::list::{AddCraftTarget, CraftChoiceInput};

        let doc = ListDocument::new();
        let mut undo = ListUndo::with_merge_interval(&doc, 0);

        apply(
            &doc,
            &mut undo,
            Edit::AddTargets(AddCraftTargetsRequest {
                targets: vec![AddCraftTarget {
                    recipe_id: 100,
                    count: 2,
                }],
                exclude_crystals: Some(true),
                include_vendors: Some(true),
                craft_choices: vec![CraftChoiceInput {
                    item_id: 200,
                    buy: false,
                    recipe_id: Some(101),
                    on_hand: 3,
                }],
            }),
        )
        .unwrap();

        assert_eq!(doc.targets().len(), 1);
        assert_eq!(doc.targets()[0].count, 2);
        assert_eq!(doc.craft_choice(200).unwrap().on_hand, 3);
        assert!(doc.craft_options().exclude_crystals);
        assert!(doc.craft_options().include_vendors);

        // Purchase a derived leaf with no explicit manual row (`need == 0`), then undo purchase.
        let leaf_key = RowKey::new(300, None);
        apply(
            &doc,
            &mut undo,
            Edit::RecordPurchase {
                key: leaf_key,
                quantity: 4,
                effective_need: Some(6),
            },
        )
        .unwrap();
        assert_eq!(doc.row(&leaf_key).unwrap().need, 0);
        assert_eq!(doc.row(&leaf_key).unwrap().acquired, 4);
        assert!(undo.can_undo_purchase());

        // Mark target completed & intermediate crafted, plus optimize choice.
        apply(
            &doc,
            &mut undo,
            Edit::AddTargetCompleted {
                recipe_id: 100,
                delta: 1,
            },
        )
        .unwrap();
        apply(
            &doc,
            &mut undo,
            Edit::AddIntermediateCrafted {
                item_id: 200,
                delta: 1,
            },
        )
        .unwrap();
        apply(
            &doc,
            &mut undo,
            Edit::ApplyOptimizedChoices(vec![(200, CraftMode::Buy, None)]),
        )
        .unwrap();
        assert_eq!(doc.craft_choice(200).unwrap().mode, CraftMode::Buy);
        assert!(undo.undo().unwrap());
        assert_eq!(doc.craft_choice(200).unwrap().mode, CraftMode::Craft);

        // ResetAllAcquired clears leaf acquired, target completed, and step crafted
        // while keeping on_hand and target count intact.
        apply(&doc, &mut undo, Edit::ResetAllAcquired).unwrap();
        assert_eq!(doc.row(&leaf_key).unwrap().acquired, 0);
        assert_eq!(doc.targets()[0].completed, 0);
        assert_eq!(doc.targets()[0].count, 2);
        assert_eq!(doc.craft_choice(200).unwrap().crafted, 0);
        assert_eq!(doc.craft_choice(200).unwrap().on_hand, 3);

        // One undo restores all reset progress together.
        assert!(undo.undo().unwrap());
        assert_eq!(doc.row(&leaf_key).unwrap().acquired, 4);
        assert_eq!(doc.targets()[0].completed, 1);
        assert_eq!(doc.craft_choice(200).unwrap().crafted, 1);
    }
}
