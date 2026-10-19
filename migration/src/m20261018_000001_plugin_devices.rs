use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(include_str!("plugin_devices.sql"))
            .await?;
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared("DROP TABLE IF EXISTS notification_outbox, plugin_delivery, plugin_snapshot, plugin_operation, plugin_retainer, plugin_character, plugin_approval, plugin_token, plugin_pairing, plugin_security_event, plugin_grant, plugin_rate_limit CASCADE; DROP FUNCTION IF EXISTS refresh_notification_pending(); ALTER TABLE alert_event DROP COLUMN IF EXISTS accepted_at; ALTER TABLE alert_event DROP COLUMN IF EXISTS pending_destinations").await?;
        Ok(())
    }
}
