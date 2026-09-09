//! Pure document mutation for a full-row `ListItem` PUT (Task 5 fix F1).
//!
//! `edit_list_item` previously called `set_quality` (which merges into an
//! existing destination row by summing `need` and incrementing `acquired`)
//! and then unconditionally called `set_need`/`set_acquired`/`set_target`
//! with the request's own values — silently overwriting whatever the merge
//! had just produced. An HQ toggle onto a row that already existed at the
//! destination quality lost the destination row's quantity every time.

use ultros_api_types::list::ListItem;
use ultros_db::entity::list_item;
use ultros_list_doc::{DocError, ListDocument, Quality, RowKey};

/// Applies a full-row PUT to the document. Only fields the request actually
/// changed relative to `before` are written, so an HQ toggle that merges into
/// an existing row keeps that row's merged quantity/acquired/target.
pub(crate) fn apply_list_item_edit(
    doc: &ListDocument,
    before: &list_item::Model,
    item: &ListItem,
) -> Result<RowKey, DocError> {
    let old_key = RowKey::new(before.item_id, before.hq);
    let new_quality = Quality::from(item.hq);
    let key = if new_quality != old_key.quality {
        doc.set_quality(&old_key, new_quality)?
    } else {
        old_key
    };

    let old_need = before.quantity.unwrap_or(1) as i64;
    let new_need = item.quantity.unwrap_or(1) as i64;
    if new_need != old_need {
        doc.set_need(&key, new_need)?;
    }

    let old_acquired = before.acquired.unwrap_or(0) as i64;
    let new_acquired = item.acquired.unwrap_or(0) as i64;
    if new_acquired != old_acquired {
        doc.set_acquired(&key, new_acquired)?;
    }

    if item.target_price != before.target_price {
        doc.set_target(&key, item.target_price)?;
    }

    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(item_id: i32, hq: Option<bool>, quantity: i32, acquired: i32) -> list_item::Model {
        list_item::Model {
            id: 1,
            item_id,
            list_id: 1,
            hq,
            quantity: Some(quantity),
            acquired: Some(acquired),
            target_price: None,
        }
    }

    fn edit(item_id: i32, hq: Option<bool>, quantity: i32, acquired: i32) -> ListItem {
        ListItem {
            id: 1,
            item_id,
            list_id: 1,
            hq,
            quantity: Some(quantity),
            acquired: Some(acquired),
            ..Default::default()
        }
    }

    /// (a) An HQ toggle onto an existing HQ row with an unchanged quantity
    /// keeps the destination row's merged sum: `set_quality` already summed
    /// need, and since the request didn't change quantity, `set_need` must
    /// not fire and clobber that sum.
    #[test]
    fn hq_toggle_onto_existing_row_keeps_merged_sum() {
        let doc = ListDocument::new();
        doc.add_row(RowKey::new(1, Some(false)), 2, None).unwrap();
        doc.add_row(RowKey::new(1, Some(true)), 5, None).unwrap();

        let before = model(1, Some(false), 2, 0);
        let item = edit(1, Some(true), 2, 0);

        let key = apply_list_item_edit(&doc, &before, &item).unwrap();
        assert_eq!(key, RowKey::new(1, Some(true)));
        let row = doc.row(&key).unwrap();
        assert_eq!(row.need, 7, "merged need must be the sum, not overwritten");
    }

    /// (b) An HQ toggle where the request also changes the quantity writes
    /// the new quantity, discarding the just-merged sum on purpose — the
    /// request's own value wins when it actually differs from `before`.
    #[test]
    fn hq_toggle_with_changed_quantity_writes_new_quantity() {
        let doc = ListDocument::new();
        doc.add_row(RowKey::new(1, Some(false)), 2, None).unwrap();
        doc.add_row(RowKey::new(1, Some(true)), 5, None).unwrap();

        let before = model(1, Some(false), 2, 0);
        let item = edit(1, Some(true), 3, 0);

        let key = apply_list_item_edit(&doc, &before, &item).unwrap();
        let row = doc.row(&key).unwrap();
        assert_eq!(row.need, 3);
    }

    /// (c) No key move: a changed `acquired` writes only `acquired`.
    #[test]
    fn no_key_move_changed_acquired_writes_only_acquired() {
        let doc = ListDocument::new();
        doc.add_row(RowKey::new(1, None), 4, None).unwrap();

        let before = model(1, None, 4, 0);
        let item = edit(1, None, 4, 3);

        let key = apply_list_item_edit(&doc, &before, &item).unwrap();
        let row = doc.row(&key).unwrap();
        assert_eq!(row.need, 4);
        assert_eq!(row.acquired, 3);
        assert_eq!(row.target, None);
    }

    /// (d) A target set on a row is written.
    #[test]
    fn target_set_on_row_is_written() {
        let doc = ListDocument::new();
        doc.add_row(RowKey::new(1, None), 4, None).unwrap();

        let before = model(1, None, 4, 0);
        let mut item = edit(1, None, 4, 0);
        item.target_price = Some(100);

        let key = apply_list_item_edit(&doc, &before, &item).unwrap();
        let row = doc.row(&key).unwrap();
        assert_eq!(row.target, Some(100));
    }
}
