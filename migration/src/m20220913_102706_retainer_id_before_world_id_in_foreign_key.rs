use sea_orm_migration::prelude::*;

use crate::m20220101_000001_create_table::{ActiveListing, Retainer};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        //alter table active_listing
        //    drop constraint active_listing_retainer_world_id_fkey;
        //alter table active_listing
        //  add constraint active_listing_retainer_world_id_fkey
        //  foreign key (retainer_id, world_id) references retainer (id, world_id);
        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(ActiveListing::Table)
                    .name("active_listing_retainer_world_id_fkey")
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .name("active_listing_retainer_world_id_fkey")
                    .from(
                        ActiveListing::Table,
                        (ActiveListing::RetainerId, ActiveListing::WorldId),
                    )
                    .to(Retainer::Table, (Retainer::Id, Retainer::WorldId))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(ActiveListing::Table)
                    .name("active_listing_retainer_world_id_fkey")
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
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
enum Post {
    Table,
    Id,
    Title,
    Text,
}
