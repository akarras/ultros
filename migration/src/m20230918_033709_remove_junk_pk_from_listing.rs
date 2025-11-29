use sea_orm_migration::prelude::*;

use crate::m20220101_000001_create_table::{ActiveListing, SaleHistory};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() == sea_orm_migration::sea_orm::DbBackend::Sqlite {
            return Ok(());
        }
        // alter table sale_history
        // drop constraint sale_history_pkey;

        // alter table sale_history
        //  add primary key (id);
        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(SaleHistory::Table)
                    .name("sale_history_pkey")
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                TableAlterStatement::new()
                    .modify_column(ColumnDef::new(SaleHistory::Id).primary_key())
                    .table(SaleHistory::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(ActiveListing::Table)
                    .name("active_listing_pkey")
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                TableAlterStatement::new()
                    .modify_column(ColumnDef::new(ActiveListing::Id).primary_key())
                    .table(ActiveListing::Table)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Replace the sample below with your own migration scripts
        todo!();
    }
}
