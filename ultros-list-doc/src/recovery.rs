//! Recovery after the server has discarded history an offline peer needs.
//!
//! The persisted Loro snapshot is the durable intent log: read its historical state at
//! the intersection with the server's accepted version, then replay only the
//! changes since that shared base. No acknowledgement sidecar can drift from
//! the snapshot, including when several tabs merge their offline work.

use crate::{DocError, ListDocument, RowChange, RowKey, diff_rows};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictField {
    Row,
    Need,
    Target,
    Name,
    Scope,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryConflict {
    pub key: Option<RowKey>,
    pub field: ConflictField,
}

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error("the shared history is unavailable; local changes need review")]
    HistoryUnavailable,
    #[error("local and server changes need review")]
    Conflicts(Vec<RecoveryConflict>),
    #[error(transparent)]
    Document(#[from] DocError),
}

pub struct RecoveredDocument {
    pub document: ListDocument,
    pub changed: bool,
    pub dropped_meta: bool,
}

/// Build a replacement without mutating either source. All conflicts are
/// checked before applying anything; a quality move (delete + add/merge) is
/// therefore recovered as a whole, or left intact for the player to review.
pub fn recover(
    local: &ListDocument,
    server_snapshot: &[u8],
    keep_local_meta: bool,
) -> Result<RecoveredDocument, RecoveryError> {
    let fresh = ListDocument::from_snapshot(server_snapshot)?;
    let base = local
        .common_base(&fresh)
        .ok_or(RecoveryError::HistoryUnavailable)?;
    // Unrelated nonempty documents are not an offline branch of this list.
    if base.schema().is_none() && (!local.rows().is_empty() || !local.meta().name.is_empty()) {
        return Err(RecoveryError::HistoryUnavailable);
    }
    let changes = diff_rows(&base.rows(), &local.rows());
    let mut conflicts = Vec::new();
    for change in &changes {
        let key = change.key();
        let remote = fresh.row(&key);
        let mut conflict = |field| {
            conflicts.push(RecoveryConflict {
                key: Some(key),
                field,
            })
        };
        match change {
            // Even identical additions may carry independent acquired counts.
            // Without a shared row identity, do not guess how to combine them.
            RowChange::Added(_) if remote.is_some() => conflict(ConflictField::Row),
            RowChange::Removed(before) => {
                if remote.is_some_and(|row| row != *before)
                    || (remote.is_some() && !base.same_row_identity(&fresh, &key))
                {
                    conflict(ConflictField::Row);
                }
            }
            RowChange::Updated { before, after } => match remote {
                Some(remote)
                    if base.same_row_identity(&fresh, &key)
                        && base.same_row_identity(local, &key) =>
                {
                    if clashes(before.need, after.need, remote.need) {
                        conflict(ConflictField::Need);
                    }
                    if clashes(before.target, after.target, remote.target) {
                        conflict(ConflictField::Target);
                    }
                    // Acquired is additive. Replay only this branch's delta,
                    // including negative corrections/undo, never its old total.
                    remote
                        .acquired
                        .checked_add(
                            after
                                .acquired
                                .checked_sub(before.acquired)
                                .ok_or(DocError::QuantityOverflow)?,
                        )
                        .ok_or(DocError::QuantityOverflow)?;
                }
                _ => conflict(ConflictField::Row),
            },
            _ => {}
        }
    }
    let base_meta = base.meta();
    let local_meta = local.meta();
    let remote_meta = fresh.meta();
    if keep_local_meta {
        if clashes(&base_meta.name, &local_meta.name, &remote_meta.name) {
            conflicts.push(RecoveryConflict {
                key: None,
                field: ConflictField::Name,
            });
        }
        if clashes(base_meta.scope, local_meta.scope, remote_meta.scope) {
            conflicts.push(RecoveryConflict {
                key: None,
                field: ConflictField::Scope,
            });
        }
    }
    if !conflicts.is_empty() {
        return Err(RecoveryError::Conflicts(conflicts));
    }
    let server_version = fresh.version();
    for change in changes {
        match change {
            RowChange::Added(row) => {
                fresh.add_row(row.key, row.need, row.target)?;
                fresh.add_acquired(&row.key, row.acquired)?;
            }
            RowChange::Removed(row) => {
                if fresh.row(&row.key).is_some() {
                    fresh.remove_row(&row.key)?;
                }
            }
            RowChange::Updated { before, after } => {
                if before.need != after.need
                    && fresh.row(&after.key).is_some_and(|r| r.need != after.need)
                {
                    fresh.set_need(&after.key, after.need)?;
                }
                if before.target != after.target
                    && fresh
                        .row(&after.key)
                        .is_some_and(|r| r.target != after.target)
                {
                    fresh.set_target(&after.key, after.target)?;
                }
                fresh.add_acquired(
                    &after.key,
                    after
                        .acquired
                        .checked_sub(before.acquired)
                        .ok_or(DocError::QuantityOverflow)?,
                )?;
            }
        }
    }
    if keep_local_meta {
        if base_meta.name != local_meta.name && remote_meta.name != local_meta.name {
            fresh.rename(&local_meta.name)?;
        }
        if base_meta.scope != local_meta.scope
            && remote_meta.scope != local_meta.scope
            && let Some(scope) = local_meta.scope
        {
            fresh.set_scope(scope)?;
        }
    }
    Ok(RecoveredDocument {
        changed: fresh.is_ahead_of(&server_version),
        document: fresh,
        dropped_meta: !keep_local_meta && base_meta != local_meta,
    })
}

fn clashes<T: PartialEq>(base: T, local: T, remote: T) -> bool {
    local != base && remote != base && local != remote
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MetaSnapshot, Quality, RowSnapshot};
    use ultros_api_types::world_helper::AnySelector;

    fn peers() -> (ListDocument, ListDocument, RowKey) {
        let key = RowKey::new(1, None);
        let server = ListDocument::from_rows(
            MetaSnapshot {
                name: "Shared list".into(),
                scope: Some(AnySelector::World(1)),
            },
            &[RowSnapshot {
                key,
                need: 10,
                acquired: 0,
                target: Some(100),
            }],
        );
        let local = ListDocument::from_snapshot(&server.export_snapshot().unwrap()).unwrap();
        (server, local, key)
    }

    /// Exercise durable reload, server compaction and acceptance of the rebased
    /// operations, not just a three-way diff of hand-written projections.
    fn reconnect(
        server: &ListDocument,
        local: &ListDocument,
        keep_meta: bool,
    ) -> Result<RecoveredDocument, RecoveryError> {
        let local = ListDocument::from_snapshot(&local.export_snapshot().unwrap()).unwrap();
        // A fresh peer makes a harmless metadata commit after the remote
        // edits. The shallow root must be AFTER those edits, not their own
        // retained change; otherwise there may be no missing history to test.
        let tip = ListDocument::from_snapshot(&server.export_snapshot().unwrap()).unwrap();
        let before = tip.version();
        let name = tip.meta().name;
        tip.rename(&format!("{name} (compaction fixture)")).unwrap();
        tip.rename(&name).unwrap();
        assert!(tip.is_ahead_of(&before));
        let compact = ListDocument::from_snapshot(&tip.export_shallow().unwrap()).unwrap();
        let recovered = recover(&local, &compact.export_snapshot().unwrap(), keep_meta)?;
        assert!(
            !compact
                .import(&recovered.document.export_since(&compact.version()).unwrap())
                .unwrap()
                .pending
        );
        assert_eq!(compact.rows(), recovered.document.rows());
        assert_eq!(compact.meta(), recovered.document.meta());
        Ok(recovered)
    }

    #[test]
    fn offline_add_keeps_remote_purchases_edits_additions_and_metadata() {
        let (server, local, key) = peers();
        local.add_row(RowKey::new(2, None), 3, None).unwrap();
        server.add_acquired(&key, 4).unwrap();
        server.set_need(&key, 12).unwrap();
        server.set_target(&key, None).unwrap();
        server.add_row(RowKey::new(3, None), 5, None).unwrap();
        server.rename("Remote name").unwrap();
        server.set_scope(AnySelector::World(2)).unwrap();
        let recovered = reconnect(&server, &local, true).unwrap();
        assert!(recovered.changed);
        assert_eq!(recovered.document.row(&key), server.row(&key));
        assert_eq!(recovered.document.meta(), server.meta());
        assert_eq!(recovered.document.rows().len(), 3);
    }

    #[test]
    fn acquired_deltas_merge_in_both_directions_without_double_replaying_accepted_work() {
        let (server, local, key) = peers();
        local.add_acquired(&key, 2).unwrap();
        server
            .import(&local.export_since(&server.version()).unwrap())
            .unwrap();
        local.add_acquired(&key, -1).unwrap();
        server.add_acquired(&key, 4).unwrap();
        let recovered = reconnect(&server, &local, true).unwrap();
        assert_eq!(recovered.document.row(&key).unwrap().acquired, 5);
        let accepted = reconnect(&recovered.document, &recovered.document, true).unwrap();
        assert!(!accepted.changed);
        assert_eq!(accepted.document.row(&key).unwrap().acquired, 5);
    }

    #[test]
    fn disjoint_fields_merge_but_different_values_for_the_same_field_need_review() {
        let (server, local, key) = peers();
        local.set_need(&key, 15).unwrap();
        server.set_target(&key, Some(50)).unwrap();
        let recovered = reconnect(&server, &local, true).unwrap();
        assert_eq!(recovered.document.row(&key).unwrap().need, 15);
        assert_eq!(recovered.document.row(&key).unwrap().target, Some(50));
        server.set_need(&key, 20).unwrap();
        let before = local.version();
        assert!(
            matches!(reconnect(&server, &local, true), Err(RecoveryError::Conflicts(fields)) if fields == vec![RecoveryConflict { key: Some(key), field: ConflictField::Need }])
        );
        assert_eq!(before, local.version());
        assert_eq!(server.row(&key).unwrap().need, 20);
        server.set_need(&key, 15).unwrap();
        assert_eq!(
            reconnect(&server, &local, true)
                .unwrap()
                .document
                .row(&key)
                .unwrap()
                .need,
            15
        );
    }

    #[test]
    fn local_delete_survives_unrelated_remote_edits_but_not_a_remote_edit_to_that_row() {
        let (server, local, key) = peers();
        local.remove_row(&key).unwrap();
        server.add_row(RowKey::new(2, None), 1, None).unwrap();
        let recovered = reconnect(&server, &local, true).unwrap();
        assert!(recovered.document.row(&key).is_none());
        assert!(recovered.document.row(&RowKey::new(2, None)).is_some());
        server.add_acquired(&key, 4).unwrap();
        assert!(matches!(
            reconnect(&server, &local, true),
            Err(RecoveryError::Conflicts(_))
        ));
    }

    #[test]
    fn remote_delete_is_kept_unless_local_edited_that_row() {
        let (server, local, key) = peers();
        server.remove_row(&key).unwrap();
        local.add_row(RowKey::new(2, None), 1, None).unwrap();
        assert!(
            reconnect(&server, &local, true)
                .unwrap()
                .document
                .row(&key)
                .is_none()
        );
        local.set_need(&key, 11).unwrap();
        assert!(matches!(
            reconnect(&server, &local, true),
            Err(RecoveryError::Conflicts(_))
        ));
    }

    #[test]
    fn quality_move_is_atomic_including_a_merge_into_an_existing_quality() {
        for existing in [false, true] {
            let (server, local, key) = peers();
            let hq = RowKey::new(1, Some(true));
            if existing {
                server.add_row(hq, 5, None).unwrap();
                server.add_acquired(&hq, 1).unwrap();
                local
                    .import(&server.export_since(&local.version()).unwrap())
                    .unwrap();
            }
            local.set_quality(&key, Quality::Hq).unwrap();
            server.rename("Another name").unwrap();
            let recovered = reconnect(&server, &local, true).unwrap();
            assert!(recovered.document.row(&key).is_none());
            assert_eq!(recovered.document.row(&hq), local.row(&hq));
            server.add_acquired(&key, 4).unwrap();
            assert!(matches!(
                reconnect(&server, &local, true),
                Err(RecoveryError::Conflicts(_))
            ));
            assert_eq!(server.row(&key).unwrap().acquired, 4);
            assert_eq!(server.row(&hq).is_some(), existing);
        }
    }

    #[test]
    fn replaced_rows_and_concurrent_adds_are_not_silently_combined() {
        let (server, local, key) = peers();
        local.add_acquired(&key, 1).unwrap();
        server.remove_row(&key).unwrap();
        server.add_row(key, 10, Some(100)).unwrap();
        assert!(matches!(
            reconnect(&server, &local, true),
            Err(RecoveryError::Conflicts(_))
        ));
        let key2 = RowKey::new(2, None);
        local.add_row(key2, 3, None).unwrap();
        server.add_row(key2, 3, None).unwrap();
        assert!(
            matches!(reconnect(&server, &local, true), Err(RecoveryError::Conflicts(fields)) if fields.len() == 2)
        );
    }

    #[test]
    fn owner_metadata_conflicts_are_reviewed_and_forbidden_metadata_is_dropped_without_losing_rows()
    {
        let (server, local, key) = peers();
        local.rename("Local name").unwrap();
        local.set_scope(AnySelector::World(3)).unwrap();
        local.set_need(&key, 12).unwrap();
        server.rename("Server name").unwrap();
        server.set_scope(AnySelector::World(2)).unwrap();
        assert!(
            matches!(reconnect(&server, &local, true), Err(RecoveryError::Conflicts(fields)) if fields.len() == 2)
        );
        let recovered = reconnect(&server, &local, false).unwrap();
        assert!(recovered.dropped_meta);
        assert_eq!(recovered.document.meta(), server.meta());
        assert_eq!(recovered.document.row(&key).unwrap().need, 12);
    }

    #[test]
    fn unavailable_local_history_preserves_both_copies_for_review() {
        let (server, local, key) = peers();
        local.set_need(&key, 15).unwrap();
        let local = ListDocument::from_snapshot(&local.export_shallow().unwrap()).unwrap();
        server.add_acquired(&key, 4).unwrap();
        let before = local.version();
        assert!(matches!(
            reconnect(&server, &local, true),
            Err(RecoveryError::HistoryUnavailable)
        ));
        assert_eq!(local.version(), before);
        assert_eq!(server.row(&key).unwrap().acquired, 4);
        assert_eq!(local.row(&key).unwrap().need, 15);
    }

    #[test]
    fn a_client_loaded_from_an_earlier_shallow_root_still_has_its_offline_intent() {
        let (server, _, key) = peers();
        let local = ListDocument::from_snapshot(&server.export_shallow().unwrap()).unwrap();
        local.set_need(&key, 15).unwrap();
        server.add_acquired(&key, 4).unwrap();
        let recovered = reconnect(&server, &local, true).unwrap();
        assert_eq!(recovered.document.row(&key).unwrap().need, 15);
        assert_eq!(recovered.document.row(&key).unwrap().acquired, 4);
    }

    #[test]
    fn offline_undo_and_peer_tab_imports_use_the_same_durable_history() {
        let (server, local, key) = peers();
        let mut undo = crate::ListUndo::new(&local);
        local.set_need(&key, 15).unwrap();
        assert!(undo.undo().unwrap());
        let tab = ListDocument::from_snapshot(&local.export_snapshot().unwrap()).unwrap();
        tab.add_row(RowKey::new(2, None), 2, None).unwrap();
        local
            .import(&tab.export_since(&local.version()).unwrap())
            .unwrap();
        server.set_need(&key, 20).unwrap();
        let recovered = reconnect(&server, &local, true).unwrap();
        assert_eq!(recovered.document.row(&key).unwrap().need, 20);
        assert!(recovered.document.row(&RowKey::new(2, None)).is_some());
    }

    #[test]
    fn an_owner_name_edit_preserves_a_remote_scope_edit() {
        let (server, local, _) = peers();
        local.rename("Offline name").unwrap();
        server.set_scope(AnySelector::World(2)).unwrap();
        let recovered = reconnect(&server, &local, true).unwrap();
        assert_eq!(recovered.document.meta().name, "Offline name");
        assert_eq!(recovered.document.meta().scope, Some(AnySelector::World(2)));
        assert!(!recovered.dropped_meta);
    }
}
