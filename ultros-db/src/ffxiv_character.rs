use sea_orm::{ActiveValue, EntityTrait, Set};

use super::UltrosDb;
use crate::entity::*;
use anyhow::Result;

impl UltrosDb {
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
}
