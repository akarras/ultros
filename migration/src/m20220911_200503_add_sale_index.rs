use crate::m20220101_000001_create_table::SaleHistory;
use crate::m20220908_170456_add_world_id_to_sale::SaleHistory as Sale2;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                IndexCreateStatement::new()
                    .table(SaleHistory::Table)
                    .name("SaleHistoryFullIndex")
                    .col(SaleHistory::PricePerItem)
                    .col(SaleHistory::Quantity)
                    .col(SaleHistory::Hq)
                    .col(SaleHistory::BuyingCharacterId)
                    .col(SaleHistory::SoldItemId)
                    .col(Sale2::WorldId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                IndexDropStatement::new()
                    .table(SaleHistory::Table)
                    .name("SaleHistoryFullIndex")
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
