use crate::m20220101_000001_create_table::{ActiveListing, Retainer};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                IndexCreateStatement::new()
                    .table(ActiveListing::Table)
                    .name("WorldItemIndex")
                    .col(ActiveListing::ItemId)
                    .col(ActiveListing::WorldId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                IndexCreateStatement::new()
                    .table(Retainer::Table)
                    .name("RetainerWorldIndex")
                    .col(Retainer::UniversalisID)
                    .col(Retainer::WorldId)
                    .col(Retainer::Name)
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
                    .name("WorldItemIndex")
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                IndexDropStatement::new()
                    .table(Retainer::Table)
                    .name("RetainerWorldIndex")
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
