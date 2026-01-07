use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create notification_endpoint table
        manager
            .create_table(
                Table::create()
                    .table(NotificationEndpoint::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NotificationEndpoint::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NotificationEndpoint::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NotificationEndpoint::Name).text().not_null())
                    .col(
                        ColumnDef::new(NotificationEndpoint::Method)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NotificationEndpoint::Config)
                            .json()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NotificationEndpoint::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_notification_endpoint_user_id")
                            .from(NotificationEndpoint::Table, NotificationEndpoint::UserId)
                            .to(DiscordUser::Table, DiscordUser::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create alert_notification_rule table
        manager
            .create_table(
                Table::create()
                    .table(AlertNotificationRule::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AlertNotificationRule::AlertId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertNotificationRule::EndpointId)
                            .integer()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(AlertNotificationRule::AlertId)
                            .col(AlertNotificationRule::EndpointId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_alert_notification_rule_alert_id")
                            .from(AlertNotificationRule::Table, AlertNotificationRule::AlertId)
                            .to(Alert::Table, Alert::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_alert_notification_rule_endpoint_id")
                            .from(
                                AlertNotificationRule::Table,
                                AlertNotificationRule::EndpointId,
                            )
                            .to(NotificationEndpoint::Table, NotificationEndpoint::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Data Migration: Convert alert_discord_destination to notification_endpoint + alert_notification_rule
        // Using PostgreSQL syntax for JSON operations.

        let db = manager.get_connection();

        // 1. Create Notification Endpoints for each unique (owner, channel_id) pair found in existing alerts.
        // We use json_build_object for Postgres.

        db.execute_unprepared(
            r#"
            INSERT INTO notification_endpoint (user_id, name, method, config)
            SELECT
                a.owner as user_id,
                'Discord Channel ' || dest.channel_id::text as name,
                'DiscordChannel' as method,
                json_build_object('channel_id', dest.channel_id) as config
            FROM alert_discord_destination dest
            JOIN alert a ON a.id = dest.alert_id
            WHERE NOT EXISTS (
                SELECT 1 FROM notification_endpoint ne
                WHERE ne.user_id = a.owner
                AND (ne.config->>'channel_id')::bigint = dest.channel_id
                AND ne.method = 'DiscordChannel'
            )
            GROUP BY a.owner, dest.channel_id
            "#,
        )
        .await?;

        // 2. Create Alert Notification Rules
        // Link alerts to the newly created endpoints.

        db.execute_unprepared(
            r#"
            INSERT INTO alert_notification_rule (alert_id, endpoint_id)
            SELECT
                dest.alert_id,
                ne.id
            FROM alert_discord_destination dest
            JOIN alert a ON a.id = dest.alert_id
            JOIN notification_endpoint ne ON ne.user_id = a.owner
            WHERE ne.method = 'DiscordChannel'
            AND (ne.config->>'channel_id')::bigint = dest.channel_id
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AlertNotificationRule::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NotificationEndpoint::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum NotificationEndpoint {
    Table,
    Id,
    UserId,
    Name,
    Method,
    Config,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum AlertNotificationRule {
    Table,
    AlertId,
    EndpointId,
}

#[derive(DeriveIden)]
pub enum DiscordUser {
    Table,
    Id,
}

#[derive(DeriveIden)]
pub enum Alert {
    Table,
    Id,
}
