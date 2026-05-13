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
                    .table(AlertListUpdate::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AlertListUpdate::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(AlertListUpdate::AlertId)
                            .integer()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(AlertListUpdate::ListId).integer().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_alert_list_update_alert_id")
                            .from(AlertListUpdate::Table, AlertListUpdate::AlertId)
                            .to(Alert::Table, Alert::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_alert_list_update_list_id")
                            .from(AlertListUpdate::Table, AlertListUpdate::ListId)
                            .to(List::Table, List::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_alert_list_update_list_id")
                    .table(AlertListUpdate::Table)
                    .col(AlertListUpdate::ListId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AlertListUpdate::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum AlertListUpdate {
    Table,
    Id,
    AlertId,
    ListId,
}

#[derive(DeriveIden)]
enum List {
    Table,
    Id,
}
