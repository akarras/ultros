use chrono::{NaiveDateTime, Utc};
use sea_orm::{
    ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
    sea_query::OnConflict,
};
use universalis::{ItemId, WorldId};

use crate::{UltrosDb, entity::listing_last_updated};

impl UltrosDb {
    pub(crate) async fn set_last_updated(
        &self,
        world_id: WorldId,
        item_id: ItemId,
    ) -> Result<(), anyhow::Error> {
        let model = listing_last_updated::ActiveModel {
            item_id: ActiveValue::Set(item_id.0),
            world_id: ActiveValue::Set(world_id.0),
            date_time: ActiveValue::Set(Utc::now().naive_utc()),
        };
        listing_last_updated::Entity::insert(model)
            .on_conflict(
                OnConflict::columns([
                    listing_last_updated::Column::ItemId,
                    listing_last_updated::Column::WorldId,
                ])
                .update_column(listing_last_updated::Column::DateTime)
                .to_owned(),
            )
            .exec(&self.db)
            .await?;
        Ok(())
    }

    /// Newest `listing_last_updated` row per world, i.e. the last time anything
    /// at all was ingested for that world.
    ///
    /// Worlds we have never ingested for are simply absent from the result.
    pub async fn get_last_ingest_per_world(
        &self,
    ) -> Result<Vec<(i32, NaiveDateTime)>, anyhow::Error> {
        Ok(listing_last_updated::Entity::find()
            .select_only()
            .column(listing_last_updated::Column::WorldId)
            .column_as(listing_last_updated::Column::DateTime.max(), "last_ingest")
            .group_by(listing_last_updated::Column::WorldId)
            .into_tuple::<(i32, NaiveDateTime)>()
            .all(&self.db)
            .await?)
    }

    /// Returns the ingest markers for one item across the given worlds — i.e.
    /// when Ultros last stored market data for the item on each world. Worlds
    /// that have never been ingested simply have no row.
    pub async fn get_listing_last_updated_for_worlds(
        &self,
        item_id: ItemId,
        world_ids: &[i32],
    ) -> Result<Vec<listing_last_updated::Model>, anyhow::Error> {
        if world_ids.is_empty() {
            return Ok(vec![]);
        }
        Ok(listing_last_updated::Entity::find()
            .filter(listing_last_updated::Column::ItemId.eq(item_id.0))
            .filter(listing_last_updated::Column::WorldId.is_in(world_ids.iter().copied()))
            .all(&self.db)
            .await?)
    }

    pub async fn get_recently_updated_listings_for_world(
        &self,
        world_id: i32,
        number_of_listings: u64,
    ) -> Result<Vec<listing_last_updated::Model>, anyhow::Error> {
        Ok(listing_last_updated::Entity::find()
            .filter(listing_last_updated::Column::WorldId.eq(world_id))
            .limit(number_of_listings)
            .order_by_desc(listing_last_updated::Column::DateTime)
            .all(&self.db)
            .await?)
    }
}
