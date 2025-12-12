
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ListItem::Table)
                    .add_column(ColumnDef::new(ListItem::Hq).boolean())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ListItem::Table)
                    .drop_column(ListItem::Hq)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum ListItem {
    Table,
    Hq,
}
