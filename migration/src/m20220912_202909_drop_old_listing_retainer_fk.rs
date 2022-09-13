use crate::m20220101_000001_create_table::{ActiveListing, Retainer};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(ActiveListing::Table)
                    .name("active_listing_retainer_id_fkey")
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(ActiveListing::Table, ActiveListing::RetainerId)
                    .to(Retainer::Table, Retainer::Id)
                    .to_owned(),
            )
            .await
    }
}
