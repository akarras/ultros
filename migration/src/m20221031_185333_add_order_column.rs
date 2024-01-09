use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(OwnedRetainers::Table)
                    .add_column(ColumnDef::new(OwnedRetainers::Weight).integer())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .drop_column(OwnedRetainers::Weight)
                    .table(OwnedRetainers::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(Iden)]
pub enum OwnedRetainers {
    Table,
    Weight,
}
