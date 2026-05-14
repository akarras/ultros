use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;

use migration::sea_orm::IntoActiveModel;
use sea_orm::ActiveModelTrait;
use sea_orm::ActiveValue;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::ModelTrait;
use sea_orm::QueryFilter;
use sea_orm::Set;
use tracing::info;
use tracing::instrument;
use ultros_api_types::user::OwnedRetainer;
use universalis::ItemId;
use universalis::WorldId;

use crate::UltrosDb;
use crate::entity::*;
use anyhow::Result;

pub type FullRetainersList = Vec<(
    Option<final_fantasy_character::Model>,
    Vec<(owned_retainers::Model, retainer::Model)>,
)>;

pub type DiscordUserRetainerListings = Vec<(
    owned_retainers::Model,
    retainer::Model,
    Vec<active_listing::Model>,
)>;

pub type DiscordUserUndercutListings = Vec<(
    owned_retainers::Model,
    retainer::Model,
    Vec<(active_listing::Model, ListingUndercutData)>,
)>;

#[derive(Debug)]
pub struct ListingUndercutData {
    pub number_behind: usize,
    pub price_to_beat: i32,
}

impl UltrosDb {
    /// Returns all retainers in the DB whose `world_id` matches one of the
    /// `final_fantasy_character` rows that the Discord user owns (i.e. has a
    /// row in `owned_ffxiv_character`). This is the source of truth for
    /// "retainers the caller is allowed to claim" — a retainer can only belong
    /// to a character on the same world, so filtering by the user's verified
    /// characters' worlds is the strictest schema-level claim filter available
    /// (the `retainer` table has no direct character link; ownership is only
    /// recorded after the fact on `owned_retainers.character_id`).
    #[instrument]
    pub async fn get_retainers_for_user_characters(
        &self,
        discord_user_id: u64,
    ) -> Result<Vec<retainer::Model>> {
        let world_ids: Vec<i32> = owned_ffxiv_character::Entity::find()
            .find_also_related(final_fantasy_character::Entity)
            .filter(owned_ffxiv_character::Column::DiscordUserId.eq(discord_user_id as i64))
            .all(&self.db)
            .await?
            .into_iter()
            .filter_map(|(_, character)| character.map(|c| c.world_id))
            .collect();

        if world_ids.is_empty() {
            return Ok(vec![]);
        }

        Ok(retainer::Entity::find()
            .filter(retainer::Column::WorldId.is_in(world_ids))
            .all(&self.db)
            .await?)
    }

    #[instrument]
    pub async fn register_retainer(
        &self,
        retainer_id: i32,
        discord_user_id: u64,
        username: String,
    ) -> Result<owned_retainers::Model> {
        let _user = self
            .get_or_create_discord_user(discord_user_id, username)
            .await?;
        // validate that the discord user & retainer exist in the database
        let retainer = retainer::Entity::find_by_id(retainer_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Retainer not found"))?;
        let other_retainers = owned_retainers::Entity::find()
            .filter(owned_retainers::Column::DiscordId.eq(discord_user_id as i64))
            .all(&self.db)
            .await?;
        let weight = other_retainers
            .into_iter()
            .map(|r| r.weight)
            .max()
            .flatten()
            .map(|w| w + 1);
        Ok(owned_retainers::ActiveModel {
            id: ActiveValue::default(),
            retainer_id: Set(retainer.id),
            character_id: ActiveValue::default(),
            discord_id: Set(discord_user_id as i64),
            weight: ActiveValue::Set(weight),
        }
        .insert(&self.db)
        .await?)
    }

    #[instrument]
    pub async fn get_owned_retainers(
        &self,
        discord_user_id: u64,
        username: String,
    ) -> Result<Vec<(owned_retainers::Model, Option<retainer::Model>)>> {
        let _user = self
            .get_or_create_discord_user(discord_user_id, username)
            .await?;
        Ok(owned_retainers::Entity::find()
            .filter(owned_retainers::Column::DiscordId.eq(discord_user_id as i64))
            .find_also_related(retainer::Entity)
            .all(&self.db)
            .await?)
    }

    #[instrument]
    pub async fn remove_owned_retainer(
        &self,
        discord_owner: u64,
        owned_retainer_id: i32,
    ) -> Result<OwnedRetainer> {
        // validate that the discord user id matches the entity we're about to delete
        let owned_retainer = owned_retainers::Entity::find_by_id(owned_retainer_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Coulnd't find the given record of ownership"))?;
        if discord_owner as i64 != owned_retainer.discord_id {
            return Err(anyhow::Error::msg("You do not own this retainer record"));
        }
        let value = owned_retainers::Entity::delete_by_id(owned_retainer.id)
            .exec(&self.db)
            .await?;
        info!("Deleted retainer {value:?}");
        Ok(owned_retainer.into())
    }

    /// Only returns the undercut items for retainers
    #[instrument]
    pub async fn get_retainer_undercut_items(
        &self,
        discord_user: u64,
    ) -> Result<DiscordUserUndercutListings> {
        // get the user's active listings from the retainers
        let retainers = self
            .get_retainer_listings_for_discord_user(discord_user)
            .await?;
        let retainer_ids: BTreeSet<_> = retainers.iter().map(|(_, r, _)| r.id).collect();
        let items_by_world: BTreeMap<i32, Vec<i32>> = retainers
            .iter()
            .flat_map(|(_, _, listings)| listings.iter().map(|m| (m.world_id, m.item_id)))
            .fold(BTreeMap::new(), |mut map, (world_id, item_id)| {
                map.entry(world_id).or_default().push(item_id);
                map
            });
        // execute multiple queries for world item listings at once
        let results =
            futures::future::join_all(items_by_world.into_iter().flat_map(|(world, item_ids)| {
                item_ids.into_iter().map(move |i| async move {
                    (
                        (world, i),
                        self.get_listings_for_world(WorldId(world), ItemId(i)).await,
                    )
                })
            }))
            .await
            .into_iter()
            // Bolt optimization: collect results into a HashMap for O(1) lookup
            .collect::<std::collections::HashMap<_, _>>();
        // now filter the retainer queries down to just listings that beat our retainer's listings
        Ok(retainers
            .into_iter()
            .map(|(owned, retainer, listings)| {
                (
                    owned,
                    retainer,
                    listings
                        .into_iter()
                        .flat_map(|listing| {
                            let l = &listing;
                            // find the item in the main listings set that matches this item
                            let number_of_listings_undercutting =
                                results.get(&(l.world_id, l.item_id)).and_then(|listings| {
                                    if let Ok(listings) = listings {
                                        if listings.is_empty() {
                                            return None;
                                        }
                                        // now check if the given listing is UNDERCUTTING than our given listing
                                        let mut number_behind = 0;
                                        let mut price_to_beat = None;

                                        for all_l in listings.iter() {
                                            if all_l.price_per_unit < l.price_per_unit
                                                && (!l.hq || l.hq == all_l.hq)
                                                // filter our own retainer listings
                                                && !retainer_ids.contains(&all_l.retainer_id)
                                            {
                                                number_behind += 1;
                                                price_to_beat = Some(
                                                    price_to_beat
                                                        .map(|p| {
                                                            std::cmp::min(p, all_l.price_per_unit)
                                                        })
                                                        .unwrap_or(all_l.price_per_unit),
                                                );
                                            }
                                        }

                                        return Some(ListingUndercutData {
                                            number_behind,
                                            price_to_beat: price_to_beat.unwrap_or_default(),
                                        });
                                    }
                                    None
                                });
                            number_of_listings_undercutting.map(|num| (listing, num))
                        })
                        .filter(|(_, data)| data.number_behind > 0)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>())
    }

    #[instrument(skip(update))]
    pub async fn update_owned_retainer<T>(
        &self,
        owner_id: i64,
        owned_retainer_id: i32,
        update: T,
    ) -> Result<()>
    where
        T: Fn(owned_retainers::ActiveModel) -> owned_retainers::ActiveModel,
    {
        let model = owned_retainers::Entity::find_by_id(owned_retainer_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::Error::msg("Retainer with id not found"))?;
        if model.discord_id != owner_id {
            return Err(anyhow::Error::msg("Unauthorized to edit this character"));
        }
        let model = model.into_active_model();
        let model = update(model);
        model.update(&self.db).await?;
        Ok(())
    }

    #[instrument]
    pub async fn get_retainer_listings_for_discord_user(
        &self,
        discord_user: u64,
    ) -> Result<DiscordUserRetainerListings> {
        let mut owned_retainers = owned_retainers::Entity::find()
            .filter(owned_retainers::Column::DiscordId.eq(discord_user as i64))
            .all(&self.db)
            .await?;
        owned_retainers.sort_by_key(|o| o.weight);
        let retainers: Vec<_> =
            futures::future::join_all(owned_retainers.into_iter().map(|r| async move {
                let listings = self.get_retainer_and_listings_for_owned(&r).await;
                (r, listings)
            }))
            .await;
        let retainers: Result<DiscordUserRetainerListings> = retainers
            .into_iter()
            .map(|(o, r)| r.map(|(r, d)| (o, r, d)))
            .collect();
        let mut retainers = retainers?;
        retainers.sort_by(|(a, _, _), (b, _, _)| {
            a.character_id
                .cmp(&b.character_id)
                .then_with(|| a.weight.cmp(&b.weight))
        });
        Ok(retainers)
    }

    async fn get_retainer_and_listings_for_owned(
        &self,
        owned_retainer: &owned_retainers::Model,
    ) -> Result<(retainer::Model, Vec<active_listing::Model>)> {
        let retainer = owned_retainer
            .find_related(retainer::Entity)
            .one(&self.db)
            .await?
            .ok_or(anyhow::Error::msg("Retainer not found"))?;
        let listings = self.get_listings_for_retainer(&retainer).await?;
        Ok((retainer, listings))
    }

    async fn get_listings_for_retainer(
        &self,
        retainer: &retainer::Model,
    ) -> Result<Vec<active_listing::Model>> {
        Ok(retainer
            .find_related(active_listing::Entity)
            .all(&self.db)
            .await?)
    }

    #[instrument]
    pub async fn get_all_owned_retainers_and_character(
        &self,
        discord_user_id: u64,
    ) -> Result<FullRetainersList> {
        let owned_retainers = owned_retainers::Entity::find()
            .find_also_related(retainer::Entity)
            .filter(owned_retainers::Column::DiscordId.eq(discord_user_id))
            .all(&self.db)
            .await?;
        let character_ids: HashSet<i32> = owned_retainers
            .iter()
            .flat_map(|(owned, _)| owned.character_id)
            .collect();
        let characters: HashMap<i32, final_fantasy_character::Model> = if character_ids.is_empty() {
            HashMap::new()
        } else {
            final_fantasy_character::Entity::find()
                .filter(final_fantasy_character::Column::Id.is_in(character_ids))
                .all(&self.db)
                .await?
                .into_iter()
                .map(|c| (c.id, c))
                .collect()
        };
        // Group retainers by character_id in a HashMap (O(n)) before flattening to FullRetainersList.
        // The owned_retainers query was find_also_related(retainer::Entity), so each row is
        // (owned_retainers::Model, Option<retainer::Model>); drop the rows that lack a related retainer.
        let mut grouped: HashMap<Option<i32>, Vec<(owned_retainers::Model, retainer::Model)>> =
            HashMap::new();
        for (owned, retainer) in owned_retainers
            .into_iter()
            .flat_map(|(owned, retainer)| retainer.map(|r| (owned, r)))
        {
            grouped
                .entry(owned.character_id)
                .or_default()
                .push((owned, retainer));
        }
        let mut value: FullRetainersList = grouped
            .into_iter()
            .map(|(character_id, retainers)| {
                let character = character_id.and_then(|id| characters.get(&id).cloned());
                (character, retainers)
            })
            .collect();
        value
            .iter_mut()
            .for_each(|(_, i)| i.sort_by_key(|(o, _)| o.weight));
        value.sort_by_key(|(o, _)| o.as_ref().map(|o| o.id).unwrap_or_default());
        Ok(value)
    }
}
