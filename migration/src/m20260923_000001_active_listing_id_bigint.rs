use sea_orm_migration::prelude::*;

/// Widens `active_listing.id` (and the dead `materia_listing.active_listing_id`
/// that references it) from `int4` to `bigint` — the durable fix for the
/// 2026-09-12 sequence exhaustion that froze the listing board.
///
/// `m20260912_000001_active_listing_id_seq_negative_range` bought time by
/// moving the sequence into the unused negative half of `int4`, which runs out
/// again around 2028-01 at ~4M inserts/day. This removes the ceiling instead.
///
/// After widening, the sequence is moved to `2^31`, above every id `int4`
/// could ever have issued: continuing from the negative range would climb
/// through 0 into the positive ids issued before the outage, and listings can
/// stay up for months, so some of those rows are still live.
///
/// Cost: `ALTER COLUMN ... TYPE bigint` rewrites the table and rebuilds every
/// index on it under `ACCESS EXCLUSIVE`. It runs in `Migrator::up` at startup,
/// before the server takes traffic, so listing ingest and the listing reads
/// wait for the rewrite. The migration cannot be run ahead of the deploy by
/// hand: the old binary decodes `id` as `i32` and fails on the widened column.
///
/// Idempotent: each step checks the current type or sequence position first,
/// so a re-run (or a run against a database that was widened by hand) changes
/// nothing and never moves the sequence backwards.
#[derive(DeriveMigrationName)]
pub struct Migration;

const UP: &str = r#"DO $$
DECLARE
    seq regclass := pg_get_serial_sequence('active_listing', 'id');
    current bigint;
BEGIN
    -- Index rebuilds dominate the rewrite; give them room for this transaction only.
    PERFORM set_config('maintenance_work_mem', '1GB', true);

    IF (SELECT atttypid FROM pg_attribute
        WHERE attrelid = 'active_listing'::regclass AND attname = 'id') <> 'bigint'::regtype THEN
        ALTER TABLE active_listing ALTER COLUMN id TYPE bigint;
    END IF;

    IF to_regclass('materia_listing') IS NOT NULL
        AND (SELECT atttypid FROM pg_attribute
             WHERE attrelid = 'materia_listing'::regclass
               AND attname = 'active_listing_id') <> 'bigint'::regtype THEN
        ALTER TABLE materia_listing ALTER COLUMN active_listing_id TYPE bigint;
    END IF;

    IF seq IS NULL THEN
        RETURN;
    END IF;
    EXECUTE format(
        'ALTER SEQUENCE %s AS bigint MINVALUE -2147483648 MAXVALUE 9223372036854775807 NO CYCLE',
        seq
    );
    EXECUTE format('SELECT last_value FROM %s', seq) INTO current;
    IF current < 2147483648 THEN
        PERFORM setval(seq, 2147483648, false);
    END IF;
END $$"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP).await?;
        Ok(())
    }

    /// Irreversible by design: every id issued after this migration is above
    /// `int4`'s range, so narrowing the column back would fail.
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

    async fn scratch_tables(db: &DatabaseConnection) -> DatabaseTransaction {
        let tx = db.begin().await.unwrap();
        // Temp tables shadow the real ones (`pg_temp` is searched first), and
        // each serial gets its own temp-schema sequence.
        tx.execute_unprepared(
            "CREATE TEMPORARY TABLE active_listing (id serial PRIMARY KEY, note text) ON COMMIT DROP;
             CREATE TEMPORARY TABLE materia_listing (
                 id serial PRIMARY KEY,
                 active_listing_id integer NOT NULL UNIQUE
                     REFERENCES active_listing (id) ON DELETE CASCADE ON UPDATE CASCADE
             ) ON COMMIT DROP",
        )
        .await
        .unwrap();
        tx
    }

    async fn insert_id(tx: &impl ConnectionTrait) -> i64 {
        tx.query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            "INSERT INTO active_listing DEFAULT VALUES RETURNING id::bigint AS id",
        ))
        .await
        .unwrap()
        .expect("RETURNING id yields a row")
        .try_get("", "id")
        .unwrap()
    }

    async fn column_type(tx: &impl ConnectionTrait, table: &str, column: &str) -> String {
        tx.query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT format_type(atttypid, atttypmod) AS t FROM pg_attribute
             WHERE attrelid = $1::regclass AND attname = $2",
            [table.into(), column.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "t")
        .unwrap()
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn widens_from_the_negative_range_without_reusing_live_ids() {
        let db = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let tx = scratch_tables(&db).await;
        // Production's state after the 09-12 outage: a row at the old ceiling
        // is still live, and the sequence is climbing through the negative range.
        tx.execute_unprepared(
            "INSERT INTO active_listing (id, note) VALUES (2147483647, 'pre-outage');
             INSERT INTO materia_listing (active_listing_id) VALUES (2147483647);
             ALTER SEQUENCE pg_temp.active_listing_id_seq MINVALUE -2147483648;
             SELECT setval(pg_get_serial_sequence('active_listing', 'id'), -2147483648, false);",
        )
        .await
        .unwrap();
        assert_eq!(insert_id(&tx).await, i32::MIN as i64);

        let manager = SchemaManager::new(&tx);
        Migration.up(&manager).await.unwrap();

        assert_eq!(column_type(&tx, "active_listing", "id").await, "bigint");
        assert_eq!(
            column_type(&tx, "materia_listing", "active_listing_id").await,
            "bigint"
        );
        // The next id clears everything int4 ever issued, and keeps going
        // past the old ceiling.
        assert_eq!(insert_id(&tx).await, 1 << 31);
        tx.execute_unprepared(
            "SELECT setval(pg_get_serial_sequence('active_listing', 'id'), 9000000000, true)",
        )
        .await
        .unwrap();
        assert_eq!(insert_id(&tx).await, 9_000_000_001);

        // A second run (a restart, or a database widened by hand) must not
        // move the sequence back onto ids that are already live.
        Migration.up(&manager).await.unwrap();
        assert_eq!(insert_id(&tx).await, 9_000_000_002);

        // Existing rows and the cascading FK survive the rewrite.
        let row = tx
            .query_one_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT count(*)::bigint AS n FROM active_listing a
                 JOIN materia_listing m ON m.active_listing_id = a.id
                 WHERE a.note = 'pre-outage'",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i64>("", "n").unwrap(), 1);
        tx.execute_unprepared("DELETE FROM active_listing WHERE id = 2147483647")
            .await
            .unwrap();
        let row = tx
            .query_one_raw(Statement::from_string(
                DbBackend::Postgres,
                "SELECT count(*)::bigint AS n FROM materia_listing",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i64>("", "n").unwrap(), 0);
        tx.rollback().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL via MIGRATION_TEST_DATABASE_URL"]
    async fn fresh_database_is_widened_too() {
        let db = Database::connect(std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let tx = scratch_tables(&db).await;
        assert_eq!(insert_id(&tx).await, 1);
        Migration.up(&SchemaManager::new(&tx)).await.unwrap();
        assert_eq!(column_type(&tx, "active_listing", "id").await, "bigint");
        assert_eq!(insert_id(&tx).await, 1 << 31);
        tx.rollback().await.unwrap();
    }
}
