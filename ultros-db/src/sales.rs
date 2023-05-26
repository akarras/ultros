use std::collections::{HashMap, HashSet};

use crate::{
    common_type_conversions::SaleHistoryReturn,
    entity::{sale_history, unknown_final_fantasy_character},
    UltrosDb,
};
use anyhow::Result;
use chrono::NaiveDateTime;

use futures::{future::try_join_all, Stream};
use migration::{
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set},
    DbErr,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue, DbBackend, FromQueryResult, QueryOrder, QuerySelect, Statement,
};
use tracing::instrument;
use ultros_api_types::{SaleHistory, UnknownCharacter};
use universalis::{ItemId, SaleView, WorldId};

impl UltrosDb {
    /// Stores a sale from a given sale view.
    /// Demands that a world name for the sale is provided as it is optional on the sale view, but can be determined other ways
    #[instrument(skip(self, sales))]
    pub async fn update_sales(
        &self,
        mut sales: Vec<SaleView>,
        item_id: ItemId,
        world_id: WorldId,
    ) -> Result<Vec<(SaleHistory, UnknownCharacter)>> {
        use sale_history::*;
        // check if the sales have already been logged
        if sales.is_empty() {
            return Ok(vec![]);
        }

        // check for any sales that have already been posted
        let already_recorded_sales = Entity::find()
            .filter(sale_history::Column::SoldItemId.eq(item_id.0))
            .filter(sale_history::Column::WorldId.eq(world_id.0))
            .order_by_desc(sale_history::Column::SoldDate)
            .limit(sales.len() as u64)
            .all(&self.db)
            .await?;
        let buyers = self.lookup_buyer_names(&sales).await?;
        sales.retain(|sale| {
            let buyer = buyers
                .get(&sale.buyer_name)
                .expect("Should always have gotten a buyer model");
            !already_recorded_sales.iter().any(|recorded| {
                sale.hq == recorded.hq
                    && buyer.id == recorded.buying_character_id
                    && sale.quantity == recorded.quantity
                    && sale.timestamp.timestamp() == recorded.sold_date.timestamp()
            })
        });
        if sales.is_empty() {
            return Ok(vec![]);
        }
        let mut recorded_sales = vec![];
        let _ = Entity::insert_many(sales.into_iter().map(|sale| {
            let buyer = buyers
                .get(&sale.buyer_name)
                .expect("Should always have a buyer model");
            let SaleView {
                hq,
                price_per_unit,
                quantity,
                ..
            } = sale;
            let record: SaleHistory = SaleHistoryReturn(
                Model {
                    id: 0,
                    quantity,
                    price_per_item: price_per_unit,
                    buying_character_id: buyer.id,
                    hq,
                    sold_item_id: item_id.0,
                    sold_date: sale.timestamp.naive_utc(),
                    world_id: world_id.0,
                },
                Some(buyer.clone()),
            )
            .into();
            recorded_sales.push((record, buyer.into()));
            ActiveModel {
                id: Default::default(),
                quantity: Set(quantity),
                price_per_item: Set(price_per_unit),
                buying_character_id: Set(buyer.id),
                hq: Set(hq),
                sold_item_id: Set(item_id.0),
                sold_date: Set(sale.timestamp.naive_utc()),
                world_id: Set(world_id.0),
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
    ) -> Result<Vec<SaleHistoryReturn>, anyhow::Error> {
        let all = futures::future::try_join_all(
            world_ids
                .map(|world_id| self.get_sale_history_with_character(world_id, item_id, limit)),
        )
        .await;

        let mut val: Vec<SaleHistoryReturn> =
            all?.into_iter().flat_map(|w| w.into_iter()).collect();
        val.sort_by_key(|sale| std::cmp::Reverse(sale.0.sold_date));
        val.truncate(limit as usize);
        Ok(val)
    }

    async fn lookup_buyer_names(
        &self,
        sales: &[SaleView],
    ) -> Result<HashMap<String, unknown_final_fantasy_character::Model>, anyhow::Error> {
        // get all the unique buyer names
        let buyers: HashSet<_> = sales.iter().map(|b| &b.buyer_name).collect();
        Ok(try_join_all(buyers.into_iter().map(|name| async move {
            let buyer = unknown_final_fantasy_character::Entity::find()
                .filter(unknown_final_fantasy_character::Column::Name.eq(name))
                .one(&self.db)
                .await?;
            let buyer = match buyer {
                Some(buyer) => buyer,
                None => {
                    unknown_final_fantasy_character::ActiveModel {
                        name: ActiveValue::Set(name.to_string()),
                        ..Default::default()
                    }
                    .insert(&self.db)
                    .await?
                }
            };
            Ok::<_, anyhow::Error>((buyer.name.clone(), buyer))
        }))
        .await?
        .into_iter()
        .collect())
    }

    pub async fn get_sale_history_with_character(
        &self,
        world_id: i32,
        item_id: i32,
        limit: u64,
    ) -> Result<Vec<SaleHistoryReturn>, anyhow::Error> {
        Ok(sale_history::Entity::find()
            .filter(sale_history::Column::SoldItemId.eq(item_id))
            .filter(sale_history::Column::WorldId.eq(world_id))
            .order_by_desc(sale_history::Column::SoldDate)
            .find_also_related(unknown_final_fantasy_character::Entity)
            .limit(limit)
            .all(&self.db)
            .await?
            .into_iter()
            .map(|(sale, character)| SaleHistoryReturn(sale, character))
            .collect())
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
