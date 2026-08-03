//! Claiming an FFXIV character for a Discord user.
//!
//! Claims are deliberately unverified. The Discord login already establishes
//! who the user is; a character claim only groups that user's own retainers
//! under a name, so there is nothing to gate. Claiming used to require pasting
//! a hashed challenge into the character's Lodestone bio, which was never
//! finished on the web side and only ever completed through the Discord bot.
//!
//! Because a claim carries no authority, several users may claim the same
//! character — the ownership table's primary key is the (character, user) pair.

use std::sync::Arc;

use lodestone::{LodestoneError, model::profile::Profile};
use thiserror::Error;
use ultros_db::{
    UltrosDb,
    entity::final_fantasy_character,
    world_data::world_cache::{self, WorldCacheError},
};

#[derive(Debug, Clone)]
pub(crate) struct CharacterClaimService {
    pub(crate) db: UltrosDb,
    pub(crate) client: reqwest::Client,
    pub(crate) world_cache: Arc<world_cache::WorldCache>,
}

#[derive(Debug, Error)]
pub enum ClaimError {
    #[error("Error reading from lodestone {0}")]
    Lodestone(#[from] LodestoneError),
    #[error("Generic DB error {0}")]
    DbError(#[from] anyhow::Error),
    #[error("World error {0}")]
    WorldCacheError(#[from] WorldCacheError),
    #[error("Lodestone returned a character name without a surname: `{0}`")]
    UnsplittableName(String),
}

impl CharacterClaimService {
    /// Claims the Lodestone character `character_id` for `discord_user_id`.
    ///
    /// The character's name and home world come from the Lodestone profile
    /// rather than the caller, so a claim can't invent a character that doesn't
    /// exist. Claiming twice is a no-op.
    pub(crate) async fn claim_character(
        &self,
        character_id: u32,
        discord_user_id: i64,
    ) -> Result<final_fantasy_character::Model, ClaimError> {
        let profile = Profile::get_async(&self.client, character_id).await?;
        let (first_name, last_name) = profile
            .name
            .split_once(' ')
            .ok_or_else(|| ClaimError::UnsplittableName(profile.name.clone()))?;
        let world_id = self
            .world_cache
            .lookup_value_by_name(&profile.server.to_string())?
            .as_world()?
            .id;

        let character = self
            .db
            .insert_character(character_id as i32, first_name, last_name, world_id)
            .await?;
        self.db
            .create_owned_character(discord_user_id, character.id)
            .await?;
        Ok(character)
    }
}
