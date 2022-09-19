use sea_orm_migration::prelude::*;

use crate::m20220101_000001_create_table::ActiveListing;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                IndexCreateStatement::new()
                    .table(ActiveListing::Table)
                    .col(ActiveListing::RetainerId)
                    .name("active_listing_retainer_id_index")
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                IndexDropStatement::new()
                    .table(ActiveListing::Table)
                    .table(ActiveListing::RetainerId)
                    .name("active_listing_retainer_id_index")
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
