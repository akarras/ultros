use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(SaleHistory::Table)
                    .add_column(
                        ColumnDef::new(SaleHistory::WorldId)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(SaleHistory::Table, SaleHistory::WorldId)
                    .to(World::Table, World::Id)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(SaleHistory::Table)
                    .drop_column(SaleHistory::WorldId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(Iden)]
enum World {
    Table,
    Id,
}

#[derive(Iden)]
pub(crate) enum SaleHistory {
    Table,
    WorldId,
}
