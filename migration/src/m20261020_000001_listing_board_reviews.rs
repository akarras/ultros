use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Deliberately no backfill: any-ingest and seller-review timestamps do
        // not establish when Ultros successfully reconciled a complete board.
        manager.get_connection().execute_unprepared(
            "CREATE TABLE listing_board_review (world_id INTEGER NOT NULL, item_id INTEGER NOT NULL,
             date_time TIMESTAMP NOT NULL, PRIMARY KEY(world_id,item_id))"
        ).await?;
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE listing_board_review")
            .await?;
        Ok(())
    }
}
