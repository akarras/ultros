use sea_orm_migration::prelude::*;

use crate::m20240424_000001_create_notification_endpoints::DiscordUser;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PushSubscription::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PushSubscription::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(PushSubscription::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PushSubscription::Endpoint).text().not_null())
                    .col(ColumnDef::new(PushSubscription::P256dh).text().not_null())
                    .col(ColumnDef::new(PushSubscription::Auth).text().not_null())
                    .col(ColumnDef::new(PushSubscription::UserAgent).text())
                    .col(
                        ColumnDef::new(PushSubscription::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(PushSubscription::LastSeenAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_push_subscription_user_id")
                            .from(PushSubscription::Table, PushSubscription::UserId)
                            .to(DiscordUser::Table, DiscordUser::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_push_subscription_user_endpoint")
                    .table(PushSubscription::Table)
                    .col(PushSubscription::UserId)
                    .col(PushSubscription::Endpoint)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PushSubscription::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum PushSubscription {
    Table,
    Id,
    UserId,
    Endpoint,
    P256dh,
    Auth,
    UserAgent,
    CreatedAt,
    LastSeenAt,
}
