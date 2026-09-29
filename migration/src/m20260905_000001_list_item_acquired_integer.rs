use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // The historical `unsigned()` column now maps to BIGINT on PostgreSQL.
        // The entity and public API use Option<i32>, so non-null values cannot
        // be decoded on databases created with the newer query builder.
        // PostgreSQL rejects out-of-range values instead of truncating them.
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE list_item ALTER COLUMN acquired TYPE integer")
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE list_item ALTER COLUMN acquired TYPE bigint")
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{Database, DbBackend, Statement, TransactionTrait};

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn acquired_integer_round_trip_preserves_nullable_values() {
        let db = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let tx = db.begin().await.unwrap();
        // PostgreSQL resolves this session-local table before the real table.
        tx.execute_unprepared("CREATE TEMPORARY TABLE list_item (acquired bigint) ON COMMIT DROP")
            .await
            .unwrap();
        tx.execute_unprepared("INSERT INTO list_item VALUES (NULL), (0), (2147483647)")
            .await
            .unwrap();
        let manager = SchemaManager::new(&tx);
        Migration.up(&manager).await.unwrap();
        let rows = tx
            .query_all_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT acquired FROM list_item ORDER BY acquired NULLS FIRST",
            ))
            .await
            .unwrap();
        let values: Vec<Option<i32>> = rows
            .iter()
            .map(|row| row.try_get("", "acquired").unwrap())
            .collect();
        assert_eq!(values, [None, Some(0), Some(i32::MAX)]);
        // Existing INTEGER installations and repeated application also work.
        Migration.up(&manager).await.unwrap();
        Migration.down(&manager).await.unwrap();
        let row = tx
            .query_one_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT max(acquired) AS acquired FROM list_item",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.try_get::<i64>("", "acquired").unwrap(),
            i64::from(i32::MAX)
        );
        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn acquired_integer_rejects_out_of_range_without_truncation() {
        let db = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let tx = db.begin().await.unwrap();
        tx.execute_unprepared("CREATE TEMPORARY TABLE list_item (acquired bigint) ON COMMIT DROP")
            .await
            .unwrap();
        tx.execute_unprepared("INSERT INTO list_item VALUES (2147483648)")
            .await
            .unwrap();
        assert!(Migration.up(&SchemaManager::new(&tx)).await.is_err());
        tx.rollback().await.unwrap();
    }
}
