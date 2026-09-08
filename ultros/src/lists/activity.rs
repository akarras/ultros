//! Turn projected row changes into activity-feed entries (spec section 4.4),
//! with the same kinds, payload shapes and messages the REST handlers wrote.

use serde_json::json;
use ultros_api_types::list::ListActivityKind;
use ultros_db::list_doc::ProjectedChange;
use ultros_list_doc::{MetaSnapshot, RowChange};

/// More rows than this of one kind in a single merge collapse into one
/// summary entry, as bulk add and bulk HQ always did.
pub(crate) const BULK_THRESHOLD: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivityEntry {
    pub kind: ListActivityKind,
    pub list_item_id: Option<i32>,
    pub item_id: Option<i32>,
    pub payload: serde_json::Value,
    pub message: String,
}

fn row_payload(row: &ultros_db::entity::list_item::Model) -> serde_json::Value {
    json!({
        "quantity": row.quantity,
        "acquired": row.acquired,
        "hq": row.hq,
        "target_price": row.target_price,
    })
}

pub(crate) fn entries_for(
    actor_name: &str,
    changes: &[ProjectedChange],
    meta: Option<&MetaSnapshot>,
    item_name: impl Fn(i32) -> String,
) -> Vec<ActivityEntry> {
    let mut entries = Vec::new();
    let added: Vec<&ProjectedChange> = changes
        .iter()
        .filter(|c| matches!(c.change, RowChange::Added(_)))
        .collect();
    let removed: Vec<&ProjectedChange> = changes
        .iter()
        .filter(|c| matches!(c.change, RowChange::Removed(_)))
        .collect();
    let updated: Vec<&ProjectedChange> = changes
        .iter()
        .filter(|c| matches!(c.change, RowChange::Updated { .. }))
        .collect();

    if added.len() > BULK_THRESHOLD {
        entries.push(ActivityEntry {
            kind: ListActivityKind::ItemAdded,
            list_item_id: None,
            item_id: None,
            payload: json!({ "bulk": true, "count": added.len() }),
            message: format!("{actor_name} imported {} items", added.len()),
        });
    } else {
        for change in added {
            entries.push(ActivityEntry {
                kind: ListActivityKind::ItemAdded,
                list_item_id: Some(change.row.id),
                item_id: Some(change.row.item_id),
                payload: row_payload(&change.row),
                message: format!("{actor_name} added {}", item_name(change.row.item_id)),
            });
        }
    }

    if removed.len() > BULK_THRESHOLD {
        entries.push(ActivityEntry {
            kind: ListActivityKind::ItemsRemoved,
            list_item_id: None,
            item_id: None,
            payload: json!({ "count": removed.len() }),
            message: format!("{actor_name} removed {} items", removed.len()),
        });
    } else {
        for change in removed {
            entries.push(ActivityEntry {
                kind: ListActivityKind::ItemRemoved,
                list_item_id: Some(change.row.id),
                item_id: Some(change.row.item_id),
                payload: row_payload(&change.row),
                message: format!("{actor_name} removed {}", item_name(change.row.item_id)),
            });
        }
    }

    if updated.len() > BULK_THRESHOLD {
        entries.push(ActivityEntry {
            kind: ListActivityKind::ItemUpdated,
            list_item_id: None,
            item_id: None,
            payload: json!({ "bulk": true, "count": updated.len() }),
            message: format!("{actor_name} updated {} items", updated.len()),
        });
    } else {
        for change in updated {
            let RowChange::Updated { before, after } = &change.change else {
                continue;
            };
            let mut diff = serde_json::Map::new();
            if before.need != after.need {
                diff.insert("quantity".into(), json!([before.need, after.need]));
            }
            if before.acquired != after.acquired {
                diff.insert("acquired".into(), json!([before.acquired, after.acquired]));
            }
            if before.target != after.target {
                diff.insert("target_price".into(), json!([before.target, after.target]));
            }
            let acquired_now = after.acquired >= after.need && before.acquired < before.need;
            let name = item_name(change.row.item_id);
            entries.push(ActivityEntry {
                kind: if acquired_now {
                    ListActivityKind::ItemAcquired
                } else {
                    ListActivityKind::ItemUpdated
                },
                list_item_id: Some(change.row.id),
                item_id: Some(change.row.item_id),
                payload: serde_json::Value::Object(diff),
                message: if acquired_now {
                    format!("{actor_name} got {name}")
                } else {
                    format!("{actor_name} updated {name}")
                },
            });
        }
    }

    if let Some(meta) = meta {
        entries.push(ActivityEntry {
            kind: ListActivityKind::ListUpdated,
            list_item_id: None,
            item_id: None,
            payload: json!({ "name": meta.name }),
            message: format!("{actor_name} updated list {}", meta.name),
        });
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultros_db::entity::list_item;
    use ultros_list_doc::{RowKey, RowSnapshot};

    fn row(id: i32, item: i32) -> list_item::Model {
        list_item::Model {
            id,
            item_id: item,
            list_id: 1,
            hq: None,
            quantity: Some(2),
            acquired: Some(0),
            target_price: None,
        }
    }

    fn snapshot(item: i32, need: i64, acquired: i64) -> RowSnapshot {
        RowSnapshot {
            key: RowKey::new(item, None),
            need,
            acquired,
            target: None,
        }
    }

    #[test]
    fn single_changes_get_one_entry_each_with_the_legacy_shapes() {
        let changes = vec![
            ProjectedChange {
                change: RowChange::Added(snapshot(5, 2, 0)),
                row: row(11, 5),
            },
            ProjectedChange {
                change: RowChange::Updated {
                    before: snapshot(6, 2, 1),
                    after: snapshot(6, 2, 2),
                },
                row: row(12, 6),
            },
            ProjectedChange {
                change: RowChange::Removed(snapshot(7, 1, 0)),
                row: row(13, 7),
            },
        ];
        let entries = entries_for("Aaron", &changes, None, |id| format!("item{id}"));
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].kind, ListActivityKind::ItemAdded);
        assert_eq!(entries[0].message, "Aaron added item5");
        assert_eq!(entries[0].payload["quantity"], 2);
        assert_eq!(entries[1].kind, ListActivityKind::ItemRemoved);
        assert_eq!(entries[2].kind, ListActivityKind::ItemAcquired);
        assert_eq!(entries[2].message, "Aaron got item6");
        assert_eq!(entries[2].payload["acquired"], serde_json::json!([1, 2]));
        assert!(entries[2].payload.get("quantity").is_none());
    }

    #[test]
    fn a_partial_acquire_is_an_update_and_meta_adds_list_updated() {
        let changes = vec![ProjectedChange {
            change: RowChange::Updated {
                before: snapshot(6, 5, 1),
                after: snapshot(6, 5, 2),
            },
            row: row(12, 6),
        }];
        let meta = MetaSnapshot {
            name: "Glamour".into(),
            scope: None,
        };
        let entries = entries_for("Aaron", &changes, Some(&meta), |_| "x".into());
        assert_eq!(entries[0].kind, ListActivityKind::ItemUpdated);
        assert_eq!(entries[1].kind, ListActivityKind::ListUpdated);
        assert_eq!(entries[1].message, "Aaron updated list Glamour");
    }

    #[test]
    fn more_than_ten_of_a_kind_collapse_into_a_summary() {
        let changes: Vec<ProjectedChange> = (0..11)
            .map(|i| ProjectedChange {
                change: RowChange::Added(snapshot(i, 1, 0)),
                row: row(100 + i, i),
            })
            .collect();
        let entries = entries_for("Aaron", &changes, None, |_| "x".into());
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].payload,
            serde_json::json!({ "bulk": true, "count": 11 })
        );
        assert_eq!(entries[0].message, "Aaron imported 11 items");
    }

    /// Exactly `BULK_THRESHOLD` rows of one kind must NOT collapse: the
    /// threshold is a strict `>`, not `>=`. Pins that boundary.
    #[test]
    fn exactly_bulk_threshold_added_rows_do_not_collapse() {
        let changes: Vec<ProjectedChange> = (0..BULK_THRESHOLD as i32)
            .map(|i| ProjectedChange {
                change: RowChange::Added(snapshot(i, 1, 0)),
                row: row(100 + i, i),
            })
            .collect();
        let entries = entries_for("Aaron", &changes, None, |_| "x".into());
        assert_eq!(entries.len(), BULK_THRESHOLD, "no collapse at the boundary");
        assert!(
            entries
                .iter()
                .all(|e| e.kind == ListActivityKind::ItemAdded && e.list_item_id.is_some())
        );
    }
}
