//! Child tables for the below-median and back-in-stock alert triggers. Both
//! mirror `alert_item_threshold`: one row per alert (`alert_id` unique),
//! cascading with the parent, the world scope stored as selector JSON.

use sea_orm_migration::prelude::*;

use crate::m20240424_000001_create_notification_endpoints::Alert;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AlertBelowMedian::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AlertBelowMedian::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(AlertBelowMedian::AlertId)
                            .integer()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(AlertBelowMedian::ItemId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertBelowMedian::WorldSelector)
                            .json()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertBelowMedian::PercentBelow)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertBelowMedian::HqOnly)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_alert_below_median_alert_id")
                            .from(AlertBelowMedian::Table, AlertBelowMedian::AlertId)
                            .to(Alert::Table, Alert::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(AlertBackInStock::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AlertBackInStock::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(AlertBackInStock::AlertId)
                            .integer()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(AlertBackInStock::ItemId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertBackInStock::WorldSelector)
                            .json()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AlertBackInStock::HqOnly)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_alert_back_in_stock_alert_id")
                            .from(AlertBackInStock::Table, AlertBackInStock::AlertId)
                            .to(Alert::Table, Alert::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AlertBackInStock::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(AlertBelowMedian::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum AlertBelowMedian {
    Table,
    Id,
    AlertId,
    ItemId,
    WorldSelector,
    PercentBelow,
    HqOnly,
}

#[derive(DeriveIden)]
enum AlertBackInStock {
    Table,
    Id,
    AlertId,
    ItemId,
    WorldSelector,
    HqOnly,
}
