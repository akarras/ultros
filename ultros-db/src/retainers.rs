use std::collections::BTreeMap;
use std::collections::BTreeSet;

use sea_orm::ActiveModelTrait;
use sea_orm::ActiveValue;
use sea_orm::ColumnTrait;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use sea_orm::Set;
use universalis::ItemId;
use universalis::WorldId;

use crate::entity::*;
use crate::UltrosDb;
use anyhow::Result;

#[derive(Debug)]
pub struct ListingUndercutData {
    pub number_behind: i32,
    pub price_to_beat: i32,
}

impl UltrosDb {
    pub async fn register_retainer(
        &self,
        retainer_id: i32,
        discord_user_id: u64,
        username: String,
    ) -> Result<owned_retainers::Model> {
        let user = self
            .get_or_create_discord_user(discord_user_id, username)
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

    /// Only returns the undercut items for retainers
    pub async fn get_retainer_undercut_items(
        &self,
        discord_user: u64,
    ) -> Result<
        Vec<(
            retainer::Model,
            Vec<(active_listing::Model, ListingUndercutData)>,
        )>,
    > {
        // get the user's active listings from the retainers
        let retainer_listings = self
            .get_retainer_listings_for_discord_user(discord_user)
            .await?;
        let retainer_ids: BTreeSet<_> = retainer_listings.iter().map(|(i, _)| i.id).collect();
        let items_by_world: BTreeMap<i32, Vec<i32>> = retainer_listings
            .iter()
            .flat_map(|(_, listings)| listings.iter().map(|m| (m.world_id, m.item_id)))
            .fold(BTreeMap::new(), |mut map, (world_id, item_id)| {
                map.entry(world_id).or_default().push(item_id);
                map
            });
        // execute multiple queries for world item listings at once
        let results = futures::future::join_all(
            items_by_world
                .into_iter()
                .map(|(world, item_ids)| {
                    item_ids.into_iter().map(move |i| async move {
                        (
                            world,
                            i,
                            self.get_listings_for_world(WorldId(world), ItemId(i)).await,
                        )
                    })
                })
                .flatten(),
        )
        .await;
        // now filter the retainer queries down to just the ones that don't have our
        Ok(retainer_listings
            .into_iter()
            .map(|(retainer, listings)| {
                (
                    retainer,
                    listings
                        .into_iter()
                        .flat_map(|listing| {
                            let l = &listing;
                            // find the item in the main listings set that matches this item
                            let number_of_listings_undercutting =
                                results.iter().find_map(|(world_id, item_id, listings)| {
                                    if let Ok(listings) = listings {
                                        if listings.is_empty() {
                                            return None;
                                        }
                                        if l.world_id == *world_id && l.item_id == *item_id {
                                            // now check if the given listing is UNDERCUTTING than our given listing
                                            let listings_in_range: Vec<_> = listings
                                                .iter()
                                                .filter(|all_l| {
                                                    all_l.price_per_unit < l.price_per_unit
                                                        && (!l.hq || l.hq == all_l.hq)
                                                        // filter our own retainer listings
                                                        && !retainer_ids
                                                            .contains(&all_l.retainer_id)
                                                })
                                                .collect();
                                            return Some(ListingUndercutData {
                                                number_behind: listings_in_range.len() as i32,
                                                price_to_beat: listings_in_range
                                                    .iter()
                                                    .map(|x| x.price_per_unit)
                                                    .min()
                                                    .unwrap_or_default(),
                                            });
                                        }
                                    }
                                    None
                                });
                            number_of_listings_undercutting.map(|num| (listing, num))
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>())
    }

    pub async fn get_retainer_listings_for_discord_user(
        &self,
        discord_user: u64,
    ) -> Result<Vec<(retainer::Model, Vec<active_listing::Model>)>> {
        let retainers = owned_retainers::Entity::find()
            .filter(owned_retainers::Column::DiscordId.eq(discord_user as i64))
            .all(&self.db)
            .await?;
        let retainer_ids = retainers.iter().map(|r| r.retainer_id);
        let retainers = retainer::Entity::find()
            .filter(retainer::Column::Id.is_in(retainer_ids))
            .find_with_related(active_listing::Entity)
            .all(&self.db)
            .await?;
        // TODO add heuristics for retainers
        Ok(retainers)
    }
}
