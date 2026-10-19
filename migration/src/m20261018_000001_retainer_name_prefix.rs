use sea_orm_migration::prelude::*;

/// All-world Discord autocomplete searches lower(name) by a literal prefix.
/// Pattern ops support that lookup even when the database uses a locale
/// collation; the old (world_id, name) index cannot serve the global search.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(false)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_retainer_name_prefix \
             ON retainer (lower(name) text_pattern_ops, world_id, id)",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP INDEX CONCURRENTLY IF EXISTS idx_retainer_name_prefix")
            .await?;
        Ok(())
    }
}
