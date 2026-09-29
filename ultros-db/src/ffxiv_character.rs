use migration::OnConflict;
use sea_orm::{ActiveValue, EntityTrait, Set};
use sea_orm::{IntoActiveModel, ModelTrait};
use tracing::instrument;

use super::UltrosDb;
use crate::entity::*;
use anyhow::{Result, anyhow};
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
        .on_conflict(
            OnConflict::column(final_fantasy_character::Column::Id)
                // The Lodestone is authoritative for all three, so a re-claim
                // picks up renames *and* world transfers. `world_id` used to be
                // left alone, which quietly pinned a transferred character to
                // its old world.
                .update_columns([
                    final_fantasy_character::Column::FirstName,
                    final_fantasy_character::Column::LastName,
                    final_fantasy_character::Column::WorldId,
                ])
                .to_owned(),
        )
        .exec_with_returning(&self.db)
        .await?)
    }

    pub async fn get_character(
        &self,
        lodestone_id: i32,
    ) -> Result<Option<final_fantasy_character::Model>> {
        Ok(final_fantasy_character::Entity::find_by_id(lodestone_id)
            .one(&self.db)
            .await?)
    }

    pub async fn update_character_name(
        &self,
        model: final_fantasy_character::Model,
        first_name: &str,
        last_name: &str,
    ) -> Result<final_fantasy_character::Model> {
        let mut model = model.into_active_model();
        model.first_name = ActiveValue::Set(first_name.to_string());
        model.last_name = ActiveValue::Set(last_name.to_string());
        Ok(model.update(&self.db).await?)
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
    pub async fn user_owns_character(
        &self,
        discord_user_id: i64,
        ffxiv_character_id: i32,
    ) -> Result<bool> {
        let owned = owned_ffxiv_character::Entity::find()
            .filter(owned_ffxiv_character::Column::DiscordUserId.eq(discord_user_id))
            .filter(owned_ffxiv_character::Column::FfxivCharacterId.eq(ffxiv_character_id))
            .one(&self.db)
            .await?;
        Ok(owned.is_some())
    }

    /// Records that `discord_user_id` has claimed `ffxiv_character_id`.
    ///
    /// Claiming is idempotent: re-claiming a character you already hold returns
    /// the existing row rather than erroring, so a double-click or a retried
    /// Discord command is harmless. Other users' claims on the same character
    /// are untouched — see the composite primary key.
    pub async fn create_owned_character(
        &self,
        discord_user_id: i64,
        ffxiv_character_id: i32,
    ) -> Result<owned_ffxiv_character::Model> {
        let model = owned_ffxiv_character::ActiveModel {
            discord_user_id: Set(discord_user_id),
            ffxiv_character_id: Set(ffxiv_character_id),
        };
        Ok(owned_ffxiv_character::Entity::insert(model)
            .on_conflict(
                OnConflict::columns([
                    owned_ffxiv_character::Column::FfxivCharacterId,
                    owned_ffxiv_character::Column::DiscordUserId,
                ])
                // A no-op update rather than `do_nothing`, so the conflicting
                // row still comes back through RETURNING.
                .update_column(owned_ffxiv_character::Column::DiscordUserId)
                .to_owned(),
            )
            .exec_with_returning(&self.db)
            .await?)
    }

    pub async fn delete_owned_character(
        &self,
        discord_user_id: i64,
        ffxiv_character_id: i32,
    ) -> Result<u64> {
        let owned = owned_ffxiv_character::Entity::find()
            .filter(owned_ffxiv_character::Column::DiscordUserId.eq(discord_user_id))
            .filter(owned_ffxiv_character::Column::FfxivCharacterId.eq(ffxiv_character_id))
            .one(&self.db)
            .await?;
        let delete = owned
            .ok_or(anyhow!("Ownership record not found"))?
            .delete(&self.db)
            .await?;
        Ok(delete.rows_affected)
    }
}
