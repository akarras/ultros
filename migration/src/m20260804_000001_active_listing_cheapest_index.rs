use sea_orm_migration::prelude::*;

/// Covering index for `UltrosDb::cheapest_listings`, which the analyzer runs on
/// boot and on bus-lag recovery:
///
/// ```sql
/// SELECT item_id, hq, world_id, MIN(price_per_unit)
/// FROM active_listing GROUP BY item_id, hq, world_id
/// ```
///
/// The only pre-existing index on this table is `WorldItemIndex` on
/// (item_id, world_id) — it carries neither `hq` nor `price_per_unit`, so that
/// query had to go to the heap for every row. Ordering the columns
/// group-keys-then-value lets Postgres serve it as an index-only scan.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// `CREATE INDEX CONCURRENTLY` is rejected inside a transaction block, and
    /// sea-orm wraps migrations in one by default on Postgres. `active_listing`
    /// is written continuously by ingest, so a plain `CREATE INDEX` would hold an
    /// exclusive write lock for the whole build — opting out of the transaction
    /// is what keeps this migration online.
    fn use_transaction(&self) -> Option<bool> {
        Some(false)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_active_listing_cheapest
                   ON active_listing (item_id, hq, world_id, price_per_unit)"#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(r#"DROP INDEX CONCURRENTLY IF EXISTS idx_active_listing_cheapest"#)
            .await?;
        Ok(())
    }
}
