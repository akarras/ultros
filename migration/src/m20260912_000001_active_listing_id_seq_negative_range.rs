use sea_orm_migration::prelude::*;

/// Moves `active_listing_id_seq` into the unused negative half of `int4`.
///
/// `active_listing.id` is a `serial` (int4) whose sequence started at 1 in
/// 2022 and has been burning ~4M ids/day since listing ingest went
/// task-per-message. On 2026-09-12 12:57 UTC it hit `2147483647` and every
/// listing insert started failing with
/// `nextval: reached maximum value of sequence "active_listing_id_seq"` —
/// the listing board stopped updating site-wide.
///
/// Listing ids carry no meaning beyond identity: nothing orders by them, no
/// external system stores them, and rows are short-lived (a listing is
/// deleted when it sells or is pulled). Every id ever issued was positive, so
/// the `[-2^31, 0)` range is guaranteed collision-free and buys another ~2.1
/// billion inserts (~500 days at the current rate) without touching the
/// column type, the `materia_listing` FK, or the API types — an instant,
/// lock-free recovery. Widening the column to `bigint` is the durable fix and
/// is left for a planned change: it rewrites the multi-million-row table
/// under `ACCESS EXCLUSIVE` and changes the `i32` id in every consumer.
///
/// Idempotent and safe to run on a healthy database: the restart into the
/// negative range only happens while the sequence is still positive, so a
/// manual `ALTER SEQUENCE ... RESTART WITH -2147483648` applied during the
/// outage is not repeated on deploy (which would collide with the rows
/// inserted in between).
#[derive(DeriveMigrationName)]
pub struct Migration;

const UP: &str = r#"DO $$
DECLARE
    seq regclass := pg_get_serial_sequence('active_listing', 'id');
    current bigint;
BEGIN
    IF seq IS NULL THEN
        RETURN;
    END IF;
    EXECUTE format('ALTER SEQUENCE %s MINVALUE -2147483648 NO CYCLE', seq);
    EXECUTE format('SELECT last_value FROM %s', seq) INTO current;
    IF current >= 0 THEN
        PERFORM setval(seq, -2147483648, false);
    END IF;
END $$"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP).await?;
        Ok(())
    }

    /// Irreversible by design: once negative ids exist, restoring
    /// `MINVALUE 1` would fail (or, with a restart, re-issue ids that are
    /// still live). The widened range is harmless to leave in place.
    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{Database, DbBackend, Statement, TransactionTrait};

    async fn insert_id(tx: &impl ConnectionTrait) -> Result<i32, DbErr> {
        let row = tx
            .query_one_raw(Statement::from_string(
                DbBackend::Postgres,
                "INSERT INTO active_listing DEFAULT VALUES RETURNING id",
            ))
            .await?
            .expect("RETURNING id yields a row");
        row.try_get("", "id")
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn exhausted_sequence_resumes_in_the_negative_range() {
        let db = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let tx = db.begin().await.unwrap();
        // A temp table shadows the real one, and its serial gets its own
        // temp-schema sequence, which `pg_get_serial_sequence` resolves first.
        tx.execute_unprepared(
            "CREATE TEMPORARY TABLE active_listing (id serial PRIMARY KEY) ON COMMIT DROP",
        )
        .await
        .unwrap();
        tx.execute_unprepared(
            "SELECT setval(pg_get_serial_sequence('active_listing', 'id'), 2147483647, true)",
        )
        .await
        .unwrap();
        // Reproduces the outage: the sequence is spent.
        let err = insert_id(&tx).await.unwrap_err().to_string();
        assert!(err.contains("reached maximum value"), "{err}");
        // A failed statement poisons the transaction; recover in a savepoint-free
        // way by starting over on a fresh transaction.
        tx.rollback().await.unwrap();

        let tx = db.begin().await.unwrap();
        tx.execute_unprepared(
            "CREATE TEMPORARY TABLE active_listing (id serial PRIMARY KEY) ON COMMIT DROP",
        )
        .await
        .unwrap();
        tx.execute_unprepared(
            "SELECT setval(pg_get_serial_sequence('active_listing', 'id'), 2147483647, true)",
        )
        .await
        .unwrap();
        let manager = SchemaManager::new(&tx);
        Migration.up(&manager).await.unwrap();
        assert_eq!(insert_id(&tx).await.unwrap(), i32::MIN);
        assert_eq!(insert_id(&tx).await.unwrap(), i32::MIN + 1);
        // Re-running (a deploy after a manual restart) must not re-issue ids
        // that are already live.
        Migration.up(&manager).await.unwrap();
        assert_eq!(insert_id(&tx).await.unwrap(), i32::MIN + 2);
        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn healthy_sequence_is_also_moved_and_never_collides() {
        let db = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let tx = db.begin().await.unwrap();
        tx.execute_unprepared(
            "CREATE TEMPORARY TABLE active_listing (id serial PRIMARY KEY) ON COMMIT DROP",
        )
        .await
        .unwrap();
        let before = insert_id(&tx).await.unwrap();
        assert!(before > 0);
        Migration.up(&SchemaManager::new(&tx)).await.unwrap();
        let after = insert_id(&tx).await.unwrap();
        assert_eq!(after, i32::MIN);
        let count = tx
            .query_one_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT count(*)::bigint AS n FROM active_listing",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(count.try_get::<i64>("", "n").unwrap(), 2);
        tx.rollback().await.unwrap();
    }
}
