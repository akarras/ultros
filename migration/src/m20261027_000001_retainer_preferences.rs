use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE plugin_retainer
             ADD COLUMN game_order SMALLINT CHECK (game_order BETWEEN 0 AND 9),
             ADD COLUMN available BOOLEAN,
             ADD COLUMN disabled BOOLEAN NOT NULL DEFAULT false;",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(
            "ALTER TABLE plugin_retainer DROP COLUMN disabled, DROP COLUMN available, DROP COLUMN game_order;"
        ).await?;
        Ok(())
    }
}
