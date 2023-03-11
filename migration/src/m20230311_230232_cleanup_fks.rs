use sea_orm_migration::prelude::*;

use crate::m20220101_000001_create_table::{ActiveListing, Retainer, SaleHistory};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .name("active_listing_retainer_world_id_fkey")
                    .table(ActiveListing::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                IndexDropStatement::new()
                    .name("UniqueRetainerIdWorld")
                    .table(Retainer::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .name("active_listing_retainer_fkey")
                    .from(ActiveListing::Table, ActiveListing::RetainerId)
                    .to(Retainer::Table, Retainer::Id)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                IndexDropStatement::new()
                    .name("sale_history_pkey")
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("sale_history_pkey")
                    .table(SaleHistory::Table)
                    .col(SaleHistory::Id)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Replace the sample below with your own migration scripts
        // todo!();

        // manager
        //     .drop_table(Table::drop().table(Post::Table).to_owned())
        //     .await
        Ok(())
    }
}
