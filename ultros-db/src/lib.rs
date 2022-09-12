mod entity;
pub(crate) mod partial_diff_iterator;
mod ffxiv_character;
mod regions_and_datacenters;


use anyhow::Result;
use chrono::prelude::Local;
use migration::{Migrator, MigratorTrait};
use sea_orm::{
    ActiveValue, Order, QueryOrder, RelationTrait,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectOptions, Database,
    DatabaseConnection, EntityTrait, IntoActiveModel, ModelTrait, QueryFilter, QuerySelect, Set,
};
use std::collections::HashSet;
use tracing::{info};
use universalis::{websocket::event_types::SaleView, ItemId, ListingView, WorldId};

use crate::entity::*;

#[derive(Clone, Debug)]
pub struct UltrosDb {
    // Connections here
    db: DatabaseConnection,
}

impl UltrosDb {
    pub async fn connect() -> Result<Self> {
        let url = std::env::var("DATABASE_URL").unwrap();
        let mut opt = ConnectOptions::new(url);
        opt.max_connections(90)
            .min_connections(0)
            // .connect_timeout(Duration::from_secs(8))
            // .idle_timeout(Duration::from_secs(8))
            // .max_lifetime(Duration::from_secs(8))
        //    .sqlx_logging(false)
        //    .sqlx_logging_level(log::LevelFilter::Info)
        ;
        let db: DatabaseConnection = Database::connect(opt).await?;
        Migrator::up(&db, None).await?;

        Ok(Self { db })
    }

    pub async fn insert_default_retainer_cities(&self) -> Result<()> {
        struct RetainerCityData {
            id: i32,
            name: &'static str,
        }
        let cities = [
            RetainerCityData {
                id: 1,
                name: "Limsa Lominsa",
            },
            RetainerCityData {
                id: 2,
                name: "Gridania",
            },
            RetainerCityData {
                id: 3,
                name: "Ul'dah",
            },
            RetainerCityData {
                id: 4,
                name: "Ishguard",
            },
            RetainerCityData {
                id: 7,
                name: "Kugane",
            },
            RetainerCityData {
                id: 10,
                name: "Crystarium",
            },
            RetainerCityData {
                id: 12,
                name: "Old Sharlyan",
            },
        ];
        // check if the database matches our coded data
        let db_cities = retainer_city::Entity::find().all(&self.db).await?;

        let cities_not_in_db: Vec<_> = cities
            .iter()
            .filter(|a| !db_cities.iter().any(|c| a.id.eq(&c.id)))
            .map(|m| retainer_city::ActiveModel {
                id: Set(m.id),
                name: Set(m.name.to_string()),
            })
            .collect();
        if !cities_not_in_db.is_empty() {
            let insert = retainer_city::Entity::insert_many(cities_not_in_db)
                .exec(&self.db)
                .await?;
            info!(
                "Added retainer home cities. Last insert id: {}",
                insert.last_insert_id
            );
        }
        Ok(())
    }

    pub async fn search_retainers(
        &self,
        retainer_name: &str,
    ) -> Result<Vec<(retainer::Model, Option<world::Model>)>> {
        let val = retainer::Entity::find()
            .find_also_related(world::Entity)
            .filter(retainer::Column::Name.like(retainer_name))
            .limit(10)
            .all(&self.db)
            .await?;
        Ok(val)
    }

    pub async fn get_retainer_listings(
        &self,
        retainer_id: i32,
    ) -> Result<Option<(retainer::Model, Vec<active_listing::Model>)>> {
        use retainer::*;
        let query = Entity::find()
            .filter(Column::Id.eq(retainer_id))
            .find_with_related(active_listing::Entity)
            .all(&self.db)
            .await?;
        Ok(query.into_iter().next())
    }

    /// Looks up a world via it's world name. Requires exact match
    pub async fn get_world(&self, world_name: &str) -> Result<world::Model> {
        use world::*;
        let worlds = Entity::find()
            .filter(Column::Name.eq(world_name))
            .one(&self.db)
            .await?
            .ok_or(anyhow::Error::msg("World not found"))?;
        Ok(worlds)
    }

    pub async fn get_listings_for_world(
        &self,
        world: WorldId,
        item: ItemId,
    ) -> Result<Vec<active_listing::Model>> {
        use active_listing::*;
        Ok(Entity::find()
            .filter(Column::ItemId.eq(item.0))
            .filter(Column::WorldId.eq(world.0))
            .all(&self.db)
            .await?)
    }

    pub async fn add_owned_character(
        &self,
        character_id: i32,
        first_name: &str,
        last_name: &str,
        world_id: WorldId,
    ) -> Result<final_fantasy_character::Model> {
        use final_fantasy_character::ActiveModel;
        let model = ActiveModel {
            id: Set(character_id),
            first_name: Set(first_name.to_string()),
            last_name: Set(last_name.to_string()),
            world_id: Set(world_id.0),
        }
        .insert(&self.db)
        .await?;
        Ok(model)
    }

    pub async fn create_alert(&self, owner: discord_user::Model) -> Result<alert::Model> {
        use alert::ActiveModel;
        Ok(ActiveModel {
            owner: Set(owner.id),
            ..Default::default()
        }
        .insert(&self.db)
        .await?)
    }

    pub async fn add_discord_notification_to_alert(
        &self,
        alert: &alert::Model,
        discord_channel: i64,
    ) -> Result<alert_discord_destination::Model> {
        use alert_discord_destination::ActiveModel;
        let model = ActiveModel {
            alert_id: Set(alert.id),
            channel_id: Set(discord_channel),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;
        Ok(model)
    }

    pub async fn add_retainer_alert(
        &self,
        alert: &alert::Model,
        retainer: &retainer::Model,
        margin_percent: i32,
    ) -> Result<alert_retainer_undercut::Model> {
        use alert_retainer_undercut::*;
        let model = ActiveModel {
            alert_id: Set(alert.id),
            margin_percent: Set(margin_percent),
            retainer_id: Set(retainer.id),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;
        Ok(model)
    }

    pub async fn get_world_from_retainer(
        &self,
        retainer: &retainer::Model,
    ) -> Result<Option<world::Model>> {
        let world = retainer.find_related(world::Entity).one(&self.db).await?;

        Ok(world)
    }

    pub async fn store_retainer(
        &self,
        retainer_id: &str,
        retainer_name: &str,
        world_id: WorldId,
        retainer_city_id: i32,
    ) -> Result<retainer::Model> {
        use retainer::*;
        let active_model = retainer::ActiveModel {
            id: NotSet,
            world_id: Set(world_id.0),
            name: Set(retainer_name.to_string()),
            universalis_id: Set(retainer_id.to_string()),
            retainer_city_id: Set(retainer_city_id),
        };
        let model = Entity::insert(active_model)
            .on_conflict(
                sea_query::OnConflict::column(Column::UniversalisId)
                    .update_columns([Column::Name, Column::WorldId])
                    .to_owned(),
            )
            .exec_with_returning(&self.db)
            .await?;

        Ok(model)
    }

    pub async fn create_listing(
        &self,
        listing: &ListingView,
        item_id: ItemId,
        world_id: WorldId,
        retainer_id: Option<i32>,
    ) -> Result<active_listing::Model> {
        let price_per_unit = listing.price_per_unit.unwrap_or(listing.total) as i32;
        let quantity = listing.quantity.unwrap_or(1) as i32;
        let retainer_id = if let Some(retainer_id) = retainer_id {
            retainer_id
        } else {
            let retainer = self
                .store_retainer(
                    &listing.retainer_id,
                    &listing.retainer_name,
                    world_id,
                    listing.retainer_city as i32,
                )
                .await?;
            retainer.id
        };
        let m = active_listing::ActiveModel {
            world_id: Set(world_id.0),
            item_id: Set(item_id.0),
            retainer_id: Set(retainer_id),
            price_per_unit: Set(price_per_unit),
            quantity: Set(quantity),
            hq: Set(listing.hq),
            timestamp: Set(listing.last_review_time.naive_utc()),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;
        Ok(m)
    }

    async fn get_retainer_ids_from_name(
        &self,
        name_and_ids: impl Iterator<Item = (&str, &str)>,
        world_id: i32,
    ) -> Result<Vec<retainer::Model>> {
        use retainer::*;
        if let Some(filter) = name_and_ids
            .map(|(name, id)| Column::Name.eq(name).and(Column::UniversalisId.eq(id)))
            .reduce(|a, b| a.or(b))
            .map(|m| m.and(Column::WorldId.eq(world_id)))
        {
            let retainers = Entity::find().filter(filter).all(&self.db).await?;
            Ok(retainers)
        } else {
            Ok(vec![])
        }
    }

    pub async fn remove_listings(
        &self,
        listings: Vec<ListingView>,
        item_id: ItemId,
        world_id: WorldId,
    ) -> Result<u64> {
        use active_listing::*;
        use retainer::Column as RColumn;
        use retainer::Entity as REntity;
        if listings.is_empty() {
            return Ok(0);
        }
        let retainer_names: HashSet<String> = listings
            .iter()
            .map(|l| l.retainer_name.to_string())
            .collect();
        let mut iter = retainer_names.iter().map(|name| {
            RColumn::Name
                .eq(name.as_str())
                .and(RColumn::WorldId.eq(world_id.0))
        });
        let filter = if let Some(filter) = iter.clone().reduce(|a, b| a.or(b)) {
            filter
        } else {
            iter.next().unwrap().clone()
        };
        let retainers = REntity::find().filter(filter).all(&self.db).await?;
        if retainers.is_empty() {
            return Ok(0);
        }
        let mut retainer_iter = listings.iter().flat_map(|m| {
            let retainer_id = retainers
                .iter()
                .find(|r| r.name == m.retainer_name)
                .map(|r| r.id)?;
            Some(
                Column::Hq
                    .eq(m.hq)
                    .and(Column::WorldId.eq(world_id.0))
                    .and(Column::ItemId.eq(item_id.0))
                    .and(Column::Id.eq(retainer_id)),
            )
        });

        let filter = retainer_iter.clone().reduce(|a, b| a.or(b)).unwrap_or(
            retainer_iter
                .next()
                .ok_or(anyhow::Error::msg("No retainers"))?,
        );
        let count = Entity::delete_many().filter(filter).exec(&self.db).await?;
        Ok(count.rows_affected)
    }

    /// Updates listings assuming a pure view of the listing board
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
            a.price_per_unit
                .cmp(&b.price_per_unit)
                .then_with(|| b.quantity.cmp(&a.quantity))
                .then_with(|| b.retainer_name.cmp(&a.retainer_name))
        });
        let all_retainers: HashSet<(String, String, i32)> = listings
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
                all_retainers
                    .iter()
                    .map(|(name, id, _)| (name.as_str(), id.as_str())),
                world_id.0,
            )
            .await?;
        // determine missing retainers
        for (name, id, retainer_city) in all_retainers {
            if !retainers.iter().any(|m| m.universalis_id == id) {
                let retainer = self
                    .store_retainer(&id, &name, world_id, retainer_city as i32)
                    .await?;
                retainers.push(retainer);
            }
        }
        let existing_items = Entity::find()
            .filter(
                Column::WorldId
                    .eq(world_id.0)
                    .and(Column::ItemId.eq(item_id.0)),
            )
            .join(sea_orm::JoinType::InnerJoin, Relation::Retainer.def())
            .order_by(Column::PricePerUnit, Order::Asc)
            .order_by(Column::Quantity, Order::Desc)
            .order_by(retainer::Column::Name, Order::Desc)
            .all(&self.db)
            .await?;
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
                    let retainer_id = retainers
                        .iter()
                        .find(|m| m.name == list.retainer_name)
                        .map(|m| m.id);
                    self.create_listing(&list, item_id, world_id, retainer_id)
                        .await?;
                    incoming_list = incoming_iter.next();
                    db_value = None;
                }
                (None, Some(model)) => {
                    model.delete(&self.db).await?;
                    db_value = db_iter.next();
                    incoming_list = None;
                }
                (Some(list), Some(model)) => {
                    match list
                        .price_per_unit
                        .map(|m| m as i32)
                        .unwrap_or(list.total as i32)
                        .cmp(&model.price_per_unit)
                        .then_with(|| {
                            model
                                .quantity
                                .cmp(&list.quantity.map(|q| q as i32).unwrap_or(1))
                        }) {
                        std::cmp::Ordering::Less => {
                            let retainer_id = retainers
                                .iter()
                                .find(|m| m.name == list.retainer_name)
                                .map(|m| m.id);
                            let future = async move {
                                let list = list;
                                self.create_listing(&list, item_id, world_id, retainer_id)
                                    .await
                            };
                            added.push(future);
                            incoming_list = incoming_iter.next();
                            db_value = Some(model);
                        }
                        std::cmp::Ordering::Equal => {
                            // NOOP, keep checking list
                            db_value = db_iter.next();
                            incoming_list = incoming_iter.next();
                        }
                        std::cmp::Ordering::Greater => {
                            removed.push(model);
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
        let remove_ids = removed
            .into_iter()
            .map(|i| Column::Id.eq(i.id).and(Column::Timestamp.eq(i.timestamp)))
            .reduce(|a, b| a.or(b));

        let (added, removed) =
            futures::future::join(futures::future::join_all(added), async move {
                if let Some(ids) = remove_ids {
                    Entity::delete_many()
                        .filter(ids)
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

    /// Stores a sale from a given sale view.
    /// Demands that a world name for the sale is provided as it is optional on the sale view, but can be determined other ways
    pub async fn store_sale(
        &self,
        mut sales: Vec<SaleView>,
        item_id: ItemId,
        world_id: WorldId,
    ) -> Result<i32> {
        use sale_history::*;
        use unknown_final_fantasy_character::Column as FFColumn;
        // check if the sales have already been logged
        if sales.is_empty() {
            return Ok(0);
        }
        // first upsert characters for each of the sales
        let buyer_names: HashSet<_> = sales.iter().map(|m| m.buyer_name.to_string()).collect();
        let filter_expression = buyer_names
            .iter()
            .map(|name| FFColumn::Name.eq(name.as_str()))
            .reduce(|inc, out| inc.or(out))
            .ok_or(anyhow::Error::msg("No characters inserted?"))?;
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
                let value = Column::WorldId
                    .eq(world_id.0)
                    .and(Column::SoldDate.eq(sale.timestamp))
                    .and(
                        Column::BuyingCharacterId
                            .eq(id)
                            .and(Column::SoldItemId.eq(item_id.0)),
                    )
                    .and(Column::PricePerItem.eq(sale.price_per_unit))
                    .and(Column::Quantity.eq(sale.quantity))
                    .and(Column::Hq.eq(sale.hq));
                value
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
            return Ok(0);
        }
        let values = Entity::insert_many(sales.into_iter().map(|sale| {
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
        Ok(values.last_insert_id.0)
    }

    /// Stores a region. This generally assumes the regions haven't changed and really is just querying for region IDs
    pub async fn store_region(&self, region_name: &str) -> Result<region::Model> {
        if let Some(value) = region::Entity::find()
            .filter(region::Column::Name.eq(region_name))
            .one(&self.db)
            .await?
        {
            return Ok(value);
        }
        info!("Inserting region {region_name}");
        Ok(region::ActiveModel {
            name: Set(region_name.to_string()),
            ..Default::default()
        }
        .insert(&self.db)
        .await?)
    }

    /// Stores a datacenter. Similarly to the region, this will mostly just update.
    /// It will try to update the datacenter if the region somehow changed. (unlikely)
    pub async fn store_datacenter(
        &self,
        datacenter_name: &str,
        region_name: &str,
    ) -> Result<datacenter::Model> {
        let region = region::Entity::find()
            .filter(region::Column::Name.eq(region_name))
            .one(&self.db)
            .await?
            .ok_or(anyhow::Error::msg("Region not found"))?;
        if let Some(dc) = datacenter::Entity::find()
            .filter(datacenter::Column::Name.eq(datacenter_name))
            .one(&self.db)
            .await?
        {
            // check if the region has changed
            if dc.region_id != region.id {
                // update the new region
                let mut active_dc = dc.into_active_model();
                active_dc.region_id = Set(region.id);
                return Ok(active_dc.update(&self.db).await?);
            }
            return Ok(dc);
        }

        let dc = datacenter::ActiveModel {
            name: Set(datacenter_name.to_string()),
            region_id: Set(region.id),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;
        Ok(dc)
    }

    /// Stores a world. Similar to the region/datacenter, but the final step.
    pub async fn store_world(
        &self,
        world_id: WorldId,
        world_name: &str,
        datacenter_name: &str,
    ) -> Result<world::Model> {
        let datacenter = datacenter::Entity::find()
            .filter(datacenter::Column::Name.eq(datacenter_name))
            .one(&self.db)
            .await?
            .ok_or(anyhow::Error::msg(
                "Datacenter required for world insertion",
            ))?;
        if let Some(world) = world::Entity::find()
            .filter(world::Column::Name.eq(world_name))
            .one(&self.db)
            .await?
        {
            if world.datacenter_id != datacenter.id {
                info!("updating {world_name} datacenter to {datacenter_name}");
                let mut active_world = world.into_active_model();
                active_world.datacenter_id = Set(datacenter.id);
                return Ok(active_world.update(&self.db).await?);
            }
            return Ok(world);
        }

        info!("Inserting world {world_name}");
        Ok(world::ActiveModel {
            id: Set(world_id.0),
            name: Set(world_name.to_string()),
            datacenter_id: Set(datacenter.id),
        }
        .insert(&self.db)
        .await?)
    }
}
