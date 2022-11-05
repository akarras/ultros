use sea_orm::{ActiveValue, EntityTrait, Set};
use tracing::instrument;

use super::UltrosDb;
use crate::entity::*;
use anyhow::Result;

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
    ) {
        let _ = ffxiv_character_verification::ActiveModel {
            id: ActiveValue::default(),
            discord_user_id: Set(discord_user_id),
            ffxiv_character_id: Set(lodestone_id),
            challenge: Set(challenge.to_string()),
        };
    }
}
