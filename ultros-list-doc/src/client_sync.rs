//! Framework-independent handshake/recovery decisions shared by browser and native clients.
use crate::{
    DocError, ImportReport, ListDocument, SyncPayload, document::versions_equal, recovery,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandshakePlan {
    Converged,
    SendDiff,
    Recover,
    Stop,
}

/// Causal version, original pending update, and whether to save before sending.
pub type PreparedSync = (Vec<u8>, Option<Vec<u8>>, bool);
pub fn handshake_plan(
    last: Option<&[u8]>,
    rebased: Option<&[u8]>,
    version: &[u8],
    snapshot: bool,
    ahead: bool,
) -> HandshakePlan {
    if !ahead {
        HandshakePlan::Converged
    } else if snapshot && rebased.is_some_and(|previous| versions_equal(previous, version)) {
        HandshakePlan::Stop
    } else if snapshot && last.is_some_and(|previous| versions_equal(previous, version)) {
        HandshakePlan::Recover
    } else {
        HandshakePlan::SendDiff
    }
}
pub fn import_server_snapshot(
    local: &ListDocument,
    bytes: &[u8],
) -> Result<ImportReport, DocError> {
    let server = ListDocument::from_snapshot(bytes)?;
    if (!local.rows().is_empty() || !local.meta().name.is_empty())
        && local
            .common_base(&server)
            .is_none_or(|base| !base.shares_history(&server))
    {
        return Ok(ImportReport { pending: true });
    }
    if !server.can_export_since(&local.version()) {
        return Ok(ImportReport { pending: true });
    }
    let copy = ListDocument::from_snapshot(&local.export_snapshot()?)?;
    match copy.import(bytes) {
        Ok(ImportReport { pending: true }) | Err(DocError::OutdatedDependency) => {
            Ok(ImportReport { pending: true })
        }
        Err(error) => Err(error),
        Ok(_) => local.import(bytes),
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientSync {
    pub last_diff_version: Option<Vec<u8>>,
    pub rebased_for: Option<Vec<u8>>,
    pub server_version: Option<Vec<u8>>,
    pub ready: bool,
    pub force_snapshot: bool,
    pub metadata_forbidden: bool,
    pub review: bool,
    #[serde(skip)]
    pub awaiting_persist: bool,
    #[serde(skip)]
    pub review_server: Option<Vec<u8>>,
}
pub struct SyncStep {
    pub replacement: Option<ListDocument>,
    pub status: &'static str,
}
impl ClientSync {
    pub fn can_edit(&self) -> bool {
        self.ready && !self.review && !self.awaiting_persist
    }
    pub fn local_changed(&mut self) {
        self.awaiting_persist = true;
    }
    pub fn persisted(&mut self) {
        self.awaiting_persist = false;
    }
    pub fn error(&mut self, code: &str) {
        if matches!(code, "metadata_forbidden" | "missing_history") {
            self.force_snapshot = true;
            self.metadata_forbidden = code == "metadata_forbidden";
        }
    }
    pub fn prepare(&mut self, doc: &ListDocument) -> Result<PreparedSync, DocError> {
        if self.review {
            return Err(DocError::InvalidStructure(
                "recovery needs review; both copies are preserved".into(),
            ));
        }
        if self.awaiting_persist {
            return Err(DocError::InvalidStructure(
                "save local changes before syncing".into(),
            ));
        }
        if self.force_snapshot || !self.ready {
            return Ok((Vec::new(), None, false));
        }
        let mut save = false;
        let update = if let Some(version) = &self.server_version {
            if doc.is_ahead_of(version) {
                let update = doc.export_since(version)?;
                save = !self
                    .last_diff_version
                    .as_deref()
                    .is_some_and(|previous| versions_equal(previous, version));
                self.last_diff_version = Some(version.clone());
                Some(update)
            } else {
                None
            }
        } else {
            None
        };
        self.awaiting_persist = save;
        Ok((doc.version(), update, save))
    }
    fn rebase(
        &mut self,
        local: &ListDocument,
        bytes: &[u8],
        version: &[u8],
    ) -> Result<SyncStep, DocError> {
        match recovery::recover(local, bytes, false) {
            Ok(recovered) => {
                self.rebased_for = Some(version.to_vec());
                self.metadata_forbidden = false;
                self.server_version = Some(version.to_vec());
                self.ready = true;
                self.force_snapshot = false;
                self.awaiting_persist = true;
                Ok(SyncStep {
                    replacement: Some(recovered.document),
                    status: "save_recovery",
                })
            }
            Err(_) => {
                self.review = true;
                self.review_server = Some(bytes.to_vec());
                self.awaiting_persist = true;
                Ok(SyncStep {
                    replacement: None,
                    status: "review",
                })
            }
        }
    }
    pub fn receive(
        &mut self,
        local: &ListDocument,
        version: &[u8],
        payload: SyncPayload,
    ) -> Result<SyncStep, DocError> {
        if self.awaiting_persist || self.review {
            return Err(DocError::InvalidStructure(
                "save or review recovery before receiving more data".into(),
            ));
        }
        let snapshot = match &payload {
            SyncPayload::Snapshot(bytes) => {
                let imported = import_server_snapshot(local, bytes)?;
                if imported.pending {
                    return self.rebase(local, bytes, version);
                }
                Some(bytes.as_slice())
            }
            SyncPayload::Updates(bytes) => {
                if local.import(bytes)?.pending {
                    self.force_snapshot = true;
                    return Ok(SyncStep {
                        replacement: None,
                        status: "snapshot_required",
                    });
                }
                None
            }
            SyncPayload::UpToDate => None,
        };
        if self
            .rebased_for
            .as_deref()
            .is_some_and(|v| !versions_equal(v, version))
        {
            self.rebased_for = None;
        }
        self.ready = true;
        self.force_snapshot = false;
        self.server_version = Some(version.to_vec());
        match handshake_plan(
            self.last_diff_version.as_deref(),
            self.rebased_for.as_deref(),
            version,
            snapshot.is_some(),
            local.is_ahead_of(version),
        ) {
            HandshakePlan::Stop => {
                self.review = true;
                self.review_server = snapshot.map(<[u8]>::to_vec);
            }
            HandshakePlan::Recover => {
                let snapshot = snapshot.ok_or_else(|| {
                    DocError::InvalidStructure("snapshot required for recovery".into())
                })?;
                return self.rebase(local, snapshot, version);
            }
            HandshakePlan::Converged => {
                self.last_diff_version = None;
                self.rebased_for = None;
            }
            HandshakePlan::SendDiff => {}
        }
        let replacement = if self.review {
            None
        } else {
            local.compact_acknowledged(version)?
        };
        self.awaiting_persist = true;
        Ok(SyncStep {
            replacement,
            status: if self.review {
                "review"
            } else {
                "save_received"
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MetaSnapshot, RowKey};
    #[test]
    fn reordered_legacy_versions_cannot_restart_rejected_recovery() {
        // Postcard maps differ on the wire but represent the same two peers.
        let ascending = [2, 1, 2, 2, 4];
        let descending = [2, 2, 4, 1, 2];
        assert_ne!(ascending, descending);
        assert!(versions_equal(&ascending, &descending));
        assert_eq!(
            handshake_plan(Some(&ascending), None, &descending, true, true),
            HandshakePlan::Recover
        );
        assert_eq!(
            handshake_plan(Some(&ascending), Some(&ascending), &descending, true, true),
            HandshakePlan::Stop
        );
        assert!(!versions_equal(&ascending, &[2, 1, 2, 2, 6]));
    }
    #[test]
    fn lost_send_retries_original_diff_and_recovery_waits_for_durable_save() {
        let server = ListDocument::new();
        server.rename("Server").unwrap();
        server.add_row(RowKey::new(1, None), 5, None).unwrap();
        let local = ListDocument::empty_peer();
        let mut client = ClientSync::default();
        client
            .receive(
                &local,
                &server.version(),
                SyncPayload::Snapshot(server.export_snapshot().unwrap()),
            )
            .unwrap();
        assert!(client.prepare(&local).is_err());
        client.persisted();
        local.add_acquired(&RowKey::new(1, None), 2).unwrap();
        client.local_changed();
        assert!(client.prepare(&local).is_err());
        client.persisted();
        let (_, first, save) = client.prepare(&local).unwrap();
        assert!(save);
        client.persisted();
        client
            .receive(&local, &server.version(), SyncPayload::UpToDate)
            .unwrap();
        client.persisted();
        let (_, retry, _) = client.prepare(&local).unwrap();
        assert_eq!(first, retry);
        let restored: ClientSync =
            serde_json::from_str(&serde_json::to_string(&client).unwrap()).unwrap();
        assert_eq!(restored.last_diff_version, client.last_diff_version);
        server.import(&first.unwrap()).unwrap();
        server.import(&retry.unwrap()).unwrap();
        assert_eq!(server.rows()[0].acquired, 2);
    }
    #[test]
    fn repeated_rejected_recovery_stops_but_a_lost_ack_can_retry() {
        let v = b"server";
        assert_eq!(
            handshake_plan(Some(v), Some(v), v, true, true),
            HandshakePlan::Stop
        );
        assert_eq!(
            handshake_plan(Some(v), Some(v), v, false, true),
            HandshakePlan::SendDiff
        );
        assert_eq!(
            handshake_plan(Some(v), None, v, true, true),
            HandshakePlan::Recover
        );
        assert_eq!(
            handshake_plan(None, None, v, true, false),
            HandshakePlan::Converged
        );
    }
    #[test]
    fn unrelated_snapshot_preserves_both_documents_for_review() {
        let local = ListDocument::new();
        local.rename("Local").unwrap();
        local.add_row(RowKey::new(1, None), 1, None).unwrap();
        let remote = ListDocument::from_rows(
            MetaSnapshot {
                name: "Other".into(),
                scope: None,
            },
            &[],
        );
        let mut state = ClientSync {
            ready: true,
            last_diff_version: Some(remote.version()),
            ..Default::default()
        };
        let snapshot = remote.export_snapshot().unwrap();
        state
            .receive(
                &local,
                &remote.version(),
                SyncPayload::Snapshot(snapshot.clone()),
            )
            .unwrap();
        assert!(state.review);
        assert_eq!(state.review_server, Some(snapshot));
        assert_eq!(local.rows().len(), 1);
    }
    #[test]
    fn a_new_receiver_can_load_a_compacted_server_without_inventing_local_intent() {
        let server = ListDocument::new();
        server.add_row(RowKey::new(2, None), 3, None).unwrap();
        let local = ListDocument::empty_peer();
        let mut state = ClientSync::default();
        let step = state
            .receive(
                &local,
                &server.version(),
                SyncPayload::Snapshot(server.export_shallow().unwrap()),
            )
            .unwrap();
        let replacement = step.replacement.unwrap();
        assert_eq!(replacement.rows(), server.rows());
        assert!(!state.review);
        assert!(state.prepare(&replacement).is_err());
        state.persisted();
        assert!(state.prepare(&replacement).is_ok());
    }
}

#[cfg(test)]
mod legacy_snapshot_tests {
    use super::*;
    use crate::RowKey;

    fn legacy() -> ListDocument {
        let raw = loro::LoroDoc::new();
        raw.get_map("meta").insert("name", "Legacy").unwrap();
        let row = raw
            .get_map("rows")
            .insert_container("1:any", loro::LoroMap::new())
            .unwrap();
        row.insert("need", 4_i64).unwrap();
        raw.commit();
        ListDocument::from_snapshot(&raw.export(loro::ExportMode::Snapshot).unwrap()).unwrap()
    }

    #[test]
    fn repeated_related_legacy_snapshots_merge_without_review() {
        let server = legacy();
        let local = ListDocument::empty_peer();
        let mut sync = ClientSync::default();
        let snapshot = server.export_snapshot().unwrap();
        for _ in 0..2 {
            sync.receive(
                &local,
                &server.version(),
                SyncPayload::Snapshot(snapshot.clone()),
            )
            .unwrap();
            assert!(!sync.review);
            sync.persisted();
        }
        local.add_acquired(&RowKey::new(1, None), 2).unwrap();
        server.add_row(RowKey::new(2, None), 3, None).unwrap();
        sync.receive(
            &local,
            &server.version(),
            SyncPayload::Snapshot(server.export_snapshot().unwrap()),
        )
        .unwrap();
        assert!(!sync.review);
        assert_eq!(local.schema(), None);
        assert_eq!(local.rows().len(), 2);
        assert_eq!(local.row(&RowKey::new(1, None)).unwrap().acquired, 2);
    }

    #[test]
    fn identical_visible_legacy_lists_do_not_prove_shared_history() {
        let local = legacy();
        let server = legacy();
        assert_eq!(local.rows(), server.rows());
        let mut sync = ClientSync::default();
        sync.receive(
            &local,
            &server.version(),
            SyncPayload::Snapshot(server.export_snapshot().unwrap()),
        )
        .unwrap();
        assert!(sync.review);
    }

    #[test]
    fn compacted_shared_legacy_history_recovers_pending_acquisitions() {
        let server = legacy();
        let local = ListDocument::from_snapshot(&server.export_snapshot().unwrap()).unwrap();
        local.add_acquired(&RowKey::new(1, None), 2).unwrap();
        server.add_row(RowKey::new(2, None), 3, None).unwrap();
        let mut sync = ClientSync::default();
        let step = sync
            .receive(
                &local,
                &server.version(),
                SyncPayload::Snapshot(server.export_shallow().unwrap()),
            )
            .unwrap();
        let replacement = step.replacement.unwrap();
        assert!(!sync.review);
        assert!(sync.awaiting_persist);
        assert_eq!(replacement.schema(), None);
        assert_eq!(replacement.rows().len(), 2);
        assert_eq!(replacement.row(&RowKey::new(1, None)).unwrap().acquired, 2);
    }
}
