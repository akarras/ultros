use sea_orm_migration::prelude::*;

/// Single-column index for the Market Pulse per-world listing count:
///
/// ```sql
/// SELECT COUNT(*) FROM active_listing WHERE world_id = ?
/// ```
///
/// None of the existing indexes can serve that predicate: `WorldItemIndex`
/// and `idx_active_listing_cheapest` lead with `item_id`, and
/// `idx_active_listing_identity` leads with `world_id` but is partial
/// (`WHERE listing_id IS NOT NULL`), so an unfiltered count can't use it.
/// Measured on prod (12.8M rows): the count ran ~770ms per home-page load,
/// which was the entire p50 of `/api/v1/market_pulse/{world}`. A plain
/// `(world_id)` btree turns it into an index-only scan over one world's
/// ~120k entries.
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
                r#"CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_active_listing_world
                   ON active_listing (world_id)"#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(r#"DROP INDEX CONCURRENTLY IF EXISTS idx_active_listing_world"#)
            .await?;
        Ok(())
    }
}
