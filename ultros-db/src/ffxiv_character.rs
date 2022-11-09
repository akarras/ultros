use sea_orm::{ActiveValue, EntityTrait, Set};
use tracing::instrument;

use super::UltrosDb;
use crate::entity::*;
use anyhow::Result;
use sea_orm::ActiveModelTrait;
use sea_orm::ColumnTrait;
use sea_orm::QueryFilter;

impl UltrosDb {
    #[instrument(skip(self))]
    pub async fn insert_character(
        &self,
        lodestone_id: i32,
        first_name: &str,
        last_name: &str,
        world_id: i32,
    ) -> Result<final_fantasy_character::Model> {
        use final_fantasy_character::*;
        Ok(Entity::insert(ActiveModel {
            id: Set(lodestone_id),
            first_name: Set(first_name.to_string()),
            last_name: Set(last_name.to_string()),
            world_id: Set(world_id),
        })
        .exec_with_returning(&self.db)
        .await?)
    }

    #[instrument(skip(self))]
    pub async fn create_character_challenge<T: ToString + std::fmt::Debug>(
        &self,
        lodestone_id: i32,
        discord_user_id: i64,
        challenge: T,
    ) -> Result<ffxiv_character_verification::Model> {
        let model = ffxiv_character_verification::ActiveModel {
            id: ActiveValue::default(),
            discord_user_id: Set(discord_user_id),
            ffxiv_character_id: Set(lodestone_id),
            challenge: Set(challenge.to_string()),
        };
        let model = model.insert(&self.db).await?;
        Ok(model)
    }

    #[instrument(skip(self))]
    pub async fn get_all_characters_for_discord_user(
        &self,
        discord_user_id: i64,
    ) -> Result<
        Vec<(
            owned_ffxiv_character::Model,
            Option<final_fantasy_character::Model>,
        )>,
    > {
        Ok(owned_ffxiv_character::Entity::find()
            .find_also_related(final_fantasy_character::Entity)
            .filter(owned_ffxiv_character::Column::DiscordUserId.eq(discord_user_id))
            .all(&self.db)
            .await?)
    }

    #[instrument(skip(self))]
    pub async fn get_pending_character_challenges_for_discord_user(
        &self,
        discord_user_id: i64,
    ) -> Result<Vec<ffxiv_character_verification::Model>> {
        Ok(ffxiv_character_verification::Entity::find()
            .filter(ffxiv_character_verification::Column::DiscordUserId.eq(discord_user_id))
            .all(&self.db)
            .await?)
    }

    #[instrument(skip(self))]
    pub async fn get_character_challenge(
        &self,
        id: i32,
    ) -> Result<ffxiv_character_verification::Model> {
        Ok(ffxiv_character_verification::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or(anyhow::Error::msg("Challenge ID not found"))?)
    }

    pub async fn create_owned_character(
        &self,
        discord_user_id: i64,
        ffxiv_character_id: i32,
    ) -> Result<owned_ffxiv_character::Model> {
        let model = owned_ffxiv_character::ActiveModel {
            discord_user_id: Set(discord_user_id),
            ffxiv_character_id: Set(ffxiv_character_id),
        };
        Ok(model.insert(&self.db).await?)
    }
}
