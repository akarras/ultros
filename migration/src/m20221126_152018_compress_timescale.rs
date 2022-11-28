use sea_orm_migration::{
    prelude::*,
    sea_orm::{Statement, StatementBuilder},
};

use crate::m20220101_000001_create_table::SaleHistory;

#[derive(DeriveMigrationName)]
pub struct Migration;

struct RawPostgresStatement {
    statement: String,
}

impl StatementBuilder for RawPostgresStatement {
    fn build(&self, db_backend: &sea_orm::DbBackend) -> Statement {
        Statement::from_string(*db_backend, self.statement.clone())
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(SaleHistory::Table)
                    .drop_column(SaleHistory::Id)
                    .add_column(ColumnDef::new(SaleHistory::BuyerName).text())
                    .to_owned(),
            )
            .await?;

        manager
            .exec_stmt(RawPostgresStatement {
                statement:
                    "ALTER TABLE sale_history ADD PRIMARY KEY (sold_date, world_id, sold_item_id);"
                        .to_string(),
            })
            .await?;
        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(SaleHistory::Table)
                    .name("sale_history_buying_character_id_fkey")
                    .to_owned(),
            )
            .await?;
        manager.exec_stmt(RawPostgresStatement{ statement: "ALTER TABLE sale_history SET (timescaledb.compress, timescaledb.compress_orderby = 'sold_date',
        timescaledb.compress_segmentby = 'world_id, sold_item_id'
        );".to_string()}).await?;
        manager.exec_stmt(RawPostgresStatement{ statement: "CREATE MATERIALIZED VIEW sale_summary_daily (world_id, i_id, hq, time, min_price, median_price, max_price, start_price, end_price, number_sold)
            WITH (timescaledb.continuous) AS
            SELECT world_id, sold_item_id, hq, time_bucket('1day', sold_date), min(price_per_item) AS minp, percentile_disc(0.5) WITHIN GROUP (order by price_per_item) AS medianp, max(price_per_item) maxp, FIRST(price_per_item, sold_date) AS start_price, LAST(price_per_item, sold_date) AS end_price, COUNT(*) as number_sold
            FROM sale_history
            GROUP BY time_bucket('1day', sold_date), hq, sold_item_id, world_id".to_string()}).await?;
        Ok(())
        //
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(SaleHistory::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(SaleHistory::Id).auto_increment().integer(),
                    )
                    .to_owned(),
            )
            .await?;

        manager.exec_stmt(RawPostgresStatement{ statement: "ALTER TABLE sale_history SET (timescaledb.compress = false, timescaledb.compress_orderby = '',
        timescaledb.compress_segmentby = 'world_id, sold_item_id'
        );".to_string()}).await?;
        Ok(())
    }
}
