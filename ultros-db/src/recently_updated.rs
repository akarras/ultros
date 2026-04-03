use chrono::Utc;
use sea_orm::{
    ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, sea_query,
};
use universalis::{ItemId, WorldId};

use crate::{UltrosDb, entity::listing_last_updated};

impl UltrosDb {
    pub(crate) async fn set_last_updated(
        &self,
        world_id: WorldId,
        item_id: ItemId,
    ) -> Result<(), anyhow::Error> {
        // OPTIMIZATION: Use ON CONFLICT to reduce database round-trips from up to 3 queries down to exactly 1
        let model = listing_last_updated::ActiveModel {
            item_id: ActiveValue::Set(item_id.0),
            world_id: ActiveValue::Set(world_id.0),
            date_time: ActiveValue::Set(Utc::now().naive_utc()),
        };

        listing_last_updated::Entity::insert(model)
            .on_conflict(
                sea_query::OnConflict::columns([
                    listing_last_updated::Column::ItemId,
                    listing_last_updated::Column::WorldId,
                ])
                .update_columns([listing_last_updated::Column::DateTime])
                .to_owned(),
            )
            .exec(&self.db)
            .await?;

        Ok(())
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
