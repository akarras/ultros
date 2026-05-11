use sea_orm_migration::prelude::*;

use crate::m20240424_000001_create_notification_endpoints::Alert;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. Add operational columns to `alert`
        manager
            .alter_table(
                Table::alter()
                    .table(Alert::Table)
                    .add_column(
                        ColumnDef::new(AlertExt::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .add_column(ColumnDef::new(AlertExt::LastFiredAt).timestamp_with_time_zone())
                    .add_column(
                        ColumnDef::new(AlertExt::CooldownSeconds)
                            .integer()
                            .not_null()
                            .default(3600),
                    )
                    .to_owned(),
            )
            .await?;

        // 2. Create alert_item_threshold (per-item version of alert_price)
        manager
            .create_table(
                Table::create()
                    .table(AlertItemThreshold::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AlertItemThreshold::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(AlertItemThreshold::AlertId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertItemThreshold::ItemId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertItemThreshold::WorldSelector)
                            .json()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertItemThreshold::PriceThreshold)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(AlertItemThreshold::HqOnly).boolean().not_null().default(false))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_alert_item_threshold_alert_id")
                            .from(AlertItemThreshold::Table, AlertItemThreshold::AlertId)
                            .to(Alert::Table, Alert::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_alert_item_threshold_item")
                    .table(AlertItemThreshold::Table)
                    .col(AlertItemThreshold::ItemId)
                    .to_owned(),
            )
            .await?;

        // 3. Create alert_event for fire history
        manager
            .create_table(
                Table::create()
                    .table(AlertEvent::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AlertEvent::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(AlertEvent::AlertId).integer().not_null())
                    .col(
                        ColumnDef::new(AlertEvent::FiredAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(AlertEvent::ItemId).integer().not_null())
                    .col(ColumnDef::new(AlertEvent::MatchedListingId).big_integer())
                    .col(ColumnDef::new(AlertEvent::MatchedPrice).integer())
                    .col(
                        ColumnDef::new(AlertEvent::Delivered)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(AlertEvent::DeliveryError).text())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_alert_event_alert_id")
                            .from(AlertEvent::Table, AlertEvent::AlertId)
                            .to(Alert::Table, Alert::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_alert_event_alert_fired")
                    .table(AlertEvent::Table)
                    .col(AlertEvent::AlertId)
                    .col(AlertEvent::FiredAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AlertEvent::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(AlertItemThreshold::Table).to_owned())
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Alert::Table)
                    .drop_column(AlertExt::CooldownSeconds)
                    .drop_column(AlertExt::LastFiredAt)
                    .drop_column(AlertExt::Enabled)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum AlertExt {
    Enabled,
    LastFiredAt,
    CooldownSeconds,
}

#[derive(DeriveIden)]
enum AlertItemThreshold {
    Table,
    Id,
    AlertId,
    ItemId,
    WorldSelector,
    PriceThreshold,
    HqOnly,
}

#[derive(DeriveIden)]
enum AlertEvent {
    Table,
    Id,
    AlertId,
    FiredAt,
    ItemId,
    MatchedListingId,
    MatchedPrice,
    Delivered,
    DeliveryError,
}
