use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;

use migration::sea_orm::IntoActiveModel;
use sea_orm::ActiveModelTrait;
use sea_orm::ActiveValue;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::LoaderTrait;
use sea_orm::QueryFilter;
use sea_orm::Set;
use sea_orm::{QueryOrder, QuerySelect, sea_query::OnConflict};
use thiserror::Error;
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

#[derive(Debug, Error)]
pub enum RetainerError {
    #[error("Retainer ownership record not found")]
    NotFound,
    #[error("{0}")]
    Forbidden(&'static str),
}

impl UltrosDb {
    /// Search market data independently of character claims. Characters only
    /// organize a user's list; they do not confer authority over a retainer.
    /// Fetch one extra row to let Discord distinguish a complete result set
    /// from one that needs a narrower search.
    #[instrument(skip(self))]
    pub async fn search_retainers_for_discord(
        &self,
        name: &str,
        world_id: Option<i32>,
    ) -> Result<Vec<retainer::Model>> {
        if name.trim().is_empty() {
            return Ok(vec![]);
        }
        let mut query = retainer::Entity::find().filter(retainer::Column::Name.like(format!(
            "{}%",
            crate::group_roles::escape_like(&retainer_search_name(name))
        )));
        if let Some(world_id) = world_id {
            query = query.filter(retainer::Column::WorldId.eq(world_id));
        }
        Ok(query
            .order_by_asc(retainer::Column::Name)
            .order_by_asc(retainer::Column::WorldId)
            .order_by_asc(retainer::Column::Id)
            .limit(26)
            .all(&self.db)
            .await?)
    }

    pub async fn get_retainer(&self, id: i32) -> Result<Option<retainer::Model>> {
        Ok(retainer::Entity::find_by_id(id).one(&self.db).await?)
    }

    #[instrument]
    pub async fn register_retainer(
        &self,
        retainer_id: i32,
        discord_user_id: u64,
        username: String,
    ) -> Result<owned_retainers::Model> {
        Ok(self
            .register_retainer_with_status(retainer_id, discord_user_id, username)
            .await?
            .0)
    }

    /// Idempotent, including concurrent requests. A duplicate must preserve
    /// character assignment and ordering, rather than updating either.
    pub async fn register_retainer_with_status(
        &self,
        retainer_id: i32,
        discord_user_id: u64,
        username: String,
    ) -> Result<(owned_retainers::Model, bool)> {
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
        let inserted = owned_retainers::Entity::insert(owned_retainers::ActiveModel {
            id: ActiveValue::default(),
            retainer_id: Set(retainer.id),
            character_id: ActiveValue::default(),
            discord_id: Set(discord_user_id as i64),
            weight: ActiveValue::Set(weight),
        })
        .on_conflict(
            OnConflict::columns([
                owned_retainers::Column::DiscordId,
                owned_retainers::Column::RetainerId,
            ])
            .do_nothing()
            .to_owned(),
        )
        // Count inserted rows: SeaORM's single-row RETURNING path reports an
        // empty conflict result as RecordNotFound rather than Conflicted.
        .exec_without_returning(&self.db)
        .await?;
        let owned = owned_retainers::Entity::find()
            .filter(owned_retainers::Column::DiscordId.eq(discord_user_id as i64))
            .filter(owned_retainers::Column::RetainerId.eq(retainer_id))
            .one(&self.db)
            .await?
            .ok_or(RetainerError::NotFound)?;
        Ok((owned, inserted == 1))
    }

    /// Shared assignment permission checks for the website and Discord bot.
    pub async fn assign_retainer_character(
        &self,
        owner_id: i64,
        owned_retainer_id: i32,
        character_id: Option<i32>,
    ) -> Result<()> {
        if let Some(character_id) = character_id
            && !self.user_owns_character(owner_id, character_id).await?
        {
            return Err(
                RetainerError::Forbidden("Add this character to your account first").into(),
            );
        }
        self.update_owned_retainer(owner_id, owned_retainer_id, |mut model| {
            model.character_id = Set(character_id);
            model
        })
        .await
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

    /// Retainer ids this Discord user has claimed. Unlike
    /// [`Self::get_owned_retainers`] this never creates a user row.
    pub async fn get_owned_retainer_ids(&self, discord_user: i64) -> Result<Vec<i32>> {
        Ok(owned_retainers::Entity::find()
            .filter(owned_retainers::Column::DiscordId.eq(discord_user))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|o| o.retainer_id)
            .collect())
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
        // execute one query per world for all items in that world to avoid N+1 queries
        let results_by_world = futures::future::join_all(items_by_world.into_iter().map(
            |(world, item_ids)| async move {
                let items = item_ids.iter().copied().map(ItemId);
                let listings = self
                    .get_listings_for_world_items(WorldId(world), items)
                    .await;
                (world, item_ids, listings)
            },
        ))
        .await;

        let mut results: HashMap<(i32, i32), Result<Vec<active_listing::Model>, _>> =
            HashMap::new();
        for (world, item_ids, listings_res) in results_by_world {
            match listings_res {
                Ok(listings) => {
                    // pre-populate with empty vectors so items with 0 listings are handled correctly
                    for item_id in item_ids {
                        results.insert((world, item_id), Ok(Vec::new()));
                    }
                    for listing in listings {
                        if let Some(Ok(vec)) = results.get_mut(&(world, listing.item_id)) {
                            vec.push(listing);
                        }
                    }
                }
                Err(e) => {
                    for item_id in item_ids {
                        results.insert((world, item_id), Err(anyhow::anyhow!(e.to_string())));
                    }
                }
            }
        }
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
            .ok_or(RetainerError::NotFound)?;
        if model.discord_id != owner_id {
            return Err(RetainerError::Forbidden("Unauthorized to edit this retainer").into());
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
            .find_also_related(retainer::Entity)
            .filter(owned_retainers::Column::DiscordId.eq(discord_user as i64))
            .all(&self.db)
            .await?;
        owned_retainers.sort_by_key(|(o, _)| o.weight);

        let mut valid_owned = Vec::with_capacity(owned_retainers.len());
        let mut retainers = Vec::with_capacity(owned_retainers.len());

        for (owned, retainer) in owned_retainers {
            let r = retainer.ok_or_else(|| anyhow::Error::msg("Retainer not found"))?;
            valid_owned.push(owned);
            retainers.push(r);
        }

        let listings: Vec<Vec<active_listing::Model>> = retainers
            .load_many(active_listing::Entity, &self.db)
            .await?;

        let retainers = valid_owned
            .into_iter()
            .zip(retainers)
            .zip(listings)
            .map(|((o, r), l)| (o, r, l))
            .collect::<Vec<_>>();

        let mut retainers = retainers;
        retainers.sort_by(|(a, _, _), (b, _, _)| {
            a.character_id
                .cmp(&b.character_id)
                .then_with(|| a.weight.cmp(&b.weight))
        });
        Ok(retainers)
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

fn retainer_search_name(name: &str) -> String {
    name.trim()
        .chars()
        .enumerate()
        .map(|(index, ch)| {
            if index == 0 {
                ch.to_ascii_uppercase()
            } else {
                ch.to_ascii_lowercase()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use sea_orm::{EntityTrait, Set};

    use super::*;

    async fn isolated_db() -> UltrosDb {
        use sea_orm::{ConnectionTrait, Database};
        let connection = Database::connect("sqlite::memory:").await.unwrap();
        for sql in [
            "CREATE TABLE discord_user (id BIGINT PRIMARY KEY, username TEXT NOT NULL)",
            "CREATE TABLE final_fantasy_character (id INTEGER PRIMARY KEY, first_name TEXT NOT NULL, last_name TEXT NOT NULL, world_id INTEGER NOT NULL)",
            "CREATE TABLE owned_ffxiv_character (ffxiv_character_id INTEGER NOT NULL, discord_user_id BIGINT NOT NULL, PRIMARY KEY (ffxiv_character_id, discord_user_id))",
            "CREATE TABLE retainer (id INTEGER PRIMARY KEY, world_id INTEGER NOT NULL, name TEXT NOT NULL, retainer_city_id INTEGER NOT NULL)",
            "CREATE TABLE owned_retainers (id INTEGER PRIMARY KEY AUTOINCREMENT, retainer_id INTEGER NOT NULL, discord_id BIGINT NOT NULL, character_id INTEGER, weight INTEGER, UNIQUE (discord_id, retainer_id))",
        ] {
            connection.execute_unprepared(sql).await.unwrap();
        }
        UltrosDb::from_connection(connection)
    }

    async fn retainer(db: &UltrosDb, id: i32, name: &str, world: i32) {
        retainer::ActiveModel {
            id: Set(id),
            name: Set(name.into()),
            world_id: Set(world),
            retainer_city_id: Set(1),
        }
        .insert(db.get_connection())
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn discord_search_and_claim_work_without_characters_across_worlds() {
        let db = isolated_db().await;
        retainer(&db, 1, "Dawnkeeper", 10).await;
        retainer(&db, 2, "Dawnkeeper", 20).await;
        retainer(&db, 3, "Starpetal", 30).await;
        assert_eq!(
            db.search_retainers_for_discord(" dAwN ", None)
                .await
                .unwrap()
                .len(),
            2
        );
        let narrowed = db
            .search_retainers_for_discord("Dawn", Some(20))
            .await
            .unwrap();
        assert_eq!(narrowed.iter().map(|r| r.id).collect::<Vec<_>>(), [2]);
        // No character claims exist. All three worlds are still trackable.
        for id in 1..=3 {
            let (_, created) = db
                .register_retainer_with_status(id, 100, "player".into())
                .await
                .unwrap();
            assert!(created);
        }
        assert_eq!(db.get_owned_retainer_ids(100).await.unwrap().len(), 3);
        assert!(
            db.search_retainers_for_discord(" ", None)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            db.search_retainers_for_discord("Missing", None)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn duplicate_and_concurrent_claims_preserve_character_and_order() {
        let db = isolated_db().await;
        retainer(&db, 1, "Dawnkeeper", 10).await;
        let (owned, created) = db
            .register_retainer_with_status(1, 100, "player".into())
            .await
            .unwrap();
        assert!(created);
        let character = db.insert_character(1, "First", "Alt", 10).await.unwrap();
        db.create_owned_character(100, character.id).await.unwrap();
        db.assign_retainer_character(100, owned.id, Some(character.id))
            .await
            .unwrap();
        db.update_owned_retainer(100, owned.id, |mut o| {
            o.weight = Set(Some(7));
            o
        })
        .await
        .unwrap();
        let (left, right) = tokio::join!(
            db.register_retainer_with_status(1, 100, "player".into()),
            db.register_retainer_with_status(1, 100, "player".into()),
        );
        for result in [left, right] {
            let (retry, created) = result.unwrap();
            assert!(!created);
            assert_eq!(retry.id, owned.id);
            assert_eq!(retry.character_id, Some(character.id));
            assert_eq!(retry.weight, Some(7));
        }
        assert_eq!(db.get_owned_retainer_ids(100).await.unwrap(), [1]);
        retainer(&db, 2, "Starpetal", 20).await;
        let (left, right) = tokio::join!(
            db.register_retainer_with_status(2, 100, "player".into()),
            db.register_retainer_with_status(2, 100, "player".into()),
        );
        let left = left.unwrap();
        let right = right.unwrap();
        assert_eq!(left.0.id, right.0.id);
        assert_ne!(
            left.1, right.1,
            "exactly one concurrent request creates the record"
        );
        // Claims are private bookkeeping, so another user may track the same retainer.
        assert!(
            db.register_retainer_with_status(1, 200, "other".into())
                .await
                .unwrap()
                .1
        );
    }

    #[tokio::test]
    async fn assignment_supports_three_characters_and_checks_both_owners() {
        let db = isolated_db().await;
        for (id, world) in [(1, 10), (2, 10), (3, 20)] {
            retainer(&db, id, &format!("Retainer{id}"), world).await;
            let owned = db
                .register_retainer(id, 100, "player".into())
                .await
                .unwrap();
            db.insert_character(id, "Player", &format!("Alt{id}"), world)
                .await
                .unwrap();
            db.create_owned_character(100, id).await.unwrap();
            db.assign_retainer_character(100, owned.id, Some(id))
                .await
                .unwrap();
        }
        let groups = db.get_all_owned_retainers_and_character(100).await.unwrap();
        assert_eq!(groups.len(), 3);
        for (character, retainers) in groups {
            let character = character.unwrap();
            assert_eq!(retainers.len(), 1);
            assert_eq!(retainers[0].0.character_id, Some(character.id));
            assert_eq!(retainers[0].1.world_id, character.world_id);
        }
        let foreign = db.register_retainer(1, 200, "other".into()).await.unwrap();
        db.insert_character(4, "Other", "Player", 10).await.unwrap();
        db.create_owned_character(200, 4).await.unwrap();
        assert!(
            db.assign_retainer_character(100, foreign.id, Some(1))
                .await
                .is_err()
        );
        let owned = db.register_retainer(1, 100, "player".into()).await.unwrap();
        assert!(
            db.assign_retainer_character(100, owned.id, Some(4))
                .await
                .is_err()
        );
        db.assign_retainer_character(100, owned.id, None)
            .await
            .unwrap();
        assert_eq!(
            db.register_retainer(1, 100, "player".into())
                .await
                .unwrap()
                .character_id,
            None
        );
    }

    #[tokio::test]
    async fn discord_search_is_bounded_and_stable() {
        let db = isolated_db().await;
        for id in (1..=40).rev() {
            retainer(&db, id, &format!("Retainer{id:02}"), 10).await;
        }
        let found = db
            .search_retainers_for_discord("Retainer", None)
            .await
            .unwrap();
        assert_eq!(found.len(), 26, "25 choices plus one overflow marker");
        assert_eq!(found.first().unwrap().name, "Retainer01");
        assert_eq!(found.last().unwrap().name, "Retainer26");
    }

    async fn test_db() -> UltrosDb {
        UltrosDb::connect().await.expect("connect to test DB")
    }

    fn unique_seed() -> i32 {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_millis();
        (millis % 1_000_000) as i32
    }

    async fn create_owned_retainer_fixture(
        db: &UltrosDb,
        seed: i32,
        owner: u64,
    ) -> (owned_retainers::Model, final_fantasy_character::Model) {
        db.insert_default_retainer_cities()
            .await
            .expect("insert retainer cities");
        let region = db.store_region(&format!("TestRegion{seed}")).await.unwrap();
        let datacenter = db
            .store_datacenter(&format!("TestDatacenter{seed}"), &region.name)
            .await
            .unwrap();
        let world_id = seed + 10_000_000;
        db.store_world(
            WorldId(world_id),
            &format!("TestWorld{seed}"),
            &datacenter.name,
        )
        .await
        .unwrap();
        let character = db
            .insert_character(seed + 20_000_000, "Test", &format!("Owner{seed}"), world_id)
            .await
            .unwrap();
        db.get_or_create_discord_user(owner, format!("owner-{seed}"))
            .await
            .unwrap();
        db.create_owned_character(owner as i64, character.id)
            .await
            .unwrap();
        let retainer = db
            .store_retainer(
                &format!("retainer-{seed}"),
                &format!("Retainer{seed}"),
                WorldId(world_id),
                1,
            )
            .await
            .unwrap();
        let owned = db
            .register_retainer(retainer.id, owner, format!("owner-{seed}"))
            .await
            .unwrap();
        (owned, character)
    }

    #[tokio::test]
    #[ignore = "requires live DB; no test_helpers scaffolding in this crate yet"]
    async fn update_owned_retainer_sets_and_clears_character_assignment() {
        let db = test_db().await;
        let owner: u64 = 9_000_000_000_000_001;
        let seed = unique_seed();
        let (owned, character) = create_owned_retainer_fixture(&db, seed, owner).await;

        db.update_owned_retainer(owner as i64, owned.id, |mut owned_retainer| {
            owned_retainer.character_id = Set(Some(character.id));
            owned_retainer
        })
        .await
        .unwrap();
        let updated = owned_retainers::Entity::find_by_id(owned.id)
            .one(db.get_connection())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.character_id, Some(character.id));

        db.update_owned_retainer(owner as i64, owned.id, |mut owned_retainer| {
            owned_retainer.character_id = Set(None);
            owned_retainer
        })
        .await
        .unwrap();
        let updated = owned_retainers::Entity::find_by_id(owned.id)
            .one(db.get_connection())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.character_id, None);
    }

    #[tokio::test]
    #[ignore = "requires live DB; no test_helpers scaffolding in this crate yet"]
    async fn update_owned_retainer_rejects_another_users_retainer() {
        let db = test_db().await;
        let owner: u64 = 9_000_000_000_000_002;
        let other_user: u64 = 9_000_000_000_000_003;
        let seed = unique_seed();
        let (owned, character) = create_owned_retainer_fixture(&db, seed, owner).await;

        let err = db
            .update_owned_retainer(other_user as i64, owned.id, |mut owned_retainer| {
                owned_retainer.character_id = Set(Some(character.id));
                owned_retainer
            })
            .await
            .unwrap_err();
        assert!(err.downcast_ref::<RetainerError>().is_some());
    }

    #[tokio::test]
    #[ignore = "requires live DB; no test_helpers scaffolding in this crate yet"]
    async fn user_owns_character_scopes_to_discord_user() {
        let db = test_db().await;
        let owner: u64 = 9_000_000_000_000_004;
        let other_user: u64 = 9_000_000_000_000_005;
        let seed = unique_seed();
        let (_owned, character) = create_owned_retainer_fixture(&db, seed, owner).await;

        assert!(
            db.user_owns_character(owner as i64, character.id)
                .await
                .unwrap()
        );
        assert!(
            !db.user_owns_character(other_user as i64, character.id)
                .await
                .unwrap()
        );
    }

    /// A claim is per-user bookkeeping, not proof of identity, so a character
    /// already claimed by someone else must still be claimable. Before the
    /// primary key became `(ffxiv_character_id, discord_user_id)` the second
    /// claim collided with the first and the earliest claimer squatted the
    /// character permanently.
    #[tokio::test]
    #[ignore = "requires live DB; no test_helpers scaffolding in this crate yet"]
    async fn two_users_can_claim_the_same_character() {
        let db = test_db().await;
        let owner: u64 = 9_000_000_000_000_006;
        let other_user: u64 = 9_000_000_000_000_007;
        let seed = unique_seed();
        let (_owned, character) = create_owned_retainer_fixture(&db, seed, owner).await;

        db.get_or_create_discord_user(other_user, format!("other-{seed}"))
            .await
            .unwrap();
        db.create_owned_character(other_user as i64, character.id)
            .await
            .unwrap();

        assert!(
            db.user_owns_character(owner as i64, character.id)
                .await
                .unwrap(),
            "the first claim must survive a second user claiming the same character"
        );
        assert!(
            db.user_owns_character(other_user as i64, character.id)
                .await
                .unwrap()
        );

        // Claiming again is a no-op rather than an error.
        db.create_owned_character(other_user as i64, character.id)
            .await
            .unwrap();
        assert_eq!(
            db.get_all_characters_for_discord_user(other_user as i64)
                .await
                .unwrap()
                .len(),
            1
        );

        // Unclaiming is scoped to the user: the other claimant keeps theirs.
        db.delete_owned_character(other_user as i64, character.id)
            .await
            .unwrap();
        assert!(
            db.user_owns_character(owner as i64, character.id)
                .await
                .unwrap()
        );
    }
}
