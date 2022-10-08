use std::collections::HashSet;

use crate::{
    entity::{
        sale_history::{self, Model},
        unknown_final_fantasy_character,
    },
    UltrosDb,
};
use anyhow::Result;
use migration::sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, Set,
};
use tracing::instrument;
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
        use unknown_final_fantasy_character::Column as FFColumn;
        // check if the sales have already been logged
        if sales.is_empty() {
            return Ok(vec![]);
        }
        // first upsert characters for each of the sales
        let buyer_names: HashSet<_> = sales.iter().map(|m| m.buyer_name.to_string()).collect();
        let filter_expression = buyer_names
            .iter()
            .map(|name| FFColumn::Name.eq(name.as_str()))
            .reduce(|inc, out| inc.or(out))
            .ok_or_else(|| anyhow::Error::msg("No characters inserted?"))?;
        let mut characters = unknown_final_fantasy_character::Entity::find()
            .filter(filter_expression)
            .all(&self.db)
            .await?;

        // fill in the rest of the characters
        for name in buyer_names {
            if !characters.iter().any(|m| m.name == name) {
                let character = unknown_final_fantasy_character::ActiveModel {
                    id: ActiveValue::default(),
                    name: Set(name),
                }
                .insert(&self.db)
                .await?;
                characters.push(character);
            }
        }

        // check for any sales that have already been posted
        let filter = sales
            .iter()
            .filter(|sale| sale.timestamp.timestamp_millis() != 0)
            .map(|sale| {
                let id = characters
                    .iter()
                    .find(|character| character.name == sale.buyer_name)
                    .map(|c| c.id)
                    .expect("Should know all characters");
                Column::WorldId
                    .eq(world_id.0)
                    .and(Column::SoldDate.eq(sale.timestamp))
                    .and(
                        Column::BuyingCharacterId
                            .eq(id)
                            .and(Column::SoldItemId.eq(item_id.0)),
                    )
                    .and(Column::PricePerItem.eq(sale.price_per_unit))
                    .and(Column::Quantity.eq(sale.quantity))
                    .and(Column::Hq.eq(sale.hq))
            })
            .reduce(|a, b| a.or(b));
        if let Some(filter) = filter {
            let already_recorded_sales = Entity::find().filter(filter).all(&self.db).await?;
            sales = sales
                .into_iter()
                .filter(|sale| {
                    !already_recorded_sales.iter().any(|recorded| {
                        let buyer_id = characters
                            .iter()
                            .find(|c| c.name == sale.buyer_name)
                            .map(|m| m.id)
                            .expect("Should know all characters");
                        sale.hq == recorded.hq
                            && buyer_id == recorded.buying_character_id
                            && sale.quantity == recorded.quantity
                            && sale.timestamp.timestamp() == recorded.sold_date.timestamp()
                    })
                })
                .collect();
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
            let character_id = characters
                .iter()
                .find(|character| character.name == buyer_name)
                .map(|c| c.id)
                .expect("Shouldn't be able to have a character not in the list");
            recorded_sales.push(Model {
                id: 0,
                quantity,
                price_per_item: price_per_unit,
                buying_character_id: character_id,
                hq,
                sold_item_id: item_id.0,
                sold_date: sale.timestamp.naive_utc(),
                world_id: world_id.0,
            });
            ActiveModel {
                id: ActiveValue::default(),
                quantity: Set(quantity),
                price_per_item: Set(price_per_unit),
                buying_character_id: Set(character_id),
                hq: Set(hq),
                sold_item_id: Set(item_id.0),
                sold_date: Set(sale.timestamp.naive_utc()),
                world_id: Set(world_id.0),
            }
        }))
        .exec(&self.db)
        .await?;
        Ok(recorded_sales)
    }
}
