use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Delivery health for a notification endpoint.
        //
        // `disabled_at` being non-NULL means delivery hit a *permanent* failure
        // (Discord `Unknown Channel` / `Missing Access` — the channel is gone or
        // the bot was removed) and we stopped retrying it. Before this, every
        // alert fire re-tried the dead destination forever: six alerts were
        // generating ~150 error events a day while their owners silently
        // received nothing.
        //
        // `last_error` keeps the reason so the endpoints UI can explain *why* it
        // stopped, rather than the endpoint just appearing to do nothing.
        db.execute_unprepared(
            r#"ALTER TABLE notification_endpoint
                ADD COLUMN IF NOT EXISTS disabled_at timestamp with time zone,
                ADD COLUMN IF NOT EXISTS last_error text"#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"ALTER TABLE notification_endpoint
                DROP COLUMN IF EXISTS disabled_at,
                DROP COLUMN IF EXISTS last_error"#,
        )
        .await?;
        Ok(())
    }
}
