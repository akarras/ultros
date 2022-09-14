use migration::Value;
use sea_orm::ActiveModelTrait;
use sea_orm::ActiveValue;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use sea_orm::QuerySelect;
use sea_orm::Set;

use crate::entity::*;
use crate::UltrosDb;
use anyhow::Result;

impl UltrosDb {
    pub async fn register_retainer(
        &self,
        retainer_id: i32,
        discord_user_id: u64,
        username: String,
    ) -> Result<owned_retainers::Model> {
        let user = self.get_or_create_discord_user(discord_user_id, username)
            .await?;
        // validate that the discord user & retainer exist in the database
        let retainer = retainer::Entity::find_by_id(retainer_id)
            .one(&self.db)
            .await?
            .ok_or(anyhow::Error::msg("Retainer not found"))?;
        Ok(owned_retainers::ActiveModel {
            id: ActiveValue::default(),
            retainer_id: Set(retainer.id),
            character_id: ActiveValue::default(),
            discord_id: Set(discord_user_id as i64),
        }
        .insert(&self.db)
        .await?)
    }

    pub async fn get_retainers_for_discord_user(
        &self,
        discord_user: u64,
    ) -> Result<Vec<(retainer::Model, Vec<active_listing::Model>)>> {
        let retainers = owned_retainers::Entity::find()
            .filter(owned_retainers::Column::DiscordId.eq(discord_user as i64))
            .all(&self.db)
            .await?;
        let retainer_ids = retainers.iter().map(|r| r.id);
        let retainers = retainer::Entity::find()
            .filter(retainer::Column::Id.is_in(retainer_ids))
            .find_with_related(active_listing::Entity)
            .all(&self.db)
            .await?;

        Ok(retainers)
    }
}
