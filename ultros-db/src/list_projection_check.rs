//! Read-only decoded projection verification. Never bootstraps or repairs documents.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, FixedOffset};
use sea_orm::{
    AccessMode, ColumnTrait, DatabaseConnection, DatabaseTransaction, DbErr, EntityTrait,
    IsolationLevel, QueryFilter, QuerySelect, TransactionTrait,
};
use serde::Serialize;
use serde_json::{Value, json};
use ultros_list_doc::{ListDocument, RowSnapshot};

use crate::entity::{list, list_doc, list_item};
use crate::list_doc::{clamp_i32, meta_of, row_snapshot};

#[derive(Default)]
pub struct Selection {
    pub ids: BTreeSet<i32>,
    /// Union these IDs with explicitly retained IDs in the same read snapshot.
    pub updated_since: Option<DateTime<FixedOffset>>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub format_version: u32,
    pub checked_at: DateTime<chrono::Utc>,
    pub selected_ids: Vec<i32>,
    pub results: Vec<ListResult>,
}

impl Report {
    /// 0: equal; 1: findings (including absent lists); 3: no selected IDs.
    pub fn exit_code(&self) -> u8 {
        if self.results.is_empty() {
            3
        } else if self
            .results
            .iter()
            .any(|result| result.status != Status::Equal)
        {
            1
        } else {
            0
        }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Equal,
    Different,
    MissingList,
    MissingDocument,
    DecodeError,
}

#[derive(Debug, Serialize)]
pub struct ListResult {
    pub list_id: i32,
    pub status: Status,
    /// A missing legacy scope means preserve the relational scope, not clear it.
    pub scope_compared: bool,
    pub differences: Vec<Value>,
}

struct Inputs {
    id: i32,
    document: Option<list_doc::Model>,
    metadata: Option<list::Model>,
    rows: Vec<list_item::Model>,
}

async fn read_inputs(txn: &DatabaseTransaction, id: i32) -> Result<Inputs, DbErr> {
    Ok(Inputs {
        id,
        document: list_doc::Entity::find_by_id(id).one(txn).await?,
        metadata: list::Entity::find_by_id(id).one(txn).await?,
        rows: list_item::Entity::find()
            .filter(list_item::Column::ListId.eq(id))
            .all(txn)
            .await?,
    })
}

async fn begin_snapshot(db: &DatabaseConnection) -> Result<DatabaseTransaction, DbErr> {
    db.begin_with_config(
        Some(IsolationLevel::RepeatableRead),
        Some(AccessMode::ReadOnly),
    )
    .await
}

/// All selected lists share one PostgreSQL MVCC snapshot. READ ONLY is enforced
/// by the database, and no migration/permission/bootstrap helper is called.
pub async fn check(db: &DatabaseConnection, selection: Selection) -> Result<Report, DbErr> {
    let txn = begin_snapshot(db).await?;
    let ids = select_ids(&txn, selection).await?;
    let mut inputs = Vec::with_capacity(ids.len());
    for id in &ids {
        inputs.push(read_inputs(&txn, *id).await?);
    }
    // Release the snapshot before decoding potentially large documents.
    txn.rollback().await?;
    Ok(Report {
        format_version: 1,
        checked_at: chrono::Utc::now(),
        selected_ids: ids.into_iter().collect(),
        results: inputs.into_iter().map(compare).collect(),
    })
}

async fn select_ids(
    txn: &DatabaseTransaction,
    selection: Selection,
) -> Result<BTreeSet<i32>, DbErr> {
    let mut ids = selection.ids;
    if let Some(since) = selection.updated_since {
        let touched = list_doc::Entity::find()
            .select_only()
            .column(list_doc::Column::ListId)
            .filter(list_doc::Column::UpdatedAt.gte(since))
            .into_tuple::<i32>()
            .all(txn)
            .await?;
        ids.extend(touched);
    }
    Ok(ids)
}

fn row_json(row: &RowSnapshot) -> Value {
    json!({"item_id": row.key.item_id, "quality": row.key.quality.as_str(),
        "quantity": row.need, "acquired": row.acquired, "target_price": row.target})
}

fn compare(input: Inputs) -> ListResult {
    let mut result = ListResult {
        list_id: input.id,
        status: Status::Equal,
        scope_compared: false,
        differences: Vec::new(),
    };
    let Some(metadata) = input.metadata else {
        result.status = Status::MissingList;
        result.differences.push(json!({"kind": "missing_list", "document_present": input.document.is_some(), "remaining_rows": input.rows.len()}));
        return result;
    };
    let Some(stored) = input.document else {
        result.status = Status::MissingDocument;
        return result;
    };
    let document = match ListDocument::from_snapshot(&stored.snapshot) {
        Ok(document) => document,
        Err(_) => {
            // Do not echo arbitrary stored contents through decoder diagnostics.
            result.status = Status::DecodeError;
            return result;
        }
    };
    let expected_meta = document.meta();
    let actual_meta = meta_of(&metadata);
    if expected_meta.name != actual_meta.name {
        result.differences.push(json!({"kind": "metadata", "field": "name", "expected": expected_meta.name, "actual": actual_meta.name}));
    }
    result.scope_compared = expected_meta.scope.is_some();
    if expected_meta.scope.is_some() && expected_meta.scope != actual_meta.scope {
        result.differences.push(json!({"kind": "metadata", "field": "scope", "expected": expected_meta.scope, "actual": actual_meta.scope}));
    }
    let mut actual = BTreeMap::new();
    for model in input.rows {
        let row = row_snapshot(&model);
        if actual.insert(row.key, row).is_some() {
            result
                .differences
                .push(json!({"kind": "duplicate_key", "row": row_json(&row)}));
        }
    }
    for mut expected in document.rows() {
        // These are the very helpers used by the writer, including its legacy
        // NULL defaults above. Target prices deliberately remain signed i64.
        expected.need = i64::from(clamp_i32(expected.need));
        expected.acquired = i64::from(clamp_i32(expected.acquired));
        match actual.remove(&expected.key) {
            None => result.differences.push(json!({"kind": "missing_row", "expected": row_json(&expected)})),
            Some(row) if row != expected => result.differences.push(json!({"kind": "different_row", "expected": row_json(&expected), "actual": row_json(&row)})),
            Some(_) => {}
        }
    }
    for row in actual.values() {
        result
            .differences
            .push(json!({"kind": "excess_row", "actual": row_json(row)}));
    }
    if !result.differences.is_empty() {
        result.status = Status::Different;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use ultros_api_types::world_helper::AnySelector;
    use ultros_list_doc::{MetaSnapshot, RowKey};

    fn fixture() -> Inputs {
        let metadata = list::Model {
            id: 1,
            owner: 1,
            name: "fixture".into(),
            world_id: Some(79),
            datacenter_id: Some(5),
            region_id: Some(1),
        };
        let rows = vec![list_item::Model {
            id: 1,
            list_id: 1,
            item_id: 2,
            hq: None,
            quantity: Some(3),
            acquired: Some(1),
            target_price: Some(-10),
        }];
        let doc = ListDocument::from_rows(meta_of(&metadata), &[row_snapshot(&rows[0])]);
        Inputs {
            id: 1,
            document: Some(list_doc::Model {
                list_id: 1,
                snapshot: doc.export_snapshot().unwrap(),
                version: doc.version(),
                changes_since_compaction: 0,
                updated_at: chrono::Utc::now().into(),
            }),
            metadata: Some(metadata),
            rows,
        }
    }

    #[test]
    fn equal_projection_uses_scope_priority_and_signed_target() {
        let input = fixture();
        assert_eq!(
            meta_of(input.metadata.as_ref().unwrap()).scope,
            Some(AnySelector::Region(1))
        );
        assert_eq!(compare(input).status, Status::Equal);
    }

    #[test]
    fn reports_all_projected_fields_and_natural_key_differences() {
        let mut input = fixture();
        let metadata = input.metadata.as_mut().unwrap();
        metadata.name = "wrong".into();
        metadata.region_id = None;
        input.rows[0].quantity = Some(4);
        input.rows[0].acquired = Some(2);
        input.rows[0].target_price = Some(20);
        let result = compare(input);
        assert_eq!(result.status, Status::Different);
        assert_eq!(result.differences.len(), 3);
        assert_eq!(result.differences[2]["actual"]["quantity"], 4);
        assert_eq!(result.differences[2]["actual"]["acquired"], 2);
        assert_eq!(result.differences[2]["actual"]["target_price"], 20);

        for change_item in [false, true] {
            let mut input = fixture();
            if change_item {
                input.rows[0].item_id = 3;
            } else {
                input.rows[0].hq = Some(true);
            }
            let result = compare(input);
            assert_eq!(result.differences[0]["kind"], "missing_row");
            assert_eq!(result.differences[1]["kind"], "excess_row");
        }
        let mut input = fixture();
        input.rows.push(input.rows[0].clone());
        assert_eq!(compare(input).differences[0]["kind"], "duplicate_key");
    }

    #[test]
    fn clamps_expected_values_but_does_not_hide_bad_relational_values() {
        let mut input = fixture();
        let doc = ListDocument::from_rows(
            meta_of(input.metadata.as_ref().unwrap()),
            &[RowSnapshot {
                key: RowKey::new(2, None),
                need: i64::MAX,
                acquired: -5,
                target: Some(-10),
            }],
        );
        input.document.as_mut().unwrap().snapshot = doc.export_snapshot().unwrap();
        input.rows[0].quantity = Some(i32::MAX);
        input.rows[0].acquired = Some(0);
        assert_eq!(compare(input).status, Status::Equal);
        let mut input = fixture();
        input.rows[0].quantity = Some(-1);
        assert_eq!(compare(input).status, Status::Different);
    }

    #[test]
    fn legacy_defaults_and_absent_scope_match_writer_semantics() {
        let mut input = fixture();
        input.rows[0].quantity = None;
        input.rows[0].acquired = None;
        let doc = ListDocument::from_rows(
            MetaSnapshot {
                name: "fixture".into(),
                scope: None,
            },
            &[row_snapshot(&input.rows[0])],
        );
        input.document.as_mut().unwrap().snapshot = doc.export_snapshot().unwrap();
        let result = compare(input);
        assert_eq!(result.status, Status::Equal);
        assert!(!result.scope_compared);
    }

    #[test]
    fn malformed_deleted_uninitialized_and_empty_are_explicit() {
        let mut input = fixture();
        input.document.as_mut().unwrap().snapshot = vec![1, 2, 3];
        assert_eq!(compare(input).status, Status::DecodeError);
        let mut input = fixture();
        input.metadata = None;
        assert_eq!(compare(input).status, Status::MissingList);
        let mut input = fixture();
        input.document = None;
        assert_eq!(compare(input).status, Status::MissingDocument);
        let mut report = Report {
            format_version: 1,
            checked_at: chrono::Utc::now(),
            selected_ids: vec![],
            results: vec![],
        };
        assert_eq!(report.exit_code(), 3);
        report.results.push(compare(fixture()));
        assert_eq!(report.exit_code(), 0);
        report.results[0].status = Status::MissingList;
        assert_eq!(report.exit_code(), 1);
    }

    #[test]
    fn future_schema_is_not_reported_as_equal() {
        let mut input = fixture();
        let stored = input.document.as_mut().unwrap();
        let document = ListDocument::from_snapshot(&stored.snapshot).unwrap();
        document
            .inner()
            .get_map("meta")
            .insert("schema", 999_i64)
            .unwrap();
        stored.snapshot = document.export_snapshot().unwrap();
        assert_eq!(compare(input).status, Status::DecodeError);
    }

    #[test]
    fn present_empty_document_is_equal_and_not_empty_selection() {
        let mut input = fixture();
        let doc = ListDocument::from_rows(meta_of(input.metadata.as_ref().unwrap()), &[]);
        input.document.as_mut().unwrap().snapshot = doc.export_snapshot().unwrap();
        input.rows.clear();
        assert_eq!(compare(input).status, Status::Equal);
    }

    // Use private tables in an isolated schema, never app fixtures or production
    // data. Two connections share this schema through per-connection search_path.
    #[tokio::test]
    #[ignore = "requires isolated PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn concurrent_edit_cannot_mix_document_and_projection_versions() {
        let url = std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap();
        let mut options = sea_orm::ConnectOptions::new(url.clone());
        options.max_connections(1).min_connections(1);
        let reader = Database::connect(options).await.unwrap();
        let mut options = sea_orm::ConnectOptions::new(url);
        options.max_connections(1).min_connections(1);
        let writer = Database::connect(options).await.unwrap();
        let schema = format!(
            "projection_check_{}_{}",
            std::process::id(),
            chrono::Utc::now().timestamp_micros()
        );
        reader
            .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        for conn in [&reader, &writer] {
            conn.execute_unprepared(&format!("SET search_path TO {schema}"))
                .await
                .unwrap();
        }
        reader.execute_unprepared("CREATE TABLE list (id integer PRIMARY KEY, owner bigint NOT NULL, name text NOT NULL, world_id integer, datacenter_id integer, region_id integer); CREATE TABLE list_item (id integer PRIMARY KEY, list_id integer NOT NULL, item_id integer NOT NULL, hq boolean, quantity integer, acquired integer, target_price bigint); CREATE TABLE list_doc (list_id integer PRIMARY KEY, snapshot bytea NOT NULL, version bytea NOT NULL, changes_since_compaction integer NOT NULL, updated_at timestamptz NOT NULL)").await.unwrap();
        let input = fixture();
        let stored = input.document.unwrap();
        reader.execute_unprepared("INSERT INTO list VALUES (1, 1, 'fixture', 79, 5, 1); INSERT INTO list_item VALUES (1, 1, 2, NULL, 3, 1, -10)").await.unwrap();
        reader
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "INSERT INTO list_doc VALUES (1, $1, $2, 0, now())",
                [stored.snapshot.clone().into(), stored.version.into()],
            ))
            .await
            .unwrap();
        let txn = begin_snapshot(&reader).await.unwrap();
        let union = || Selection {
            ids: BTreeSet::from([2]), // Retained deletion/nonexistent ID must survive discovery.
            updated_since: Some("2000-01-01T00:00:00Z".parse().unwrap()),
        };
        // Selection itself establishes the snapshot before a new list appears.
        assert_eq!(
            select_ids(&txn, union()).await.unwrap(),
            BTreeSet::from([1, 2])
        );
        let old = list_doc::Entity::find_by_id(1)
            .one(&txn)
            .await
            .unwrap()
            .unwrap();
        let newer = ListDocument::from_snapshot(&old.snapshot).unwrap();
        newer.set_need(&RowKey::new(2, None), 7).unwrap();
        let write = writer.begin().await.unwrap();
        write.execute_raw(Statement::from_sql_and_values(DbBackend::Postgres,
            "UPDATE list_doc SET snapshot = $1, version = $2, updated_at = now() WHERE list_id = 1", [newer.export_snapshot().unwrap().into(), newer.version().into()])).await.unwrap();
        write
            .execute_unprepared("UPDATE list_item SET quantity = 7 WHERE list_id = 1")
            .await
            .unwrap();
        write.execute_unprepared("INSERT INTO list SELECT 3, owner, name, world_id, datacenter_id, region_id FROM list WHERE id = 1; INSERT INTO list_doc SELECT 3, snapshot, version, changes_since_compaction, updated_at FROM list_doc WHERE list_id = 1; INSERT INTO list_item SELECT 3, 3, item_id, hq, quantity, acquired, target_price FROM list_item WHERE id = 1").await.unwrap();
        write.commit().await.unwrap();
        assert_eq!(
            select_ids(&txn, union()).await.unwrap(),
            BTreeSet::from([1, 2])
        );
        let coherent = read_inputs(&txn, 1).await.unwrap();
        assert_eq!(coherent.rows[0].quantity, Some(3));
        assert_eq!(compare(coherent).status, Status::Equal);
        txn.rollback().await.unwrap();
        let discovered = check(&reader, union()).await.unwrap();
        assert_eq!(discovered.selected_ids, vec![1, 2, 3]);
        assert_eq!(discovered.results[1].status, Status::MissingList);
        assert_eq!(discovered.results[2].status, Status::Equal);
        let selection = || Selection {
            ids: BTreeSet::from([1]),
            updated_since: None,
        };
        assert_eq!(check(&reader, selection()).await.unwrap().exit_code(), 0);
        writer
            .execute_unprepared("UPDATE list_item SET quantity = 9 WHERE list_id = 1")
            .await
            .unwrap();
        assert_eq!(check(&reader, selection()).await.unwrap().exit_code(), 1);
        // READ ONLY is database-enforced, not merely an operator convention.
        let txn = begin_snapshot(&reader).await.unwrap();
        assert!(
            txn.execute_unprepared("UPDATE list_item SET quantity = 10")
                .await
                .is_err()
        );
        txn.rollback().await.unwrap();
        reader
            .execute_unprepared(&format!("DROP SCHEMA {schema} CASCADE"))
            .await
            .unwrap();
    }
}
