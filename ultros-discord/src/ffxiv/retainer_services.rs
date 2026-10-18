//! External effects used by the retainer journey. Tests provide an in-memory
//! implementation; only this adapter knows about Poise, Postgres and Lodestone.
use anyhow::anyhow;
use ultros_db::{
    entity::{
        active_listing, final_fantasy_character::Model as Character,
        owned_retainers::Model as Owned, retainer::Model as Retainer,
    },
    world_data::world_cache::AnySelector,
};

use super::{Context, Error};
use crate::EventType;

#[async_trait::async_trait]
pub(super) trait RetainerServices: Sync {
    fn world_name(&self, id: i32) -> String;
    fn world_id(&self, name: Option<&str>) -> Result<Option<i32>, Error>;
    fn locale(&self) -> Option<&str>;
    fn retainer_added(&self, owned: Owned);
    async fn characters(&self) -> Result<Vec<Character>, Error>;
    async fn search(&self, name: &str, world: Option<i32>) -> Result<Vec<Retainer>, Error>;
    async fn retainer(&self, id: i32) -> Result<Option<Retainer>, Error>;
    async fn save(&self, id: i32) -> Result<(Owned, bool), Error>;
    async fn assign(&self, owned: i32, character: Option<i32>) -> Result<(), Error>;
    async fn retainers(&self) -> Result<Vec<(Owned, Retainer)>, Error>;
    async fn listings(&self, id: i32) -> Result<Vec<active_listing::Model>, Error>;
    async fn remove(&self, owned: i32) -> Result<(), Error>;
    async fn search_characters(
        &self,
        name: &str,
        world: Option<&str>,
    ) -> Result<Vec<lodestone::search::ProfileSearchResult>, Error>;
    async fn claim_character(&self, id: u32) -> Result<Character, Error>;
}

pub(super) struct LiveServices<'a>(pub Context<'a>);

fn storage_error(error: anyhow::Error) -> Error {
    tracing::warn!(?error, "Discord retainer storage operation failed");
    anyhow!("Retainer management is temporarily unavailable. Try again shortly.").into()
}

#[async_trait::async_trait]
impl RetainerServices for LiveServices<'_> {
    fn world_name(&self, id: i32) -> String {
        self.0
            .data()
            .world_cache
            .lookup_selector(&AnySelector::World(id))
            .map(|world| world.get_name().to_owned())
            .unwrap_or_else(|_| format!("World {id}"))
    }

    fn world_id(&self, name: Option<&str>) -> Result<Option<i32>, Error> {
        name.filter(|name| !name.trim().is_empty()).map(|name| {
            self.0.data().world_cache.lookup_value_by_name(name.trim())
                .and_then(|world| world.as_world().map(|world| world.id))
                .map_err(|_| anyhow!("I couldn't find the world `{name}`. Check its name or leave the world blank to search everywhere.").into())
        }).transpose()
    }

    fn locale(&self) -> Option<&str> {
        self.0.locale()
    }

    fn retainer_added(&self, owned: Owned) {
        let _ = self
            .0
            .data()
            .event_senders
            .retainers
            .send(EventType::added(owned.into()));
    }

    async fn characters(&self) -> Result<Vec<Character>, Error> {
        Ok(self
            .0
            .data()
            .db
            .get_all_characters_for_discord_user(self.0.author().id.get() as i64)
            .await
            .map_err(storage_error)?
            .into_iter()
            .filter_map(|(_, c)| c)
            .collect())
    }

    async fn search(&self, name: &str, world: Option<i32>) -> Result<Vec<Retainer>, Error> {
        self.0.data().db.search_retainers_for_discord(name, world).await.map_err(|error| {
            tracing::warn!(?error, "Discord retainer search failed");
            anyhow!("Retainer search is temporarily unavailable. Your tracked retainers are safe. Try again shortly.").into()
        })
    }

    async fn retainer(&self, id: i32) -> Result<Option<Retainer>, Error> {
        self.0
            .data()
            .db
            .get_retainer(id)
            .await
            .map_err(storage_error)
    }

    async fn save(&self, id: i32) -> Result<(Owned, bool), Error> {
        self.0
            .data()
            .db
            .register_retainer_with_status(
                id,
                self.0.author().id.get(),
                self.0.author().name.clone(),
            )
            .await
            .map_err(storage_error)
    }

    async fn assign(&self, owned: i32, character: Option<i32>) -> Result<(), Error> {
        self.0
            .data()
            .db
            .assign_retainer_character(self.0.author().id.get() as i64, owned, character)
            .await
            .map_err(storage_error)
    }

    async fn retainers(&self) -> Result<Vec<(Owned, Retainer)>, Error> {
        Ok(self
            .0
            .data()
            .db
            .get_all_owned_retainers_and_character(self.0.author().id.get())
            .await
            .map_err(storage_error)?
            .into_iter()
            .flat_map(|(_, retainers)| retainers)
            .collect())
    }

    async fn listings(&self, id: i32) -> Result<Vec<active_listing::Model>, Error> {
        Ok(self
            .0
            .data()
            .db
            .get_retainer_listings(id)
            .await
            .map_err(storage_error)?
            .1)
    }

    async fn remove(&self, owned: i32) -> Result<(), Error> {
        let removed = self
            .0
            .data()
            .db
            .remove_owned_retainer(self.0.author().id.get(), owned)
            .await
            .map_err(storage_error)?;
        let _ = self
            .0
            .data()
            .event_senders
            .retainers
            .send(EventType::removed(removed));
        Ok(())
    }

    async fn search_characters(
        &self,
        name: &str,
        world: Option<&str>,
    ) -> Result<Vec<lodestone::search::ProfileSearchResult>, Error> {
        super::character::search(self.0, name, world).await
    }

    async fn claim_character(&self, id: u32) -> Result<Character, Error> {
        super::character::claim(self.0, id).await
    }
}
