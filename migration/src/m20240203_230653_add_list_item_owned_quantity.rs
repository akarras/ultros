use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(ListItem::Table)
                    .add_column_if_not_exists(ColumnDef::new(ListItem::Acquired).unsigned().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(ListItem::Table)
                    .drop_column(ListItem::Acquired)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum ListItem {
    Table,
    Acquired,
}
