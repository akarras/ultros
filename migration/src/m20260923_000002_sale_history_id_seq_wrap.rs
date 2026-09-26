use sea_orm_migration::prelude::*;

/// Lets `sale_history_id_seq` wrap into the unused negative half of `int4`
/// instead of failing at `2147483647` the way `active_listing_id_seq` did on
/// 2026-09-12.
///
/// `sale_history.id` is the same `int4` serial, and the table is append-only
/// and far larger (~1.9 billion rows in 2026-08), so its sequence sits within
/// a few hundred million ids of the ceiling. Hitting it would stop sale
/// ingest site-wide.
///
/// Widening this column the way `active_listing.id` is widened is not viable
/// as a routine deploy: the table is hundreds of GB, `ALTER COLUMN ... TYPE
/// bigint` would lock it for hours, and ClickHouse's `sales`/`sale_receipts`
/// carry the id as `Int32` in their sorting keys, which ClickHouse refuses to
/// retype in place. So this keeps the column and doubles the id space:
///
/// - `MINVALUE -2147483648 CYCLE`: when the sequence reaches `2147483647`, the
///   next id is `-2147483648` instead of an error. Every id issued so far is
///   positive, so the negative range cannot collide. The remaining positive
///   ids are still used first, unlike the immediate restart in
///   `m20260912_000001_active_listing_id_seq_negative_range`.
/// - Nothing orders or pages by `sale_history.id`: reads and the ClickHouse
///   backfill key on `sold_date`, and ClickHouse only uses `pg_id` as a
///   dedup tiebreaker, where a negative `Int32` is as good as a positive one.
///
/// The limit this leaves: after a further ~2.1 billion sales the sequence
/// climbs back through 0 into ids that are still stored, and inserts fail with
/// a unique violation on `sale_history_pkey`. A bigint `sale_history` (with
/// rebuilt ClickHouse tables) has to land before then.
///
/// Instant and lock-light (`ALTER SEQUENCE` does not touch the table), and
/// idempotent. Skips a sequence that is no longer `int4`.
#[derive(DeriveMigrationName)]
pub struct Migration;

const UP: &str = r#"DO $$
DECLARE
    seq regclass := pg_get_serial_sequence('sale_history', 'id');
BEGIN
    IF seq IS NULL
        OR (SELECT seqtypid FROM pg_sequence WHERE seqrelid = seq) <> 'integer'::regtype THEN
        RETURN;
    END IF;
    EXECUTE format('ALTER SEQUENCE %s MINVALUE -2147483648 CYCLE', seq);
END $$"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP).await?;
        Ok(())
    }

    /// Irreversible by design: once the sequence has wrapped, negative ids
    /// exist and `MINVALUE 1` would be rejected. Leaving it in place is harmless.
    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{
        Database, DatabaseConnection, DatabaseTransaction, DbBackend, Statement, TransactionTrait,
    };

    async fn scratch_table(db: &DatabaseConnection) -> DatabaseTransaction {
        let tx = db.begin().await.unwrap();
        // A temp table shadows the real one, and its serial gets its own
        // temp-schema sequence, which `pg_get_serial_sequence` resolves first.
        tx.execute_unprepared(
            "CREATE TEMPORARY TABLE sale_history (id serial PRIMARY KEY) ON COMMIT DROP",
        )
        .await
        .unwrap();
        tx
    }

    async fn insert_id(tx: &impl ConnectionTrait) -> Result<i32, DbErr> {
        tx.query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            "INSERT INTO sale_history DEFAULT VALUES RETURNING id",
        ))
        .await?
        .expect("RETURNING id yields a row")
        .try_get("", "id")
    }

    async fn set_last_value(tx: &impl ConnectionTrait, value: i32) {
        tx.execute_unprepared(&format!(
            "SELECT setval(pg_get_serial_sequence('sale_history', 'id'), {value}, true)"
        ))
        .await
        .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn spends_the_positive_range_then_wraps_negative() {
        let db = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let tx = scratch_table(&db).await;
        set_last_value(&tx, i32::MAX - 2).await;
        let manager = SchemaManager::new(&tx);
        Migration.up(&manager).await.unwrap();
        // Remaining positive ids are not skipped...
        assert_eq!(insert_id(&tx).await.unwrap(), i32::MAX - 1);
        assert_eq!(insert_id(&tx).await.unwrap(), i32::MAX);
        // ...and the ceiling wraps instead of failing.
        assert_eq!(insert_id(&tx).await.unwrap(), i32::MIN);
        // Re-running does not restart the sequence.
        Migration.up(&manager).await.unwrap();
        assert_eq!(insert_id(&tx).await.unwrap(), i32::MIN + 1);
        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn recovers_an_already_exhausted_sequence() {
        let db = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let tx = scratch_table(&db).await;
        set_last_value(&tx, i32::MAX).await;
        // Reproduces the failure this prevents.
        let err = insert_id(&tx).await.unwrap_err().to_string();
        assert!(err.contains("reached maximum value"), "{err}");
        tx.rollback().await.unwrap();

        let tx = scratch_table(&db).await;
        set_last_value(&tx, i32::MAX).await;
        Migration.up(&SchemaManager::new(&tx)).await.unwrap();
        assert_eq!(insert_id(&tx).await.unwrap(), i32::MIN);
        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn leaves_a_widened_sequence_alone() {
        let db = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let tx = db.begin().await.unwrap();
        tx.execute_unprepared(
            "CREATE TEMPORARY TABLE sale_history (id bigserial PRIMARY KEY) ON COMMIT DROP",
        )
        .await
        .unwrap();
        Migration.up(&SchemaManager::new(&tx)).await.unwrap();
        let row = tx
            .query_one_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT min_value, cycle FROM pg_sequences
                 WHERE schemaname = (SELECT nspname FROM pg_namespace WHERE oid = pg_my_temp_schema())
                   AND sequencename = 'sale_history_id_seq'",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i64>("", "min_value").unwrap(), 1);
        assert!(!row.try_get::<bool>("", "cycle").unwrap());
        tx.rollback().await.unwrap();
    }
}
