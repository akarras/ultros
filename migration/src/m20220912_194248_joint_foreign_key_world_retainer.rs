use sea_orm_migration::prelude::*;

use crate::m20220101_000001_create_table::{ActiveListing, Retainer};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                IndexCreateStatement::new()
                    .if_not_exists()
                    .name("UniqueRetainerNamePerWorld")
                    .table(Retainer::Table)
                    .col(Retainer::WorldId)
                    .col(Retainer::Name)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                IndexCreateStatement::new()
                    .if_not_exists()
                    .name("UniqueRetainerIdWorld")
                    .table(Retainer::Table)
                    .col(Retainer::Id)
                    .col(Retainer::WorldId)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .name("active_listing_retainer_world_id_fkey")
                    .from(
                        ActiveListing::Table,
                        (ActiveListing::WorldId, ActiveListing::RetainerId),
                    )
                    .to(Retainer::Table, (Retainer::WorldId, Retainer::Id))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                IndexDropStatement::new()
                    .table(Retainer::Table)
                    .name("UniqueRetainerNamePerWorld")
                    .to_owned(),
            )
            .await?;
        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(ActiveListing::Table)
                    .name("active_listing_retainer_world_id_fkey")
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
        Ok(())
    }
}
