//! The server's copy of each list's Loro document and the single merge path
//! that keeps the relational rows a projection of it (spec section 4).
//!
//! The legacy writers in `lists.rs` (`add_item_to_list`, `update_list_item`,
//! `add_items_to_list`, `update_list`, and the bulk `update_many`) still write
//! `list_item` and the `list` name/scope columns directly, until Task 5 of
//! the Phase 3 plan removes them. Until then, this module is the door only
//! for the Labs path: callers routed through the document sync flow. See
//! `project_rows`'s `Added` arm for how the two writers are kept from
//! violating `idx_list_item_natural_key` on each other during the overlap.

use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseTransaction,
    DbBackend, DbErr, EntityTrait, IntoActiveModel, QueryFilter, Statement, TransactionTrait,
};
use ultros_api_types::list::ListPermission;
use ultros_api_types::world_helper::AnySelector;
use ultros_list_doc::{
    DocError, ListDocument, MetaSnapshot, RowChange, RowKey, RowSnapshot, diff_rows,
};

use crate::UltrosDb;
use crate::entity::{list, list_doc, list_item};
use crate::lists::ListError;

/// Past either bound the next store is a shallow snapshot (spec section 4.1).
pub const COMPACT_AFTER_CHANGES: i32 = 5_000;
pub const COMPACT_AFTER_BYTES: usize = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ListDocError {
    #[error("update bytes are not a valid document update")]
    InvalidUpdate,
    #[error("update depends on history this server does not have; resync from a snapshot")]
    MissingHistory,
    #[error("only the list owner can change its name or scope")]
    MetaForbidden,
    #[error("{0}")]
    List(#[from] ListError),
    #[error("document: {0}")]
    Doc(#[from] DocError),
    #[error("database: {0}")]
    Db(#[from] DbErr),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// One projected row change with the relational row it touched: the row
/// after the change for adds and updates, the row before it for removals.
#[derive(Clone, Debug)]
pub struct ProjectedChange {
    pub change: RowChange,
    pub row: list_item::Model,
}

#[derive(Debug)]
pub struct MergeOutcome {
    /// The list row after any meta projection.
    pub list: list::Model,
    /// The update bytes, verbatim, for relaying to other peers.
    pub relay: Vec<u8>,
    pub changes: Vec<ProjectedChange>,
    pub meta_before: MetaSnapshot,
    pub meta_after: MetaSnapshot,
}

impl MergeOutcome {
    pub fn meta_changed(&self) -> bool {
        self.meta_before != self.meta_after
    }
}

#[derive(Clone, Debug)]
pub struct StoredDoc {
    pub snapshot: Vec<u8>,
    pub version: Vec<u8>,
}

/// The list's scope, translated from its `world_id`/`datacenter_id`/`region_id`
/// columns into the document's `AnySelector`.
///
/// This is deliberately not `lists.rs`'s `TryFrom<&list::Model> for AnySelector`
/// impl: that one targets `world_data::world_cache::AnySelector`, a distinct
/// type from `ultros_api_types::world_helper::AnySelector` that `MetaSnapshot`
/// carries. Same variant shapes, same priority order (region, then
/// datacenter, then world), just a different enum to land in.
fn scope_of(list: &list::Model) -> Option<AnySelector> {
    match (list.world_id, list.datacenter_id, list.region_id) {
        (_, _, Some(region)) => Some(AnySelector::Region(region)),
        (_, Some(datacenter), _) => Some(AnySelector::Datacenter(datacenter)),
        (Some(world), _, _) => Some(AnySelector::World(world)),
        _ => None,
    }
}

fn meta_of(list: &list::Model) -> MetaSnapshot {
    MetaSnapshot {
        name: list.name.clone(),
        scope: scope_of(list),
    }
}

fn row_snapshot(model: &list_item::Model) -> RowSnapshot {
    RowSnapshot {
        key: RowKey::new(model.item_id, model.hq),
        need: model.quantity.unwrap_or(1) as i64,
        acquired: model.acquired.unwrap_or(0) as i64,
        target: model.target_price,
    }
}

fn clamp_i32(value: i64) -> i32 {
    value.clamp(0, i32::MAX as i64) as i32
}

/// `get_permission`'s `anyhow::Result` carries a downcastable `ListError` for
/// the not-found/forbidden cases; unwrap it so callers see `ListDocError::List`
/// instead of the catch-all `Other`.
fn permission_error(e: anyhow::Error) -> ListDocError {
    match e.downcast::<ListError>() {
        Ok(list_error) => ListDocError::List(list_error),
        Err(e) => ListDocError::Other(e),
    }
}

async fn lock_row(txn: &DatabaseTransaction, list_id: i32) -> Result<(), ListDocError> {
    txn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT list_id FROM list_doc WHERE list_id = $1 FOR UPDATE",
        [list_id.into()],
    ))
    .await?;
    Ok(())
}

/// The stored document, locked for this transaction, built from the list's
/// rows on first touch. Two first touches can race on the insert; the loser
/// re-reads the winner's row.
async fn load_or_create(
    txn: &DatabaseTransaction,
    list_id: i32,
) -> Result<list_doc::Model, ListDocError> {
    lock_row(txn, list_id).await?;
    if let Some(stored) = list_doc::Entity::find_by_id(list_id).one(txn).await? {
        return Ok(stored);
    }
    let list = list::Entity::find_by_id(list_id)
        .one(txn)
        .await?
        .ok_or(ListError::NotFound)?;
    let rows = list_item::Entity::find()
        .filter(list_item::Column::ListId.eq(list_id))
        .all(txn)
        .await?;
    let snapshots: Vec<RowSnapshot> = rows.iter().map(row_snapshot).collect();
    let doc = ListDocument::from_rows(meta_of(&list), &snapshots);
    let fresh = list_doc::ActiveModel {
        list_id: Set(list_id),
        snapshot: Set(doc.export_snapshot()?),
        version: Set(doc.version()),
        changes_since_compaction: Set(0),
        updated_at: sea_orm::ActiveValue::NotSet,
    };
    list_doc::Entity::insert(fresh)
        .on_conflict(
            OnConflict::column(list_doc::Column::ListId)
                .do_nothing()
                .to_owned(),
        )
        .exec_without_returning(txn)
        .await?;
    lock_row(txn, list_id).await?;
    list_doc::Entity::find_by_id(list_id)
        .one(txn)
        .await?
        .ok_or_else(|| ListDocError::Db(DbErr::RecordNotFound(format!("list_doc {list_id}"))))
}

fn natural_key_filter(list_id: i32, key: &RowKey) -> sea_orm::Select<list_item::Entity> {
    let select = list_item::Entity::find()
        .filter(list_item::Column::ListId.eq(list_id))
        .filter(list_item::Column::ItemId.eq(key.item_id));
    match key.hq() {
        Some(hq) => select.filter(list_item::Column::Hq.eq(hq)),
        None => select.filter(list_item::Column::Hq.is_null()),
    }
}

async fn project_rows(
    txn: &DatabaseTransaction,
    list_id: i32,
    changes: Vec<RowChange>,
) -> Result<Vec<ProjectedChange>, ListDocError> {
    let mut projected = Vec::with_capacity(changes.len());
    for change in changes {
        match &change {
            RowChange::Added(row) => {
                // A legacy writer (`lists.rs`, still live until Task 5) can
                // insert a row with this same natural key between this
                // document's last store and this import. Update it in place
                // rather than inserting a duplicate, which would violate
                // `idx_list_item_natural_key` and abort the transaction.
                if let Some(model) = natural_key_filter(list_id, &row.key).one(txn).await? {
                    let mut active = model.into_active_model();
                    active.quantity = Set(Some(clamp_i32(row.need)));
                    active.acquired = Set(Some(clamp_i32(row.acquired)));
                    active.target_price = Set(row.target);
                    let model = active.update(txn).await?;
                    projected.push(ProjectedChange { change, row: model });
                } else {
                    let model = list_item::ActiveModel {
                        id: sea_orm::ActiveValue::NotSet,
                        item_id: Set(row.key.item_id),
                        list_id: Set(list_id),
                        hq: Set(row.key.hq()),
                        quantity: Set(Some(clamp_i32(row.need))),
                        acquired: Set(Some(clamp_i32(row.acquired))),
                        target_price: Set(row.target),
                    }
                    .insert(txn)
                    .await?;
                    projected.push(ProjectedChange { change, row: model });
                }
            }
            RowChange::Removed(row) => {
                if let Some(model) = natural_key_filter(list_id, &row.key).one(txn).await? {
                    list_item::Entity::delete_by_id(model.id).exec(txn).await?;
                    projected.push(ProjectedChange { change, row: model });
                } else {
                    tracing::warn!(
                        list_id,
                        item_id = row.key.item_id,
                        hq = ?row.key.hq(),
                        "document change has no matching list_item row"
                    );
                }
            }
            RowChange::Updated { after, .. } => {
                if let Some(model) = natural_key_filter(list_id, &after.key).one(txn).await? {
                    let mut active = model.into_active_model();
                    active.quantity = Set(Some(clamp_i32(after.need)));
                    active.acquired = Set(Some(clamp_i32(after.acquired)));
                    active.target_price = Set(after.target);
                    let model = active.update(txn).await?;
                    projected.push(ProjectedChange { change, row: model });
                } else {
                    tracing::warn!(
                        list_id,
                        item_id = after.key.item_id,
                        hq = ?after.key.hq(),
                        "document change has no matching list_item row"
                    );
                }
            }
        }
    }
    Ok(projected)
}

async fn project_meta(
    txn: &DatabaseTransaction,
    list_id: i32,
    before: &MetaSnapshot,
    after: &MetaSnapshot,
) -> Result<list::Model, ListDocError> {
    let list = list::Entity::find_by_id(list_id)
        .one(txn)
        .await?
        .ok_or(ListError::NotFound)?;
    if before == after {
        return Ok(list);
    }
    let mut active = list.into_active_model();
    active.name = Set(after.name.clone());
    if let Some(scope) = after.scope {
        let (datacenter_id, region_id, world_id) = match scope {
            AnySelector::Datacenter(dc) => (Some(dc), None, None),
            AnySelector::Region(region) => (None, Some(region), None),
            AnySelector::World(world) => (None, None, Some(world)),
        };
        active.datacenter_id = Set(datacenter_id);
        active.region_id = Set(region_id);
        active.world_id = Set(world_id);
    }
    Ok(active.update(txn).await?)
}

async fn store(
    txn: &DatabaseTransaction,
    stored: &list_doc::Model,
    doc: &ListDocument,
) -> Result<(), ListDocError> {
    let changes = stored.changes_since_compaction + 1;
    let full = doc.export_snapshot()?;
    let (snapshot, changes) =
        if changes >= COMPACT_AFTER_CHANGES || full.len() >= COMPACT_AFTER_BYTES {
            (doc.export_shallow()?, 0)
        } else {
            (full, changes)
        };
    let mut active = stored.clone().into_active_model();
    active.snapshot = Set(snapshot);
    active.version = Set(doc.version());
    active.changes_since_compaction = Set(changes);
    active.updated_at = Set(chrono::Utc::now().into());
    active.update(txn).await?;
    Ok(())
}

/// Spec section 4.2, steps 1 to 6, given the already-loaded `list_doc` row.
/// Runs inside the caller's transaction. Split out from `merge` so a caller
/// that already holds `stored` (`edit_list_doc`, which needs it to build the
/// document for its own edit) does not pay for a second `load_or_create` —
/// a row lock plus a `SELECT` — under the same transaction. The document
/// itself is still parsed fresh from `stored.snapshot` here: `edit_list_doc`'s
/// own `ListDocument` already has its local edit applied directly, so it
/// cannot double as the pre-import baseline this function diffs against.
async fn merge_loaded(
    txn: &DatabaseTransaction,
    list_id: i32,
    permission: ListPermission,
    stored: &list_doc::Model,
    update: &[u8],
) -> Result<MergeOutcome, ListDocError> {
    let doc = ListDocument::from_snapshot(&stored.snapshot)?;
    let rows_before = doc.rows();
    let meta_before = doc.meta();
    let report = doc.import(update).map_err(|error| match error {
        DocError::OutdatedDependency => ListDocError::MissingHistory,
        _ => ListDocError::InvalidUpdate,
    })?;
    // Loro parked ops whose dependencies this document lacks: the stored
    // snapshot would omit them even though `import` returned `Ok`. Bail out
    // before any write so the transaction rolls back and the caller resyncs
    // from a fresh snapshot instead of the update silently going missing.
    if report.pending {
        return Err(ListDocError::MissingHistory);
    }
    let rows_after = doc.rows();
    let meta_after = doc.meta();
    if meta_after != meta_before && permission < ListPermission::Owner {
        return Err(ListDocError::MetaForbidden);
    }
    let list = project_meta(txn, list_id, &meta_before, &meta_after).await?;
    let changes = project_rows(txn, list_id, diff_rows(&rows_before, &rows_after)).await?;
    store(txn, stored, &doc).await?;
    Ok(MergeOutcome {
        list,
        relay: update.to_vec(),
        changes,
        meta_before,
        meta_after,
    })
}

/// Spec section 4.2, steps 1 to 6. Runs inside the caller's transaction.
async fn merge(
    txn: &DatabaseTransaction,
    list_id: i32,
    permission: ListPermission,
    update: &[u8],
) -> Result<MergeOutcome, ListDocError> {
    let stored = load_or_create(txn, list_id).await?;
    merge_loaded(txn, list_id, permission, &stored, update).await
}

impl UltrosDb {
    /// The stored document for a reader, created from the rows on first touch.
    pub async fn list_doc_snapshot(
        &self,
        list_id: i32,
        user_id: i64,
    ) -> Result<StoredDoc, ListDocError> {
        let permission = self
            .get_permission(list_id, user_id)
            .await
            .map_err(permission_error)?;
        if permission < ListPermission::Read {
            return Err(ListError::Forbidden("Insufficient permissions to read list").into());
        }
        let txn = self.db.begin().await?;
        let stored = load_or_create(&txn, list_id).await?;
        txn.commit().await?;
        Ok(StoredDoc {
            snapshot: stored.snapshot,
            version: stored.version,
        })
    }

    /// Merge update bytes from a peer. Needs write permission; a change to
    /// the name or scope needs the owner.
    pub async fn apply_list_update(
        &self,
        list_id: i32,
        user_id: i64,
        update: &[u8],
    ) -> Result<MergeOutcome, ListDocError> {
        let permission = self
            .get_permission(list_id, user_id)
            .await
            .map_err(permission_error)?;
        if permission < ListPermission::Write {
            return Err(ListError::Forbidden("Insufficient permissions to edit list").into());
        }
        let txn = self.db.begin().await?;
        match merge(&txn, list_id, permission, update).await {
            Ok(outcome) => {
                txn.commit().await?;
                Ok(outcome)
            }
            Err(e) => {
                let _ = txn.rollback().await;
                Err(e)
            }
        }
    }

    /// Edit the document as the server peer on behalf of `user_id`, then merge
    /// the resulting update through the same path a socket update takes.
    pub async fn edit_list_doc<R>(
        &self,
        list_id: i32,
        user_id: i64,
        edit: impl FnOnce(&ListDocument) -> Result<R, DocError>,
    ) -> Result<(R, MergeOutcome), ListDocError> {
        let permission = self
            .get_permission(list_id, user_id)
            .await
            .map_err(permission_error)?;
        if permission < ListPermission::Write {
            return Err(ListError::Forbidden("Insufficient permissions to edit list").into());
        }
        let txn = self.db.begin().await?;
        let result = async {
            let stored = load_or_create(&txn, list_id).await?;
            let doc = ListDocument::from_snapshot(&stored.snapshot)?;
            let before = doc.version();
            let value = edit(&doc)?;
            let update = doc.export_since(&before)?;
            let outcome = merge_loaded(&txn, list_id, permission, &stored, &update).await?;
            Ok::<_, ListDocError>((value, outcome))
        }
        .await;
        match result {
            Ok(value) => {
                txn.commit().await?;
                Ok(value)
            }
            Err(e) => {
                let _ = txn.rollback().await;
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::list_shared_user;
    use sea_orm::Database;
    use ultros_list_doc::Quality;

    const OWNER: i64 = 990_000_000_301;
    const EDITOR: i64 = 990_000_000_302;

    async fn db() -> UltrosDb {
        let conn = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        UltrosDb::from_connection(conn)
    }

    /// A list owned by OWNER with EDITOR granted write, torn down by the caller
    /// through `delete_list`, which cascades to `list_doc`.
    ///
    /// `create_list` and `set_scope` reference `region`/`datacenter`/`world`
    /// rows that the running app seeds from the world cache at startup; a bare
    /// scratch database has none, so this fixture seeds fixed-id rows itself
    /// (idempotent via `ON CONFLICT`, never torn down) before creating the list.
    async fn scratch_list(db: &UltrosDb) -> i32 {
        db.get_connection()
            .execute_unprepared(
                "INSERT INTO region (id, name) VALUES (1, 'ScratchRegion') ON CONFLICT (id) DO NOTHING;
                 INSERT INTO datacenter (id, name, region_id) VALUES (5, 'ScratchDC', 1) ON CONFLICT (id) DO NOTHING;
                 INSERT INTO world (id, name, datacenter_id) VALUES (79, 'ScratchWorld', 5) ON CONFLICT (id) DO NOTHING;",
            )
            .await
            .unwrap();
        let owner = db
            .get_or_create_discord_user(OWNER as u64, "ListDocOwner".into())
            .await
            .unwrap();
        db.get_or_create_discord_user(EDITOR as u64, "ListDocEditor".into())
            .await
            .unwrap();
        // `create_list`'s selector is `world_data::world_cache::AnySelector`,
        // a different type from the `ultros_api_types::world_helper::AnySelector`
        // this module (and the rest of this test file) uses for the document's
        // scope. Same variants, different enum, so this one call is qualified.
        let list = db
            .create_list(
                owner,
                "scratch".into(),
                Some(crate::world_data::world_cache::AnySelector::Datacenter(5)),
            )
            .await
            .unwrap();
        list_shared_user::ActiveModel {
            list_id: Set(list.id),
            user_id: Set(EDITOR),
            permission: Set(2),
        }
        .insert(db.get_connection())
        .await
        .unwrap();
        list.id
    }

    async fn peer(db: &UltrosDb, list_id: i32, user: i64) -> ListDocument {
        let stored = db.list_doc_snapshot(list_id, user).await.unwrap();
        ListDocument::from_snapshot(&stored.snapshot).unwrap()
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn a_peer_update_projects_into_rows_and_back_again() {
        let db = db().await;
        let list_id = scratch_list(&db).await;
        let key = RowKey::new(5, Some(true));
        let peer = peer(&db, list_id, EDITOR).await;
        let since = peer.version();
        peer.add_row(key, 3, Some(120)).unwrap();
        peer.add_acquired(&key, 1).unwrap();
        let outcome = db
            .apply_list_update(list_id, EDITOR, &peer.export_since(&since).unwrap())
            .await
            .unwrap();
        assert_eq!(outcome.changes.len(), 1);
        let row = &outcome.changes[0].row;
        assert_eq!(
            (
                row.item_id,
                row.hq,
                row.quantity,
                row.acquired,
                row.target_price
            ),
            (5, Some(true), Some(3), Some(1), Some(120))
        );
        assert!(!outcome.meta_changed());

        let since = peer.version();
        peer.set_need(&key, 9).unwrap();
        let outcome = db
            .apply_list_update(list_id, EDITOR, &peer.export_since(&since).unwrap())
            .await
            .unwrap();
        assert!(matches!(
            outcome.changes[0].change,
            RowChange::Updated { .. }
        ));
        assert_eq!(outcome.changes[0].row.quantity, Some(9));

        let since = peer.version();
        peer.remove_row(&key).unwrap();
        let outcome = db
            .apply_list_update(list_id, EDITOR, &peer.export_since(&since).unwrap())
            .await
            .unwrap();
        assert!(matches!(outcome.changes[0].change, RowChange::Removed(_)));
        assert!(db.get_list_items(list_id, OWNER).await.unwrap().is_empty());
        db.delete_list(list_id, OWNER).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn an_update_missing_its_dependencies_is_rejected_not_merged() {
        let db = db().await;
        let list_id = scratch_list(&db).await;
        let peer = peer(&db, list_id, EDITOR).await;
        let server_version_before = peer.version();

        // Edit A, then edit B on top of it.
        let key_a = RowKey::new(11, None);
        peer.add_row(key_a, 1, None).unwrap();
        let v1 = peer.version();
        let key_b = RowKey::new(12, None);
        peer.add_row(key_b, 1, None).unwrap();

        // Exporting only what happened since v1 gets B's ops without A's;
        // the server, still at its pre-A version, cannot apply B until it
        // has seen A first.
        let update_b = peer.export_since(&v1).unwrap();
        let err = db
            .apply_list_update(list_id, EDITOR, &update_b)
            .await
            .unwrap_err();
        assert!(matches!(err, ListDocError::MissingHistory), "{err}");
        assert!(db.get_list_items(list_id, OWNER).await.unwrap().is_empty());

        // The full update, A and B together, applies cleanly.
        let update_full = peer.export_since(&server_version_before).unwrap();
        let outcome = db
            .apply_list_update(list_id, EDITOR, &update_full)
            .await
            .unwrap();
        assert_eq!(outcome.changes.len(), 2);
        let rows = db.get_list_items(list_id, OWNER).await.unwrap();
        let mut item_ids: Vec<i32> = rows.iter().map(|r| r.item_id).collect();
        item_ids.sort();
        assert_eq!(item_ids, vec![11, 12]);

        db.delete_list(list_id, OWNER).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn legacy_rows_seed_the_document_on_first_touch() {
        let db = db().await;
        let list_id = scratch_list(&db).await;
        list_item::ActiveModel {
            id: sea_orm::ActiveValue::NotSet,
            item_id: Set(7),
            list_id: Set(list_id),
            hq: Set(None),
            quantity: Set(None),
            acquired: Set(None),
            target_price: Set(None),
        }
        .insert(db.get_connection())
        .await
        .unwrap();
        let doc = peer(&db, list_id, OWNER).await;
        assert_eq!(
            doc.rows(),
            vec![RowSnapshot {
                key: RowKey::new(7, None),
                need: 1,
                acquired: 0,
                target: None
            }]
        );
        assert_eq!(doc.meta().scope, Some(AnySelector::Datacenter(5)));
        db.delete_list(list_id, OWNER).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn meta_changes_need_the_owner_and_bad_bytes_change_nothing() {
        let db = db().await;
        let list_id = scratch_list(&db).await;
        let editor = peer(&db, list_id, EDITOR).await;
        let since = editor.version();
        editor.rename("renamed by editor").unwrap();
        let err = db
            .apply_list_update(list_id, EDITOR, &editor.export_since(&since).unwrap())
            .await
            .unwrap_err();
        assert!(matches!(err, ListDocError::MetaForbidden), "{err}");
        let (list, _) = db.get_list(list_id, OWNER).await.unwrap();
        assert_eq!(list.name, "scratch");

        let err = db
            .apply_list_update(list_id, OWNER, b"garbage")
            .await
            .unwrap_err();
        assert!(matches!(err, ListDocError::InvalidUpdate), "{err}");

        let (_, outcome) = db
            .edit_list_doc(list_id, OWNER, |doc| {
                doc.rename("renamed by owner")?;
                doc.set_scope(AnySelector::World(79))
            })
            .await
            .unwrap();
        assert!(outcome.meta_changed());
        assert_eq!(
            (outcome.list.name.as_str(), outcome.list.world_id),
            ("renamed by owner", Some(79))
        );
        db.delete_list(list_id, OWNER).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn edit_as_server_merges_a_quality_move() {
        let db = db().await;
        let list_id = scratch_list(&db).await;
        let key = RowKey::new(8, None);
        db.edit_list_doc(list_id, EDITOR, |doc| doc.add_row(key, 2, None))
            .await
            .unwrap();
        let (moved, outcome) = db
            .edit_list_doc(list_id, EDITOR, |doc| doc.set_quality(&key, Quality::Hq))
            .await
            .unwrap();
        assert_eq!(moved, RowKey::new(8, Some(true)));
        let kinds: Vec<bool> = outcome
            .changes
            .iter()
            .map(|c| matches!(c.change, RowChange::Added(_)))
            .collect();
        assert_eq!(
            kinds,
            vec![false, true],
            "removed the any row, added the hq row"
        );
        let rows = db.get_list_items(list_id, OWNER).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].item_id, rows[0].hq, rows[0].quantity),
            (8, Some(true), Some(2))
        );
        db.delete_list(list_id, OWNER).await.unwrap();
    }
}
