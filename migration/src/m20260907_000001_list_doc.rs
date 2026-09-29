//! The server's copy of each list's Loro document (spec section 4.1), and
//! the natural-key uniqueness the projection relies on (section 4.3).

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Duplicate `(list_id, item_id, hq)` rows predate the application-level
/// dedupe. Fold each group into its lowest id, summing what a merge of adds
/// would have summed, then drop the rest. Match the document projection's
/// 0..=i32::MAX bounds so valid integer rows cannot overflow during migration.
const FOLD_DUPLICATES: &str = r#"
WITH dupes AS (
    SELECT list_id, item_id, hq,
           MIN(id) AS keep_id,
           LEAST(2147483647, GREATEST(0, SUM(COALESCE(quantity, 1)))) AS quantity,
           LEAST(2147483647, GREATEST(0, SUM(COALESCE(acquired, 0)))) AS acquired
    FROM list_item
    GROUP BY list_id, item_id, hq
    HAVING COUNT(*) > 1
)
UPDATE list_item li
SET quantity = d.quantity, acquired = d.acquired
FROM dupes d
WHERE li.id = d.keep_id
"#;

const DELETE_DUPLICATES: &str = r#"
DELETE FROM list_item li
USING (
    SELECT l.id
    FROM list_item l
    WHERE EXISTS (
        SELECT 1 FROM list_item o
        WHERE o.list_id = l.list_id
          AND o.item_id = l.item_id
          AND o.hq IS NOT DISTINCT FROM l.hq
          AND o.id < l.id
    )
) x
WHERE li.id = x.id
"#;

/// NULL `hq` means "any quality" and must collide with itself, which a plain
/// unique index on a nullable column does not do.
const CREATE_NATURAL_KEY_INDEX: &str = r#"
CREATE UNIQUE INDEX IF NOT EXISTS idx_list_item_natural_key
ON list_item (list_id, item_id, (COALESCE(hq::int, -1)))
"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ListDoc::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ListDoc::ListId)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ListDoc::Snapshot).binary().not_null())
                    .col(ColumnDef::new(ListDoc::Version).binary().not_null())
                    .col(
                        ColumnDef::new(ListDoc::ChangesSinceCompaction)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(ListDoc::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_list_doc_list_id")
                            .from(ListDoc::Table, ListDoc::ListId)
                            .to(List::Table, List::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        let conn = manager.get_connection();
        conn.execute_unprepared(FOLD_DUPLICATES).await?;
        conn.execute_unprepared(DELETE_DUPLICATES).await?;
        conn.execute_unprepared(CREATE_NATURAL_KEY_INDEX).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP INDEX IF EXISTS idx_list_item_natural_key")
            .await?;
        manager
            .drop_table(Table::drop().table(ListDoc::Table).if_exists().to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ListDoc {
    Table,
    ListId,
    Snapshot,
    Version,
    ChangesSinceCompaction,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum List {
    Table,
    Id,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{Database, DbBackend, Statement, TransactionTrait};

    /// Run the complete migration inside a unique schema in one transaction.
    /// Its real tables allow PostgreSQL to validate the foreign key, and
    /// rollback removes the entire fixture without changing existing schemas.
    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn duplicates_fold_into_the_lowest_id_and_the_index_exists() {
        let db = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let tx = db.begin().await.unwrap();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let schema = format!("list_doc_migration_{}_{nonce}", std::process::id());
        tx.execute_unprepared(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        tx.execute_unprepared(&format!("SET LOCAL search_path TO {schema}"))
            .await
            .unwrap();
        tx.execute_unprepared("CREATE TABLE list (id integer PRIMARY KEY)")
            .await
            .unwrap();
        tx.execute_unprepared(
            "CREATE TABLE list_item (id serial PRIMARY KEY, list_id integer, item_id integer, hq boolean, quantity integer, acquired integer, target_price bigint)",
        )
        .await
        .unwrap();
        tx.execute_unprepared("INSERT INTO list VALUES (1)")
            .await
            .unwrap();
        tx.execute_unprepared(
            "INSERT INTO list_item (list_id, item_id, hq, quantity, acquired) VALUES \
             (1, 5, NULL, 2, 1), (1, 5, NULL, 3, NULL), (1, 5, true, 1, 0), (1, 6, NULL, NULL, 4), \
             (1, 7, NULL, 2147483647, 2147483647), (1, 7, NULL, 1, 1), \
             (1, 8, NULL, -2147483648, -2147483648), (1, 8, NULL, -1, -1), \
             (1, 9, NULL, 2147483646, 2147483646), (1, 9, NULL, NULL, 1)",
        )
        .await
        .unwrap();
        let manager = SchemaManager::new(&tx);
        Migration.up(&manager).await.unwrap();
        let rows = tx
            .query_all_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT id, item_id, hq, quantity, acquired FROM list_item ORDER BY item_id, hq NULLS FIRST",
            ))
            .await
            .unwrap();
        type Row = (i32, i32, Option<bool>, Option<i32>, Option<i32>);
        let values: Vec<Row> = rows
            .iter()
            .map(|row| {
                (
                    row.try_get("", "id").unwrap(),
                    row.try_get("", "item_id").unwrap(),
                    row.try_get("", "hq").unwrap(),
                    row.try_get("", "quantity").unwrap(),
                    row.try_get("", "acquired").unwrap(),
                )
            })
            .collect();
        assert_eq!(
            values,
            vec![
                (1, 5, None, Some(5), Some(1)),
                (3, 5, Some(true), Some(1), Some(0)),
                (4, 6, None, None, Some(4)),
                (5, 7, None, Some(i32::MAX), Some(i32::MAX)),
                (7, 8, None, Some(0), Some(0)),
                (9, 9, None, Some(i32::MAX), Some(i32::MAX)),
            ]
        );
        let indexes = tx
            .query_all_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT indexname FROM pg_indexes \
                 WHERE indexname = 'idx_list_item_natural_key' \
                   AND schemaname = current_schema()",
            ))
            .await
            .unwrap();
        assert_eq!(indexes.len(), 1);
        tx.rollback().await.unwrap();
    }
}
