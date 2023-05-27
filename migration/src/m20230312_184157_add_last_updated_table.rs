use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ListingLastUpdated::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ListingLastUpdated::ItemId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ListingLastUpdated::WorldId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ListingLastUpdated::DateTime)
                            .date_time()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(ListingLastUpdated::ItemId)
                            .col(ListingLastUpdated::WorldId),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                IndexCreateStatement::new()
                    .table(ListingLastUpdated::Table)
                    .col(ListingLastUpdated::WorldId)
                    .col((ListingLastUpdated::DateTime, IndexOrder::Desc))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ListingLastUpdated::Table).to_owned())
            .await
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
enum ListingLastUpdated {
    Table,
    ItemId,
    WorldId,
    DateTime,
}
