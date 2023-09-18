use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use universalis::{ItemId, WorldId};

use crate::{entity::listing_last_updated, UltrosDb};

impl UltrosDb {
    pub(crate) async fn set_last_updated(
        &self,
        world_id: WorldId,
        item_id: ItemId,
    ) -> Result<(), anyhow::Error> {
        // just assume most items have an update, handle the failure case manually
        let model = listing_last_updated::ActiveModel {
            item_id: ActiveValue::Set(item_id.0),
            world_id: ActiveValue::Set(world_id.0),
            date_time: ActiveValue::Set(Utc::now().naive_utc()),
        };
        let updated = model.clone().update(&self.db).await;
        match updated {
            Ok(_updated) => {}
            Err(_e) => {
                match model.clone().insert(&self.db).await {
                    Ok(ok) => {}
                    Err(e) => {
                        // ok now can we update?
                        model.clone().update(&self.db).await?;
                    }
                }
            }
        }
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
