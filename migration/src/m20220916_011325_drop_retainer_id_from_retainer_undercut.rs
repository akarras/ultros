use sea_orm_migration::prelude::*;

use crate::m20220101_000001_create_table::AlertRetainerUndercut;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AlertRetainerUndercut::Table)
                    .drop_column(AlertRetainerUndercut::RetainerId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AlertRetainerUndercut::Table)
                    .add_column(ColumnDef::new(AlertRetainerUndercut::RetainerId).integer())
                    .to_owned(),
            )
            .await
    }
}
