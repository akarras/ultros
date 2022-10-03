use sea_orm_migration::prelude::*;

use crate::m20220101_000001_create_table::SaleHistory;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("sale_history_sold_date_index")
                    .table(SaleHistory::Table)
                    .col(SaleHistory::SoldDate)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                IndexDropStatement::new()
                    .table(SaleHistory::Table)
                    .name("sale_history_sold_date_index")
                    .to_owned(),
            )
            .await
    }
}
