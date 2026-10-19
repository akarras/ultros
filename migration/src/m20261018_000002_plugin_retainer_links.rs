use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(
            "ALTER TABLE plugin_retainer ADD COLUMN public_retainer_id INTEGER REFERENCES retainer(id) ON DELETE SET NULL;
             CREATE UNIQUE INDEX plugin_retainer_public_link ON plugin_retainer(owner, public_retainer_id) WHERE public_retainer_id IS NOT NULL;"
        ).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE plugin_retainer DROP COLUMN public_retainer_id;")
            .await?;
        Ok(())
    }
}
