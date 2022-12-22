use anyhow::Result;
use futures::{
    future::{join_all, try_join_all},
    Stream,
};
use migration::DbErr;
use sea_orm::{ColumnTrait, DbBackend, EntityTrait, FromQueryResult, QueryFilter, Statement};
use std::collections::HashSet;
use tracing::instrument;
use universalis::{ItemId, ListingView, WorldId};

use crate::{
    entity::{active_listing, retainer},
    partial_diff_iterator::PartialDiffIterator,
    UltrosDb,
};

impl PartialEq<ListingView> for ListingData {
    fn eq(&self, other: &ListingView) -> bool {
        self.0.world_id == other.world_id.unwrap_or_default() as i32
            && self.0.price_per_unit == other.price_per_unit.unwrap_or_default() as i32
            && self.0.quantity == other.quantity.unwrap_or_default() as i32
            && self.0.hq == other.hq
            && self.1.name == other.retainer_name
        // timestamp intentionally ignored
    }
}

struct ListingData(active_listing::Model, retainer::Model);

impl PartialOrd<ListingView> for ListingData {
    fn partial_cmp(&self, other: &ListingView) -> Option<std::cmp::Ordering> {
        let ListingData(listing, retainer) = self;
        match (listing.world_id as u16).partial_cmp(&other.world_id.unwrap_or_default()) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        match retainer.name.partial_cmp(&other.retainer_name) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        match listing
            .price_per_unit
            .partial_cmp(&(other.price_per_unit.unwrap_or_default() as i32))
        {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        match listing
            .quantity
            .partial_cmp(&(other.quantity.unwrap_or_default() as i32))
        {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord,
        }
        listing.hq.partial_cmp(&other.hq)
    }
}

impl UltrosDb {
    pub async fn remove_listings(
        &self,
        remove_listings: Vec<ListingView>,
        item_id: ItemId,
        world_id: WorldId,
    ) -> Result<u64> {
        let listings = self
            .get_all_listings_with_retainers(world_id.0, item_id)
            .await?;

        let items = join_all(
            PartialDiffIterator::new(
                listings
                    .into_iter()
                    .flat_map(|(listing, retainer)| retainer.map(|r| ListingData(listing, r))),
                remove_listings.into_iter(),
            )
            .flat_map(|listing| match listing {
                crate::partial_diff_iterator::Diff::Same(listing, _) => Some(listing.0),
                _ => None,
            })
            .map(|listing| {
                active_listing::Entity::delete_by_id((listing.id, listing.timestamp)).exec(&self.db)
            }),
        )
        .await;

        Ok(items.len() as u64)
    }

    #[instrument(skip(self))]
    pub async fn get_all_listings_in_worlds_with_retainers(
        &self,
        worlds: &Vec<i32>,
        item: ItemId,
    ) -> Result<Vec<(active_listing::Model, Option<retainer::Model>)>> {
        let result = try_join_all(
            worlds
                .into_iter()
                .map(|world| self.get_all_listings_with_retainers(*world, item)),
        )
        .await?;
        let data = result.into_iter().flat_map(|s| s.into_iter()).collect();
        Ok(data)
    }

    #[instrument(skip(self))]
    pub async fn get_all_listings_with_retainers(
        &self,
        world: i32,
        item: ItemId,
    ) -> Result<Vec<(active_listing::Model, Option<retainer::Model>)>> {
        use active_listing::*;
        Ok(Entity::find()
            .filter(Column::ItemId.eq(item.0))
            .filter(Column::WorldId.eq(world))
            .find_also_related(retainer::Entity)
            .all(&self.db)
            .await?)
    }

    /// Updates listings assuming a pure view of the listing board
    #[instrument(skip(self))]
    pub async fn update_listings(
        &self,
        mut listings: Vec<ListingView>,
        item_id: ItemId,
        world_id: WorldId,
    ) -> Result<(Vec<active_listing::Model>, Vec<active_listing::Model>)> {
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
                queried_retainers.iter().map(|(name, _, _)| name.as_str()),
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
        let remove_iter = removed.iter();
        let added = added.iter().map(|m| {
            let retainer_id = retainers
                .iter()
                .find(|r| r.name == m.retainer_name)
                .expect("Should always have a retainer at this point.")
                .id;
            self.create_listing(m, item_id, world_id, Some(retainer_id))
        });
        let (added, _removed_result) =
            futures::future::join(futures::future::join_all(added), async move {
                let remove_result = futures::future::try_join_all(remove_iter.map(|(l, _)| {
                    active_listing::Entity::delete_by_id((l.id, l.timestamp)).exec(&self.db)
                }))
                .await?;
                Result::<usize>::Ok(remove_result.len())
            })
            .await;

        let added = added.into_iter().flatten().collect();
        Ok((added, removed.into_iter().map(|(m, _)| m).collect()))
    }

    pub async fn get_all_listings_for_world(
        &self,
        world_id: i32,
    ) -> Result<impl Stream<Item = Result<active_listing::Model, DbErr>> + '_, anyhow::Error> {
        Ok(active_listing::Entity::find()
            .filter(active_listing::Column::WorldId.eq(world_id))
            .stream(&self.db)
            .await?)
    }

    pub async fn cheapest_listings(
        &self,
    ) -> Result<impl Stream<Item = Result<ListingSummary, DbErr>> + '_, DbErr> {
        ListingSummary::find_by_statement(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"SELECT ranks.* FROM (SELECT l.item_id, l.hq, l.price_per_unit, l.world_id,
                RANK() OVER (PARTITION BY l.item_id, l.hq, l.world_id ORDER BY l.price_per_unit ASC) listing_rank
                FROM active_listing l) ranks
                WHERE ranks.listing_rank = 1"#,
            vec![],
        )).stream(&self.db).await
    }
}

#[derive(Debug, FromQueryResult)]
pub struct ListingSummary {
    pub item_id: i32,
    pub hq: bool,
    pub price_per_unit: i32,
    pub world_id: i32,
}
