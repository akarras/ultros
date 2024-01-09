use sea_orm_migration::prelude::*;

use crate::m20220101_000001_create_table::SaleHistory;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                IndexDropStatement::new()
                    .name("sale_history_sold_date_index")
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("sale_history_lookup_index")
                    .table(SaleHistory::Table)
                    .col(SaleHistory::SoldItemId)
                    .col(SaleHistory::WorldId)
                    .col((SaleHistory::SoldDate, IndexOrder::Desc))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                IndexDropStatement::new()
                    .name("sale_history_lookup_index")
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("sale_history_sold_date_index")
                    .table(SaleHistory::Table)
                    .col(SaleHistory::SoldDate)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
