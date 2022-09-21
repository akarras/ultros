use chrono::Duration;
use chrono::Utc;
use migration::Alias;
use migration::BinOper;
use migration::ColumnRef;
use migration::Expr;
use migration::Query;
use migration::SeaRc;
use migration::SimpleExpr;
use tracing::instrument;

use crate::entity::*;
use crate::UltrosDb;
use anyhow::Result;
use sea_orm::*;

#[derive(FromQueryResult)]
pub struct BestResellResults {
    pub item_id: i32,
    pub profit: i32,
    pub margin: i32,
}

impl UltrosDb {
    /// Tries to calculate what the best item to resell for the given world is
    /// Assumes that the user is willing to travel to all worlds in the region
    /// Parameters:
    /// * world_id - World you want to sale items on
    /// * sale_amount_threshold - How many recent sales within the window should have occured (see next argument)
    /// * sale_window - How long ago should sales be considered for this query
    #[instrument]
    pub async fn get_best_item_to_resell_on_world(
        &self,
        world_id: i32,
        sale_amount_threshold: i32,
        sale_window: Duration,
    ) -> Result<Vec<BestResellResults>> {
        let min_sale_price_alias: DynIden = SeaRc::new(Alias::new("min_sale_price"));
        let world_sale_history_query = Query::select()
            .from(sale_history::Entity)
            .column(sale_history::Column::SoldItemId)
            .expr_as(
                sale_history::Column::PricePerItem.min(),
                min_sale_price_alias.clone(),
            )
            .and_where(sale_history::Column::WorldId.eq(world_id))
            .and_where(sale_history::Column::SoldDate.gt(Utc::now() - sale_window))
            .and_having(
                Expr::val(sale_amount_threshold)
                    .less_than(Expr::col(sale_history::Column::Quantity).sum()),
            )
            .group_by_col(sale_history::Column::SoldItemId)
            .to_owned();
        let all_worlds_in_region_query = Query::select()
            .from(world::Entity)
            .column(world::Column::Id)
            .and_where(
                world::Column::DatacenterId.in_subquery(
                    Query::select()
                        .from(datacenter::Entity)
                        .column(datacenter::Column::Id)
                        .and_where(
                            datacenter::Column::Id.in_subquery(
                                Query::select()
                                    .column(world::Column::DatacenterId)
                                    .and_where(world::Column::Id.eq(world_id))
                                    .from(world::Entity)
                                    .to_owned(),
                            ),
                        )
                        .to_owned(),
                ),
            )
            .to_owned();
        let query_iden: DynIden = SeaRc::new(Alias::new("sale_hist"));
        let profit: DynIden = SeaRc::new(Alias::new("profit"));
        let margin: DynIden = SeaRc::new(Alias::new("margin"));

        let all_query = Query::select()
            .from(active_listing::Entity)
            .column(active_listing::Column::ItemId)
            .expr_as(
                SimpleExpr::Column(ColumnRef::TableColumn(
                    query_iden.clone(),
                    min_sale_price_alias.clone(),
                ))
                .sub(active_listing::Column::PricePerUnit.min()),
                profit.clone(),
            )
            .expr_as(
                SimpleExpr::Binary(
                    Box::new(SimpleExpr::Binary(
                        Box::new(
                            SimpleExpr::Column(ColumnRef::TableColumn(
                                query_iden.clone(),
                                min_sale_price_alias.clone(),
                            )),
                        ),
                        BinOper::Div,
                        Box::new(
                            active_listing::Column::PricePerUnit
                                .min(),
                        ),
                    )),
                    BinOper::Mul,
                    Box::new(SimpleExpr::Value(Value::Float(Some(100.0)))),
                ),
                margin.clone(),
            )
            .join_subquery(
                JoinType::InnerJoin,
                world_sale_history_query,
                query_iden.clone(),
                Expr::tbl(active_listing::Entity, active_listing::Column::ItemId)
                    .equals(query_iden.clone(), sale_history::Column::SoldItemId),
            )
            .and_where(active_listing::Column::WorldId.in_subquery(all_worlds_in_region_query))
            .group_by_col(active_listing::Column::ItemId)
            .group_by_col((query_iden.clone(), min_sale_price_alias.clone()))
            .limit(10)
            .order_by(profit, Order::Desc)
            .to_owned();
        let query = self.db.get_database_backend().build(&all_query);
        let results = BestResellResults::find_by_statement(query)
            .all(&self.db)
            .await?;

        // now find sale history for *all* items in our server
        Ok(results)
    }
}
