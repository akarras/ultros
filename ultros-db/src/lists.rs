use crate::{
    entity::{active_listing, discord_user, list, list_item, retainer},
    world_cache::{AnySelector, WorldCache},
    UltrosDb,
};
use anyhow::anyhow;
use anyhow::Result;
use futures::future::try_join_all;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, IntoActiveModel, ModelTrait,
    QueryFilter,
};
use std::{collections::HashMap, sync::Arc};
use tracing::instrument;
use universalis::ItemId;

impl TryFrom<&list::Model> for AnySelector {
    type Error = anyhow::Error;

    fn try_from(value: &list::Model) -> Result<Self, Self::Error> {
        let list::Model {
            world_id,
            datacenter_id,
            region_id,
            ..
        } = value;
        match (world_id, datacenter_id, region_id) {
            (_, _, Some(r)) => Ok(AnySelector::Region(*r)),
            (_, Some(d), _) => Ok(AnySelector::Datacenter(*d)),
            (Some(w), _, _) => Ok(AnySelector::World(*w)),
            _ => Err(anyhow!("List has no world filter selected")),
        }
    }
}

impl UltrosDb {
    /// Creates a list for the given Discord user with the given name
    #[instrument(skip(self))]
    pub async fn create_list(
        &self,
        discord_user: discord_user::Model,
        name: String,
        selector: Option<AnySelector>,
    ) -> Result<list::Model> {
        let list = list::ActiveModel {
            id: Default::default(),
            owner: ActiveValue::Set(discord_user.id),
            name: ActiveValue::Set(name),
            world_id: match selector {
                Some(AnySelector::World(w)) => ActiveValue::Set(Some(w)),
                _ => Default::default(),
            },
            datacenter_id: match selector {
                Some(AnySelector::Datacenter(d)) => ActiveValue::Set(Some(d)),
                _ => Default::default(),
            },
            region_id: match selector {
                Some(AnySelector::Region(r)) => ActiveValue::Set(Some(r)),
                _ => Default::default(),
            },
        }
        .insert(&self.db)
        .await?;
        Ok(list)
    }

    pub async fn update_list<T>(
        &self,
        list_id: i32,
        discord_user: i64,
        update: T,
    ) -> Result<list::Model>
    where
        T: FnOnce(&mut list::ActiveModel),
    {
        let list = list::Entity::find_by_id(list_id)
            .one(&self.db)
            .await?
            .ok_or(anyhow!("Unable to find list"))?;
        if list.owner != discord_user {
            return Err(anyhow!("List not owned by Discord user"));
        }
        let mut model = list.into_active_model();
        update(&mut model);
        Ok(model.update(&self.db).await?)
    }

    /// Deletes the given list assuming that it is owned by the Discord user
    #[instrument(skip(self))]
    pub async fn delete_list(&self, list_id: i32, discord_user: i64) -> Result<()> {
        let list = list::Entity::find_by_id(list_id)
            .one(&self.db)
            .await?
            .ok_or(anyhow::anyhow!("Failed to find list with that ID"))?;
        if list.owner != discord_user {
            return Err(anyhow::anyhow!("List not owned by that user"));
        }
        list.delete(&self.db).await?;
        Ok(())
    }

    pub async fn get_lists_for_user(&self, discord_user: i64) -> Result<Vec<list::Model>> {
        Ok(list::Entity::find()
            .filter(list::Column::Owner.eq(discord_user))
            .all(&self.db)
            .await?)
    }

    pub async fn get_list(&self, list_id: i32, discord_user: i64) -> Result<list::Model> {
        let list = list::Entity::find_by_id(list_id)
            .one(&self.db)
            .await?
            .ok_or(anyhow!("List not found"))?;
        if list.owner != discord_user {
            return Err(anyhow!("List not owned by user"));
        }
        Ok(list)
    }

    pub async fn get_list_items(
        &self,
        list: i32,
        discord_user: i64,
    ) -> Result<Vec<list_item::Model>> {
        let list = list::Entity::find_by_id(list)
            .one(&self.db)
            .await?
            .ok_or(anyhow!("List not found"))?;
        if list.owner != discord_user {
            return Err(anyhow!("Discord user doesn't own that list"));
        }
        Ok(list_item::Entity::find()
            .filter(list_item::Column::ListId.eq(list.id))
            .all(&self.db)
            .await?)
    }

    /// Adds an item to the list.
    #[instrument(skip(self))]
    pub async fn add_item_to_list(
        &self,
        list: &list::Model,
        discord_user: i64,
        item_id: i32,
        hq: Option<bool>,
        quantity: Option<i32>,
    ) -> Result<list_item::Model> {
        if list.owner != discord_user {
            return Err(anyhow::anyhow!("Failed to add item to list"));
        }
        // if the item already exists in the list, just update the existing list
        let mut filter = list_item::Entity::find().filter(list_item::Column::ItemId.eq(item_id));
        if let Some(hq) = hq {
            filter = filter.filter(list_item::Column::Hq.eq(hq));
        }
        if let Some(item) = filter.one(&self.db).await? {
            let new_quantity = item.quantity.unwrap_or(1) + quantity.unwrap_or(1);
            let mut item = item.into_active_model();
            item.quantity = ActiveValue::Set(Some(new_quantity));
            Ok(item.update(&self.db).await?)
        } else {
            Ok(list_item::ActiveModel {
                id: Default::default(),
                item_id: ActiveValue::Set(item_id),
                list_id: ActiveValue::Set(list.id),
                hq: ActiveValue::Set(hq),
                quantity: ActiveValue::Set(quantity),
            }
            .insert(&self.db)
            .await?)
        }
    }

    // #[instrument(skip(self))]
    pub async fn add_items_to_list(
        &self,
        list: &list::Model,
        discord_user: i64,
        items: impl Iterator<Item = list_item::Model>,
    ) -> Result<u64> {
        if list.owner != discord_user {
            return Err(anyhow::anyhow!("Failed to add item to list"));
        }
        // for items that are already matching our list, we should update and insert
        let mut existing_list_items: HashMap<_, _> = list
            .find_related(list_item::Entity)
            .all(&self.db)
            .await?
            .into_iter()
            .map(|item| ((item.hq, item.item_id), item))
            .collect();

        let mut insert_queue = vec![];
        let mut updated_models = vec![];
        items.into_iter().for_each(|item| {
            let key = (item.hq, item.item_id);
            // removing from the map and assuming that the incoming list won't have duplicates
            if let Some(existing) = existing_list_items.remove(&key) {
                let new_quantity = existing.quantity.unwrap_or(1) + item.quantity.unwrap_or(1);
                let mut existing = existing.into_active_model();
                existing.quantity = ActiveValue::Set(Some(new_quantity));
                updated_models.push(existing);
            } else {
                insert_queue.push(item);
            }
        });
        try_join_all(
            updated_models
                .into_iter()
                .map(|updated| updated.update(&self.db)),
        )
        .await?;
        let many = list_item::Entity::insert_many(insert_queue.into_iter().map(|item| {
            let list_item::Model {
                item_id,
                list_id,
                hq,
                quantity,
                ..
            } = item;
            list_item::ActiveModel {
                id: Default::default(),
                item_id: ActiveValue::Set(item_id),
                list_id: ActiveValue::Set(list_id),
                hq: ActiveValue::Set(hq),
                quantity: ActiveValue::Set(quantity),
            }
        }))
        .exec_without_returning(&self.db)
        .await?;
        Ok(many)
    }

    #[instrument(skip(self))]
    pub async fn remove_item_from_list(
        &self,
        discord_user: i64,
        list_item_id: i32,
    ) -> Result<list_item::Model> {
        let list_item = list_item::Entity::find_by_id(list_item_id)
            .one(&self.db)
            .await?
            .ok_or(anyhow!("No list item"))?;
        let list = list::Entity::find_by_id(list_item.list_id)
            .one(&self.db)
            .await?
            .ok_or(anyhow!("List not found"))?;
        if discord_user != list.owner {
            return Err(anyhow!("User doesn't own item"));
        }
        list_item.clone().delete(&self.db).await?;
        Ok(list_item)
    }

    pub async fn get_listings_for_list(
        &self,
        discord_user: i64,
        list_id: i32,
        world_cache: &Arc<WorldCache>,
    ) -> Result<
        Vec<(
            list_item::Model,
            Vec<(active_listing::Model, Option<retainer::Model>)>,
        )>,
    > {
        let list = list::Entity::find_by_id(list_id)
            .one(&self.db)
            .await?
            .ok_or(anyhow!("List not found"))?;
        let selector = AnySelector::try_from(&list)?;
        let result = world_cache.lookup_selector(&selector)?;
        let worlds = world_cache
            .get_all_worlds_in(&result)
            .ok_or(anyhow!("Unable to get worlds for list"))?;
        if list.owner != discord_user {
            return Err(anyhow!("List not owned by user"));
        }
        let list_items = list_item::Entity::find()
            .filter(list_item::Column::ListId.eq(list_id))
            .all(&self.db)
            .await?;
        let worlds = &worlds;
        Ok(try_join_all(list_items.into_iter().map(|item| async move {
            self.get_all_listings_in_worlds_with_retainers(worlds, ItemId(item.item_id))
                .await
                .map(|listings| (item, listings))
        }))
        .await?)
    }
}
