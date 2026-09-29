use sea_orm_migration::prelude::*;

/// Drop `SaleHistoryFullIndex` — a 59 GB index on `sale_history` that no query
/// can use.
///
/// `m20220911_200503_add_sale_index` created it as
///
/// ```sql
/// CREATE INDEX "SaleHistoryFullIndex" ON sale_history
///     (price_per_item, quantity, hq, buying_character_id, sold_item_id, world_id)
/// ```
///
/// Its leading column is `price_per_item`, and nothing in `ultros-db` ever
/// filters or orders on that column — it is only ever projected. A btree whose
/// leading column is never a predicate is only reachable by a full index scan,
/// which the planner will never prefer over `sale_history_lookup_index`
/// (`sold_item_id, world_id, sold_date DESC`), the index purpose-built for the
/// one hot query on this table:
///
/// ```sql
/// SELECT ... FROM sale_history
/// WHERE sold_item_id = $1 AND world_id = $2
/// ORDER BY sold_date DESC LIMIT $3
/// ```
///
/// Measured on production before this migration:
///
/// | index                       | size  | `idx_scan` |
/// |-----------------------------|-------|------------|
/// | `SaleHistoryFullIndex`      | 59 GB | 0          |
/// | `sale_history_lookup_index` | 53 GB | 3,629,728  |
///
/// over a window carrying 1.18 M inserts into a 122 GB table. So it is pure
/// cost: six columns of index maintenance on every ingested sale, and 59 GB of
/// page cache competing with the index that actually serves reads. That
/// competition is visible — `EXPLAIN (ANALYZE, BUFFERS)` on the hot query shows
/// 100 of 109 buffers coming from disk (42 ms of `shared read` out of a 44 ms
/// execution), and under load those reads stretch to the 1.1–1.5 s
/// `slow statement` warnings that precede the pool-acquire timeouts behind
/// GlitchTip #2209 / #2210 and the catch-up ingest failures (#6868 / #6869).
#[derive(DeriveMigrationName)]
pub struct Migration;

/// `sale_history` is written continuously by ingest, and a plain `DROP INDEX`
/// takes an `ACCESS EXCLUSIVE` lock on the table while it unlinks 59 GB of
/// segment files. `CONCURRENTLY` keeps writers running, at the cost of not
/// being allowed inside a transaction block — hence `use_transaction` below,
/// matching `m20260804_000001_active_listing_cheapest_index`.
///
/// The identifier **must stay double-quoted**. It was created mixed-case, so an
/// unquoted `DROP INDEX ... IF EXISTS SaleHistoryFullIndex` folds to
/// `salehistoryfullindex`, matches nothing, and — because of `IF EXISTS` —
/// succeeds while dropping nothing at all.
const DROP_SQL: &str = r#"DROP INDEX CONCURRENTLY IF EXISTS "SaleHistoryFullIndex""#;

const RECREATE_SQL: &str = r#"CREATE INDEX CONCURRENTLY IF NOT EXISTS "SaleHistoryFullIndex"
                   ON sale_history (price_per_item, quantity, hq, buying_character_id, sold_item_id, world_id)"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(false)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(DROP_SQL)
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(RECREATE_SQL)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The index was created mixed-case, so Postgres only resolves it when the
    /// identifier is double-quoted. Unquoted, it folds to lowercase, matches
    /// nothing, and `IF EXISTS` turns the whole migration into a silent no-op
    /// that leaves 59 GB in place while reporting success.
    #[test]
    fn drop_quotes_the_mixed_case_identifier() {
        assert!(
            DROP_SQL.contains(r#""SaleHistoryFullIndex""#),
            "identifier must be double-quoted, got: {DROP_SQL}"
        );
        assert!(
            !DROP_SQL.contains(" SaleHistoryFullIndex"),
            "identifier must never appear unquoted, got: {DROP_SQL}"
        );
    }

    #[test]
    fn recreate_quotes_the_mixed_case_identifier() {
        assert!(
            RECREATE_SQL.contains(r#""SaleHistoryFullIndex""#),
            "identifier must be double-quoted, got: {RECREATE_SQL}"
        );
    }

    /// `CONCURRENTLY` is what keeps ingest writing while the drop runs; it is
    /// also what forces `use_transaction() == Some(false)`. If either half is
    /// removed without the other, the migration fails at runtime — Postgres
    /// rejects `CONCURRENTLY` inside a transaction block.
    #[test]
    fn concurrent_ddl_runs_outside_a_transaction() {
        assert!(DROP_SQL.contains("CONCURRENTLY"));
        assert!(RECREATE_SQL.contains("CONCURRENTLY"));
        assert_eq!(Migration.use_transaction(), Some(false));
    }

    /// `down` must restore exactly the column list
    /// `m20220911_200503_add_sale_index` created, in order — a reversal that
    /// silently changes the index is not a reversal.
    #[test]
    fn down_restores_the_original_column_list() {
        assert!(RECREATE_SQL.contains(
            "(price_per_item, quantity, hq, buying_character_id, sold_item_id, world_id)"
        ));
    }
}
