use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TABLE listing_board_revision (
                world_id INTEGER NOT NULL, item_id INTEGER NOT NULL,
                revision BIGINT NOT NULL DEFAULT 0,
                PRIMARY KEY(world_id,item_id))",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE listing_board_revision")
            .await?;
        Ok(())
    }
}
