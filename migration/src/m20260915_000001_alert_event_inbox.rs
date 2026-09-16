use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Notification inbox: alert_event rows become read/unread messages
        // with their own title/body/click_url, rather than being purely a
        // delivery-attempt log.
        db.execute_unprepared(
            r#"ALTER TABLE alert_event
                ADD COLUMN IF NOT EXISTS read_at   timestamp with time zone,
                ADD COLUMN IF NOT EXISTS title     text,
                ADD COLUMN IF NOT EXISTS body      text,
                ADD COLUMN IF NOT EXISTS click_url text"#,
        )
        .await?;

        db.execute_unprepared(r#"CREATE INDEX IF NOT EXISTS idx_alert_owner ON alert (owner)"#)
            .await?;

        db.execute_unprepared(
            r#"CREATE INDEX IF NOT EXISTS idx_alert_event_alert_unread ON alert_event (alert_id) WHERE read_at IS NULL"#,
        )
        .await?;

        // A user has at most one InApp ("This site") endpoint. Without this,
        // two concurrent first-time `GET /api/v1/endpoints` calls both pass
        // `get_or_create_inapp_endpoint`'s select-then-insert race and each
        // insert a row — and since `delete_endpoint` refuses to delete InApp
        // rows, the duplicate is permanent.
        db.execute_unprepared(
            r#"CREATE UNIQUE INDEX IF NOT EXISTS uq_notification_endpoint_inapp ON notification_endpoint (user_id) WHERE method = 'InApp'"#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(r#"DROP INDEX IF EXISTS uq_notification_endpoint_inapp"#)
            .await?;
        db.execute_unprepared(r#"DROP INDEX IF EXISTS idx_alert_event_alert_unread"#)
            .await?;
        db.execute_unprepared(r#"DROP INDEX IF EXISTS idx_alert_owner"#)
            .await?;

        db.execute_unprepared(
            r#"ALTER TABLE alert_event
                DROP COLUMN IF EXISTS read_at,
                DROP COLUMN IF EXISTS title,
                DROP COLUMN IF EXISTS body,
                DROP COLUMN IF EXISTS click_url"#,
        )
        .await?;

        Ok(())
    }
}
