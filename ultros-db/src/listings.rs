use anyhow::Result;
use migration::{Order, Value};
use sea_orm::{
    ColumnTrait, EntityTrait, ModelTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait,
};
use std::collections::HashSet;
use tracing::{info, instrument, warn};
use universalis::{ItemId, ListingView, WorldId};

use crate::{
    entity::{active_listing, retainer},
    UltrosDb,
};

impl UltrosDb {
    /// Updates listings assuming a pure view of the listing board
    #[instrument(skip(self))]
    pub async fn update_listings(
        &self,
        mut listings: Vec<ListingView>,
        item_id: ItemId,
        world_id: WorldId,
    ) -> Result<(Vec<active_listing::Model>, i32)> {
        use active_listing::*;
        // Assumes that we are being given a full list of all the listings for the item and world.
        // First, query the db to see what listings it has
        // Then diff against the listings that we have
        listings.sort_by(|a, b| {
            a.hq.cmp(&b.hq)
                .then_with(|| a.quantity.cmp(&b.quantity))
                .then_with(|| a.price_per_unit.cmp(&b.price_per_unit))
                .then_with(|| a.retainer_name.cmp(&b.retainer_name))
        });

        let queried_retainers: HashSet<(String, String, i32)> = listings
            .iter()
            .map(|listing| {
                (
                    listing.retainer_name.to_string(),
                    listing.retainer_id.clone(),
                    listing.retainer_city as i32,
                )
            })
            .collect();

        let mut retainers = self
            .get_retainer_ids_from_name(
                queried_retainers.iter().map(|(name, id, _)| name.as_str()),
                world_id.0,
            )
            .await?;
        // determine missing retainers
        for (name, id, retainer_city) in queried_retainers {
            if !retainers.iter().any(|m| m.name == name) {
                let retainer = self
                    .store_retainer(&id, &name, world_id, retainer_city as i32)
                    .await?;
                retainers.push(retainer);
            }
        }
        let mut existing_items = Entity::find()
            .filter(
                Column::WorldId
                    .eq(world_id.0)
                    .and(Column::ItemId.eq(item_id.0)),
            )
            .find_also_related(retainer::Entity)
            .all(&self.db)
            .await?;
        existing_items.sort_by(|(listinga, retainera), (listingb, retainerb)| {
            let retainer_name_a = retainera
                .as_ref()
                .map(|m| m.name.as_str())
                .unwrap_or_default();
            let retainer_name_b = retainerb
                .as_ref()
                .map(|m| m.name.as_str())
                .unwrap_or_default();
            listinga
                .hq
                .cmp(&listingb.hq)
                .then_with(|| listinga.quantity.cmp(&listingb.quantity))
                .then_with(|| listinga.price_per_unit.cmp(&listingb.price_per_unit))
                .then_with(|| retainer_name_a.cmp(retainer_name_b))
        });
        let mut incoming_iter = listings.into_iter();
        let mut db_iter = existing_items.into_iter();
        // compare each item, then advance the list
        let mut incoming_list = incoming_iter.next();
        let mut db_value = db_iter.next();
        let mut added = vec![];
        let mut removed = vec![];
        loop {
            match (incoming_list, db_value) {
                (Some(list), None) => {
                    added.push(list);
                    incoming_list = incoming_iter.next();
                    db_value = None;
                }
                (None, Some(model)) => {
                    removed.push(model);
                    incoming_list = None;
                    db_value = db_iter.next();
                }
                (Some(list), Some((model, retainer))) => {
                    let price_per_unit = list.price_per_unit.unwrap_or(list.total) as i32;
                    let quantity = list.quantity.unwrap_or(1) as i32;
                    let retainer_name = retainer
                        .as_ref()
                        .map(|r| r.name.as_str())
                        .unwrap_or_default();
                    match price_per_unit
                        .cmp(&model.price_per_unit)
                        .then_with(|| quantity.cmp(&model.quantity))
                        .then_with(|| list.retainer_name.as_str().cmp(retainer_name))
                        .then_with(|| list.hq.cmp(&model.hq))
                    {
                        std::cmp::Ordering::Less => {
                            added.push(list);
                            incoming_list = incoming_iter.next();
                            db_value = Some((model, retainer));
                        }
                        std::cmp::Ordering::Equal => {
                            // item in list, keep checking list
                            db_value = db_iter.next();
                            incoming_list = incoming_iter.next();
                        }
                        std::cmp::Ordering::Greater => {
                            removed.push((model, retainer));
                            incoming_list = Some(list);
                            db_value = db_iter.next();
                        }
                    }
                }
                (None, None) => {
                    // lists exhausted, exit this loop
                    break;
                }
            }
        }

        // let remove_ids = removed
        //     .into_iter()
        //     .map(|i| Column::Id.eq(i.id))
        //     .reduce(|a, b| a.or(b));
        let is_in = if removed.is_empty() {
            None
        } else {
            Some(Column::Id.is_in(removed.into_iter().map(|(m, _)| Value::Int(Some(m.id)))))
        };
        let added = added.iter().map(|m| {
            let retainer_id = retainers
                .iter()
                .find(|r| r.name == m.retainer_name)
                .expect("Should always have a retainer at this point.")
                .id;
            self.create_listing(m, item_id, world_id, Some(retainer_id))
        });
        let (added, removed) =
            futures::future::join(futures::future::join_all(added), async move {
                if let Some(is_in) = is_in {
                    Entity::delete_many()
                        .filter(is_in)
                        .exec(&self.db)
                        .await
                        .map(|i| i.rows_affected)
                } else {
                    Ok(0)
                }
            })
            .await;

        let added = added.into_iter().flatten().collect();
        Ok((added, removed? as i32))
    }
}
