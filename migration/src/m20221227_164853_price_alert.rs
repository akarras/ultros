use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add the List, Item, and Price Alert tables.
        manager
            .create_table(
                Table::create()
                    .table(List::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(List::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(List::Owner).integer().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(ListItem::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ListItem::Id)
                            .integer()
                            .not_null()
                            .primary_key()
                            .auto_increment(),
                    )
                    .col(ColumnDef::new(ListItem::ItemId).integer().not_null())
                    .col(ColumnDef::new(ListItem::ListId).integer().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(PriceAlert::Table)
                    .col(
                        ColumnDef::new(PriceAlert::Id)
                            .integer()
                            .not_null()
                            .primary_key()
                            .auto_increment(),
                    )
                    .col(ColumnDef::new(PriceAlert::ListId).integer().not_null())
                    .col(ColumnDef::new(PriceAlert::AlertId).integer().not_null())
                    .col(ColumnDef::new(PriceAlert::WorldId).integer())
                    .col(ColumnDef::new(PriceAlert::DatacenterId).integer())
                    .col(ColumnDef::new(PriceAlert::RegionId).integer())
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PriceAlert::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ListItem::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(List::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(Iden)]
enum List {
    Table,
    Id,
    Owner,
}

#[derive(Iden)]
enum ListItem {
    Table,
    Id,
    ListId,
    ItemId,
}

#[derive(Iden)]
enum PriceAlert {
    Table,
    Id,
    AlertId,
    ListId,
    WorldId,
    RegionId,
    DatacenterId,
}
