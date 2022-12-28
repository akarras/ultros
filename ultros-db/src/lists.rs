use crate::{
    entity::{active_listing, discord_user, list, list_item},
    UltrosDb,
};
use anyhow::anyhow;
use anyhow::Result;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, ModelTrait, QueryFilter};
use tracing::instrument;

impl UltrosDb {
    /// Creates a list for the given Discord user with the given name
    #[instrument(skip(self))]
    pub async fn create_list(
        &self,
        discord_user: discord_user::Model,
        name: &str,
    ) -> Result<list::Model> {
        let list = list::ActiveModel {
            id: Default::default(),
            owner: ActiveValue::Set(discord_user.id),
            name: ActiveValue::Set(name.to_string()),
        }
        .insert(&self.db)
        .await?;
        Ok(list)
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

    /// Adds an item to the list.
    #[instrument(skip(self))]
    pub async fn add_item_to_list(
        &self,
        list: &list::Model,
        discord_user: i64,
        item_id: i32,
        hq: Option<bool>,
    ) -> Result<list_item::Model> {
        if list.owner != discord_user {
            return Err(anyhow::anyhow!("Failed to add item to list"));
        }
        Ok(list_item::ActiveModel {
            id: Default::default(),
            item_id: ActiveValue::Set(item_id),
            list_id: ActiveValue::Set(list.id),
            hq: ActiveValue::Set(hq),
        }
        .insert(&self.db)
        .await?)
    }

    #[instrument(skip(self))]
    pub async fn remove_item_from_list(&self, discord_user: i64, item_id: i32) -> Result<()> {
        let list_item = list_item::Entity::find_by_id(item_id)
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
        list_item.delete(&self.db).await?;
        Ok(())
    }

    pub async fn get_listings_for_list(
        &self,
        discord_user: i64,
        list_id: i32,
    ) -> Result<Vec<Vec<active_listing::Model>>> {
        let list = list::Entity::find_by_id(list_id)
            .one(&self.db)
            .await?
            .ok_or(anyhow!("List not found"))?;

        if list.owner != discord_user {
            return Err(anyhow!("List not owned by user"));
        }
        let list_items = list_item::Entity::find()
            .filter(list_item::Column::ListId.eq(list_id))
            .all(&self.db)
            .await?;

        unimplemented!("")
    }
}
