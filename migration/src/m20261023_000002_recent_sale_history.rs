use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // No historical scan or index build on sale_history during migration.
        // New writers maintain the projection; a separate resumable command
        // seeds existing history before readers switch to it.
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TABLE recent_sale_history (
                world_id INTEGER NOT NULL,
                sold_item_id INTEGER NOT NULL,
                hq BOOLEAN NOT NULL,
                pg_id INTEGER NOT NULL,
                price_per_item INTEGER NOT NULL,
                sold_date TIMESTAMP NOT NULL,
                PRIMARY KEY(world_id,sold_item_id,hq,pg_id)
            );
            CREATE TABLE recent_sales_backfill_state (
                id INTEGER PRIMARY KEY CHECK(id=1),
                cursor_item INTEGER,
                cursor_world INTEGER,
                complete BOOLEAN NOT NULL DEFAULT FALSE,
                retention INTEGER NOT NULL CHECK(retention>0),
                CHECK((cursor_item IS NULL)=(cursor_world IS NULL))
            );
            INSERT INTO recent_sales_backfill_state(id,retention) VALUES(1,6)",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "DROP TABLE recent_sales_backfill_state; DROP TABLE recent_sale_history",
            )
            .await?;
        Ok(())
    }
}
