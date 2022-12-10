use crate::{
    entity::{
        sale_history::{self, Model},
    },
    UltrosDb,
};
use anyhow::Result;
use chrono::{Duration, NaiveDateTime};

use futures::Stream;
use migration::{
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set},
    DbErr,
};
use sea_orm::{
    DbBackend, FromQueryResult, QueryOrder, QuerySelect, Statement,
};
use tracing::{instrument};
use universalis::{websocket::event_types::SaleView, ItemId, WorldId};

impl UltrosDb {
    /// Stores a sale from a given sale view.
    /// Demands that a world name for the sale is provided as it is optional on the sale view, but can be determined other ways
    #[instrument(skip(self))]
    pub async fn store_sale(
        &self,
        mut sales: Vec<SaleView>,
        item_id: ItemId,
        world_id: WorldId,
    ) -> Result<Vec<Model>> {
        use sale_history::*;
        // check if the sales have already been logged
        if sales.is_empty() {
            return Ok(vec![]);
        }
        // just nudge the timestamps to not be exactly aligned...
        let mut last_timestamp = None;
        for sale in &mut sales {
            if let Some(t) = last_timestamp {
                if t >= sale.timestamp {
                    sale.timestamp = t;
                    sale.timestamp += Duration::microseconds(1);
                    assert!(sale.timestamp != last_timestamp.unwrap());
                }
            }

            last_timestamp = Some(sale.timestamp.clone());
        }

        // check for any sales that have already been posted
        let last_sale = sales.last().map(|date| date.timestamp);
        if let Some(filter) = last_sale {
            let already_recorded_sales = Entity::find()
                .filter(sale_history::Column::SoldDate.gte(filter))
                .filter(sale_history::Column::WorldId.eq(world_id.0))
                .filter(sale_history::Column::SoldItemId.eq(item_id.0))
                .all(&self.db)
                .await?;
            sales.retain(|sale| {
                !already_recorded_sales.iter().any(|recorded| {
                    
                    sale.hq == recorded.hq
                        && sale.buyer_name == recorded.buyer_name.as_ref().map(|s| s.as_str()).unwrap_or_default()
                        && sale.quantity == recorded.quantity
                        && sale.timestamp.timestamp() == recorded.sold_date.timestamp()
                })
            });
        }
        if sales.is_empty() {
            return Ok(vec![]);
        }
        let mut recorded_sales = vec![];
        let _ = Entity::insert_many(sales.into_iter().map(|sale| {
            let SaleView {
                hq,
                price_per_unit,
                quantity,
                buyer_name,
                ..
            } = sale;
            recorded_sales.push(Model {
                quantity,
                price_per_item: price_per_unit,
                buying_character_id: 0,
                hq,
                sold_item_id: item_id.0,
                sold_date: sale.timestamp.naive_utc(),
                buyer_name: Some(buyer_name.clone()),
                world_id: world_id.0,
            });
            ActiveModel {
                quantity: Set(quantity),
                price_per_item: Set(price_per_unit),
                buying_character_id: Set(0),
                hq: Set(hq),
                sold_item_id: Set(item_id.0),
                sold_date: Set(sale.timestamp.naive_utc()),
                world_id: Set(world_id.0),
                buyer_name: Set(Some(buyer_name)),
            }
        }))
        .exec_without_returning(&self.db)
        .await?;
        Ok(recorded_sales)
    }

    pub async fn get_sale_history_from_multiple_worlds(
        &self,
        world_ids: impl Iterator<Item = i32>,
        item_id: i32,
        limit: u64,
    ) -> Result<
        Vec<sale_history::Model>,
        anyhow::Error,
    > {
        let all = futures::future::try_join_all(
            world_ids
                .map(|world_id| self.get_sale_history_with_character(world_id, item_id, limit)),
        )
        .await;
        
        let mut val: Vec<
            sale_history::Model,
        > = all?.into_iter().flat_map(|w| w.into_iter()).collect();
        val.sort_by_key(|sale| std::cmp::Reverse(sale.sold_date));
        val.truncate(limit as usize);
        Ok(val)
    }

    pub async fn get_sale_history_with_character(
        &self,
        world_id: i32,
        item_id: i32,
        limit: u64,
    ) -> Result<Vec<sale_history::Model>,
        anyhow::Error> {
        Ok(sale_history::Entity::find()
            .filter(sale_history::Column::SoldItemId.eq(item_id))
            .filter(sale_history::Column::WorldId.eq(world_id))
            .order_by_desc(sale_history::Column::SoldDate)
            .limit(limit)
            .all(&self.db)
            .await?)
    }

    pub async fn get_sale_history_for_multiple_items_worlds_joined_future(
        &self,
        world_ids: impl Iterator<Item = i32>,
        item_ids: impl Iterator<Item = i32> + Clone,
        limit: u64,
    ) -> Result<Vec<Vec<sale_history::Model>>, anyhow::Error> {
        let all = futures::future::join_all(world_ids.flat_map(|world_id| {
            item_ids
                .clone()
                .map(move |item_id| self.get_sale_history_for_item(world_id, item_id, limit))
        }))
        .await;
        let result = all
            .into_iter()
            .collect::<Result<Vec<Vec<sale_history::Model>>, anyhow::Error>>()?;
        Ok(result)
    }

    pub async fn get_sale_history_for_item(
        &self,
        world_id: i32,
        item_id: i32,
        limit: u64,
    ) -> Result<Vec<sale_history::Model>, anyhow::Error> {
        Ok(sale_history::Entity::find()
            .filter(sale_history::Column::SoldItemId.eq(item_id))
            .filter(sale_history::Column::WorldId.eq(world_id))
            .order_by_desc(sale_history::Column::SoldDate)
            .limit(limit)
            .all(&self.db)
            .await?)
    }

    pub async fn last_n_sales(
        &self,
        n_sales: i32,
    ) -> Result<impl Stream<Item = Result<AbbreviatedSaleData, DbErr>> + '_, DbErr> {
        AbbreviatedSaleData::find_by_statement(Statement::from_sql_and_values(
                DbBackend::Postgres,
                r#"SELECT filter.* FROM (SELECT h.sold_item_id, h.hq, h.price_per_item, h.sold_date, h.world_id,
                RANK() OVER (PARTITION BY h.sold_item_id, h.hq, h.world_id ORDER BY h.sold_date DESC) sale_rank
                FROM sale_history h) filter
                WHERE filter.sale_rank <= $1
                "#,
                vec![n_sales.into()],
            ))
            .stream(&self.db)
            .await
    }
}

#[derive(Debug, FromQueryResult)]
pub struct AbbreviatedSaleData {
    pub sold_item_id: i32,
    pub hq: bool,
    pub price_per_item: i32,
    pub sold_date: NaiveDateTime,
    pub world_id: i32,
}
