mod alerts;
pub mod common_type_conversions;
mod discord;
pub mod entity;
mod ffxiv_character;
pub mod listings;
pub mod lists;
pub(crate) mod partial_diff_iterator;
pub mod price_optimizer;
mod regions_and_datacenters;
pub mod retainers;
pub mod sales;
pub mod world_cache;
mod worlds;

pub use sea_orm::error::DbErr as SeaDbErr;
pub use sea_orm::ActiveValue;

use anyhow::Result;
use chrono::{Duration, Utc};
use futures::{future::try_join_all, Stream};
use migration::{sea_orm::QueryOrder, DbErr, Migrator, MigratorTrait};

use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectOptions, Database,
    DatabaseConnection, EntityTrait, FromQueryResult, IntoActiveModel, ModelTrait, QueryFilter,
    QuerySelect, Set,
};

use tracing::{error, info, instrument};
use ultros_api_types::Retainer;
use universalis::{ItemId, ListingView, WorldId};

use crate::entity::*;

#[derive(Clone, Debug)]
pub struct UltrosDb {
    // Connections here
    db: DatabaseConnection,
}

impl UltrosDb {
    #[instrument]
    pub async fn connect() -> Result<Self> {
        let url = std::env::var("DATABASE_URL").expect("Missing DATABASE_URL environment variable");
        let mut opt = ConnectOptions::new(url);
        let max_connections = std::env::var("POSTGRES_MAX_CONNECTIONS")
            .ok()
            .and_then(|connections| {
                connections
                    .parse::<u32>()
                    .map_err(|e| {
                        error!(error = %e, "Unable to read POSTGRES_MAX_CONNECTIONS env variable");
                        e
                    })
                    .ok()
            })
            .unwrap_or(300);
        opt.max_connections(max_connections)
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

    #[instrument(skip(self))]
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

    #[instrument(skip(self))]
    pub async fn search_retainers(&self, retainer_name: &str) -> Result<Vec<Retainer>> {
        // retainer names are forced to be lower except for the first character which will be uppercase.
        let retainer_name: String = retainer_name
            .chars()
            .enumerate()
            .map(|(i, byte)| {
                if i == 0 {
                    byte.to_ascii_uppercase()
                } else {
                    byte.to_ascii_lowercase()
                }
            })
            .collect();

        let val = retainer::Entity::find()
            .filter(retainer::Column::Name.like(&format!("{retainer_name}%")))
            .limit(10)
            .all(&self.db)
            .await?;
        Ok(val.into_iter().map(|val| Retainer::from(val)).collect())
    }

    #[instrument(skip(self))]
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
    #[instrument(skip(self))]
    pub async fn get_world(&self, world_name: &str) -> Result<world::Model> {
        use world::*;
        let worlds = Entity::find()
            .filter(Column::Name.eq(world_name))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("World not found"))?;
        Ok(worlds)
    }

    #[instrument(skip(self))]
    pub async fn get_datacenter_from_world(
        &self,
        world: &world::Model,
    ) -> Result<datacenter::Model> {
        datacenter::Entity::find_by_id(world.datacenter_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Datacenter not found"))
    }

    #[instrument(skip(self))]
    pub async fn get_region_from_datacenter(
        &self,
        datacenter: &datacenter::Model,
    ) -> Result<region::Model> {
        region::Entity::find_by_id(datacenter.region_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Region not found"))
    }

    #[instrument(skip(self, world_id, item))]
    pub async fn get_multiple_listings_for_worlds(
        &self,
        world_id: impl Iterator<Item = WorldId>,
        item: impl Iterator<Item = ItemId> + Clone,
        limit: u64,
    ) -> Result<Vec<active_listing::Model>> {
        let join = futures::future::try_join_all(world_id.flat_map(|world| {
            item.clone()
                .map(move |i| self.get_listings_for_world(world, i))
        }))
        .await?;
        Ok(join.into_iter().flat_map(|l| l.into_iter()).collect())
    }

    #[instrument(skip(self))]
    pub async fn get_listings_for_world(
        &self,
        world: WorldId,
        item: ItemId,
    ) -> Result<Vec<active_listing::Model>> {
        use active_listing::*;
        let listings = Entity::find()
            .filter(Column::ItemId.eq(item.0))
            .filter(Column::WorldId.eq(world.0))
            .all(&self.db)
            .await?;
        Ok(listings)
    }

    #[instrument(skip(self))]
    pub async fn get_cheapest_listing_by_world(
        &self,
        world: i32,
        item: i32,
        is_hq: bool,
    ) -> Result<Option<active_listing::Model>> {
        use active_listing::*;
        Ok(Entity::find()
            .filter(Column::ItemId.eq(item))
            .filter(Column::WorldId.eq(world))
            .filter(Column::Hq.eq(is_hq))
            .order_by_asc(Column::PricePerUnit)
            .one(&self.db)
            .await?)
    }

    #[instrument(skip(self))]
    pub async fn create_alert(&self, owner: discord_user::Model) -> Result<alert::Model> {
        use alert::ActiveModel;
        Ok(ActiveModel {
            owner: Set(owner.id),
            ..Default::default()
        }
        .insert(&self.db)
        .await?)
    }

    #[instrument(skip(self))]
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

    #[instrument(skip(self))]
    pub async fn get_world_from_retainer(
        &self,
        retainer: &retainer::Model,
    ) -> Result<Option<world::Model>> {
        let world = retainer.find_related(world::Entity).one(&self.db).await?;

        Ok(world)
    }

    #[instrument(skip(self))]
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
            retainer_city_id: Set(retainer_city_id),
        };
        let model = Entity::insert(active_model)
            .on_conflict(
                sea_query::OnConflict::columns([Column::Name, Column::WorldId].into_iter())
                    .update_columns([Column::RetainerCityId])
                    .to_owned(),
            )
            .exec_with_returning(&self.db)
            .await?;

        Ok(model)
    }

    #[instrument(skip(self))]
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

    #[instrument(skip(self, names))]
    async fn get_retainer_ids_from_name(
        &self,
        names: impl Iterator<Item = &str>,
        world_id: i32,
    ) -> Result<Vec<retainer::Model>> {
        use retainer::*;
        let retainers = try_join_all(names.map(|name| {
            Entity::find()
                .filter(Column::Name.eq(name))
                .filter(Column::WorldId.eq(world_id))
                .one(&self.db)
        }))
        .await?
        .into_iter()
        .flatten()
        .collect();
        Ok(retainers)
    }

    #[instrument(skip(self))]
    pub async fn stream_sales_within_days(
        &self,
        days: i64,
        world_id: i32,
    ) -> Result<impl Stream<Item = Result<sale_history::Model, DbErr>> + '_, anyhow::Error> {
        Ok(sale_history::Entity::find()
            .filter(sale_history::Column::WorldId.eq(world_id))
            .filter(sale_history::Column::SoldDate.gt(Utc::now() - Duration::days(days)))
            .stream(&self.db)
            .await?)
    }

    /// Stores a region. This generally assumes the regions haven't changed and really is just querying for region IDs
    #[instrument(skip(self))]
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
    #[instrument(skip(self))]
    pub async fn store_datacenter(
        &self,
        datacenter_name: &str,
        region_name: &str,
    ) -> Result<datacenter::Model> {
        let region = region::Entity::find()
            .filter(region::Column::Name.eq(region_name))
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Region not found"))?;
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
    #[instrument(skip(self))]
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
            .ok_or_else(|| anyhow::Error::msg("Datacenter required for world insertion"))?;
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

    #[instrument(skip(self))]
    pub async fn get_unique_item_ids(&self) -> Result<Vec<i32>> {
        let items = active_listing::Entity::find()
            .select_only()
            .column(active_listing::Column::ItemId)
            .distinct()
            .into_model::<UniqueItemId>()
            .all(&self.db)
            .await?;

        Ok(items.into_iter().map(|i| i.item).collect())
    }
}
#[derive(Debug, FromQueryResult)]
pub struct UniqueItemId {
    pub item: i32,
}
