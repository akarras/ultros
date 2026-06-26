use crate::{
    UltrosDb,
    common::try_update_value::ActiveValueCmpSet,
    common_type_conversions::{ListSharedGroupReturn, ListSharedUserReturn, UserGroupMemberReturn},
    entity::{
        active_listing, discord_user, list, list_activity, list_invite, list_item,
        list_shared_group, list_shared_user, retainer, user_group, user_group_member,
    },
    world_data::world_cache::{AnySelector, WorldCache},
};
use anyhow::Result;
use anyhow::anyhow;
use futures::future::try_join_all;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, Condition, EntityTrait, IntoActiveModel, JoinType,
    ModelTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait, TransactionTrait,
    sea_query::Expr,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use thiserror::Error;
use tracing::instrument;
use ultros_api_types::list::{ListActivityKind, ListPermission};
use universalis::ItemId;

#[derive(Debug, Error)]
pub enum ListError {
    #[error("List not found")]
    NotFound,
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0}")]
    BadRequest(&'static str),
    #[error("Invite not found")]
    InviteNotFound,
    #[error("Invite has reached max uses")]
    InviteExhausted,
}

fn validate_share_permission(permission: ListPermission) -> Result<()> {
    match permission {
        ListPermission::Read | ListPermission::Write => Ok(()),
        ListPermission::None => {
            Err(ListError::BadRequest("Sharing requires Read or Write permission").into())
        }
        ListPermission::Owner => {
            Err(ListError::BadRequest("Owner permission cannot be granted by sharing").into())
        }
    }
}

fn new_invite_id() -> Result<String> {
    let mut bytes = [0_u8; 24];
    getrandom::fill(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

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
    pub async fn get_permission(&self, list_id: i32, user_id: i64) -> Result<ListPermission> {
        let list = list::Entity::find_by_id(list_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::NotFound)?;

        if list.owner == user_id {
            return Ok(ListPermission::Owner);
        }

        // Check shared users
        let shared_user = list_shared_user::Entity::find()
            .filter(list_shared_user::Column::ListId.eq(list_id))
            .filter(list_shared_user::Column::UserId.eq(user_id))
            .one(&self.db)
            .await?;

        let mut max_permission = shared_user
            .map(|s| ListPermission::from(s.permission))
            .unwrap_or(ListPermission::None);

        if max_permission == ListPermission::Owner {
            return Ok(ListPermission::Owner);
        }

        // Check shared groups: single JOIN across list_shared_group and
        // user_group_member, filtered by (list_id, user_id). Replaces the
        // previous 1+N pattern of fetching every shared group then probing
        // membership in a loop.
        let group_perms: Vec<i16> = list_shared_group::Entity::find()
            .select_only()
            .column(list_shared_group::Column::Permission)
            .join(
                JoinType::InnerJoin,
                list_shared_group::Relation::UserGroup.def(),
            )
            .join(
                JoinType::InnerJoin,
                user_group::Relation::UserGroupMember.def(),
            )
            .filter(list_shared_group::Column::ListId.eq(list_id))
            .filter(user_group_member::Column::UserId.eq(user_id))
            .into_tuple()
            .all(&self.db)
            .await?;

        for perm in group_perms {
            let p = ListPermission::from(perm);
            if p > max_permission {
                max_permission = p;
            }
        }

        let owned_group_perms: Vec<i16> = list_shared_group::Entity::find()
            .select_only()
            .column(list_shared_group::Column::Permission)
            .join(
                JoinType::InnerJoin,
                list_shared_group::Relation::UserGroup.def(),
            )
            .filter(list_shared_group::Column::ListId.eq(list_id))
            .filter(user_group::Column::OwnerId.eq(user_id))
            .into_tuple()
            .all(&self.db)
            .await?;

        for perm in owned_group_perms {
            let p = ListPermission::from(perm);
            if p > max_permission {
                max_permission = p;
            }
        }

        Ok(max_permission)
    }

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
        let permission = self.get_permission(list_id, discord_user).await?;
        if permission < ListPermission::Owner {
            return Err(ListError::Forbidden("Only the owner can update list settings").into());
        }
        let list = list::Entity::find_by_id(list_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::NotFound)?;
        let mut model = list.into_active_model();
        update(&mut model);
        Ok(model.update(&self.db).await?)
    }

    /// Deletes the given list assuming that it is owned by the Discord user
    #[instrument(skip(self))]
    pub async fn delete_list(&self, list_id: i32, discord_user: i64) -> Result<()> {
        let permission = self.get_permission(list_id, discord_user).await?;
        if permission < ListPermission::Owner {
            return Err(ListError::Forbidden("Only the owner can delete the list").into());
        }
        let list = list::Entity::find_by_id(list_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::NotFound)?;
        let txn = self.db.begin().await?;
        let _items = list_item::Entity::delete_many()
            .filter(list_item::Column::ListId.eq(list.id))
            .exec(&txn)
            .await?;
        list.delete(&txn).await?;
        txn.commit().await?;
        Ok(())
    }

    pub async fn get_lists_for_user(&self, discord_user: i64) -> Result<Vec<list::Model>> {
        // This should probably also include lists shared with the user
        let owned_lists = list::Entity::find()
            .filter(list::Column::Owner.eq(discord_user))
            .all(&self.db)
            .await?;

        let shared_lists = list::Entity::find()
            .inner_join(list_shared_user::Entity)
            .filter(list_shared_user::Column::UserId.eq(discord_user))
            .all(&self.db)
            .await?;

        let group_lists = list::Entity::find()
            .inner_join(list_shared_group::Entity)
            .join(
                JoinType::InnerJoin,
                list_shared_group::Relation::UserGroup.def(),
            )
            .join(
                JoinType::InnerJoin,
                user_group::Relation::UserGroupMember.def(),
            )
            .filter(user_group_member::Column::UserId.eq(discord_user))
            .all(&self.db)
            .await?;

        let mut all_lists = owned_lists;
        all_lists.extend(shared_lists);
        all_lists.extend(group_lists);
        all_lists.sort_by_key(|l| l.id);
        all_lists.dedup_by_key(|l| l.id);

        Ok(all_lists)
    }

    pub async fn get_list_by_name_for_user(
        &self,
        discord_user: i64,
        list_name: &str,
    ) -> Result<Option<list::Model>> {
        let owned_list = list::Entity::find()
            .filter(list::Column::Owner.eq(discord_user))
            .filter(list::Column::Name.eq(list_name))
            .one(&self.db)
            .await?;
        if owned_list.is_some() {
            return Ok(owned_list);
        }

        let shared_list = list::Entity::find()
            .inner_join(list_shared_user::Entity)
            .filter(list_shared_user::Column::UserId.eq(discord_user))
            .filter(list::Column::Name.eq(list_name))
            .one(&self.db)
            .await?;
        if shared_list.is_some() {
            return Ok(shared_list);
        }

        let group_list = list::Entity::find()
            .inner_join(list_shared_group::Entity)
            .join(
                JoinType::InnerJoin,
                list_shared_group::Relation::UserGroup.def(),
            )
            .join(
                JoinType::InnerJoin,
                user_group::Relation::UserGroupMember.def(),
            )
            .filter(user_group_member::Column::UserId.eq(discord_user))
            .filter(list::Column::Name.eq(list_name))
            .one(&self.db)
            .await?;

        Ok(group_list)
    }

    pub async fn get_list(&self, list_id: i32, discord_user: i64) -> Result<list::Model> {
        let permission = self.get_permission(list_id, discord_user).await?;
        if permission < ListPermission::Read {
            return Err(ListError::Forbidden("Insufficient permissions to read list").into());
        }
        let list = list::Entity::find_by_id(list_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::NotFound)?;
        Ok(list)
    }

    pub async fn get_list_items(
        &self,
        list_id: i32,
        discord_user: i64,
    ) -> Result<Vec<list_item::Model>> {
        let permission = self.get_permission(list_id, discord_user).await?;
        if permission < ListPermission::Read {
            return Err(ListError::Forbidden("Insufficient permissions to read list items").into());
        }
        Ok(list_item::Entity::find()
            .filter(list_item::Column::ListId.eq(list_id))
            .all(&self.db)
            .await?)
    }

    pub async fn get_list_item(
        &self,
        list_item_id: i32,
        discord_user: i64,
    ) -> Result<list_item::Model> {
        let item = list_item::Entity::find_by_id(list_item_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::BadRequest("Item not found"))?;
        let permission = self.get_permission(item.list_id, discord_user).await?;
        if permission < ListPermission::Read {
            return Err(ListError::Forbidden("Insufficient permissions to read list item").into());
        }
        Ok(item)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_list_activity(
        &self,
        list_id: i32,
        actor_user_id: i64,
        actor_username: String,
        kind: ListActivityKind,
        list_item_id: Option<i32>,
        item_id: Option<i32>,
        payload: serde_json::Value,
        message: String,
    ) -> Result<list_activity::Model> {
        Ok(list_activity::ActiveModel {
            id: Default::default(),
            list_id: ActiveValue::Set(list_id),
            actor_user_id: ActiveValue::Set(actor_user_id),
            actor_username: ActiveValue::Set(actor_username),
            kind: ActiveValue::Set(kind.as_str().to_string()),
            list_item_id: ActiveValue::Set(list_item_id),
            item_id: ActiveValue::Set(item_id),
            payload: ActiveValue::Set(payload),
            message: ActiveValue::Set(message),
            created_at: ActiveValue::Set(chrono::Utc::now().into()),
        }
        .insert(&self.db)
        .await?)
    }

    pub async fn get_list_activity(
        &self,
        list_id: i32,
        discord_user: i64,
        limit: u64,
        before: Option<i64>,
    ) -> Result<Vec<list_activity::Model>> {
        let permission = self.get_permission(list_id, discord_user).await?;
        if permission < ListPermission::Read {
            return Err(
                ListError::Forbidden("Insufficient permissions to read list activity").into(),
            );
        }
        let mut query = list_activity::Entity::find()
            .filter(list_activity::Column::ListId.eq(list_id))
            .order_by_desc(list_activity::Column::Id)
            .limit(limit.clamp(1, 100));
        if let Some(before) = before {
            query = query.filter(list_activity::Column::Id.lt(before));
        }
        Ok(query.all(&self.db).await?)
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
        acquired: Option<i32>,
    ) -> Result<list_item::Model> {
        let permission = self.get_permission(list.id, discord_user).await?;
        if permission < ListPermission::Write {
            return Err(
                ListError::Forbidden("Insufficient permissions to add item to list").into(),
            );
        }
        // if the item already exists in the list, just update the existing list
        let existing = list_item::Entity::find()
            .filter(list_item::Column::ListId.eq(list.id))
            .filter(list_item::Column::ItemId.eq(item_id))
            .filter(list_item::Column::Hq.eq(hq))
            .one(&self.db)
            .await?;
        if let Some(item) = existing {
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
                acquired: ActiveValue::Set(acquired),
                target_price: ActiveValue::Set(None),
            }
            .insert(&self.db)
            .await?)
        }
    }

    /// Update list item
    #[instrument(skip(self))]
    pub async fn update_list_item(
        &self,
        updated_item: list_item::Model,
        discord_user: i64,
    ) -> Result<list_item::Model> {
        let permission = self
            .get_permission(updated_item.list_id, discord_user)
            .await?;
        if permission < ListPermission::Write {
            return Err(
                ListError::Forbidden("Insufficient permissions to update list item").into(),
            );
        }
        let mut item = list_item::Entity::find_by_id(updated_item.id)
            .one(&self.db)
            .await?
            .ok_or(ListError::BadRequest("Item not found"))?
            .into_active_model();
        item.hq.cmp_set_value(updated_item.hq);
        item.quantity.cmp_set_value(updated_item.quantity);
        item.acquired.cmp_set_value(updated_item.acquired);
        item.target_price.cmp_set_value(updated_item.target_price);
        if item.is_changed() {
            Ok(item.update(&self.db).await?)
        } else {
            Ok(updated_item)
        }
    }

    /// Update only the `target_price` on a list_item. Requires `Write`
    /// permission on the owning list. Pass `None` to clear an existing target.
    #[instrument(skip(self))]
    pub async fn set_list_item_target_price(
        &self,
        owner: i64,
        list_item_id: i32,
        target_price: Option<i64>,
    ) -> Result<()> {
        let item = list_item::Entity::find_by_id(list_item_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::BadRequest("Item not found"))?;
        let permission = self.get_permission(item.list_id, owner).await?;
        if permission < ListPermission::Write {
            return Err(
                ListError::Forbidden("Insufficient permissions to update list item").into(),
            );
        }
        let mut active: list_item::ActiveModel = item.into_active_model();
        active.target_price = ActiveValue::Set(target_price);
        active.update(&self.db).await?;
        Ok(())
    }

    /// Return all list_items for `list_id` that have a non-null `target_price`.
    /// Used by the price tracker to pre-compute per-list thresholds on refresh.
    pub async fn get_list_items_with_target(&self, list_id: i32) -> Result<Vec<list_item::Model>> {
        Ok(list_item::Entity::find()
            .filter(list_item::Column::ListId.eq(list_id))
            .filter(list_item::Column::TargetPrice.is_not_null())
            .all(&self.db)
            .await?)
    }

    /// Look up a list by id without a permission check. Used by internal code
    /// (the price tracker dispatch path) that has already authorized the
    /// operation via the `alert_list_threshold` row.
    pub async fn get_list_by_id(&self, list_id: i32) -> Result<Option<list::Model>> {
        Ok(list::Entity::find_by_id(list_id).one(&self.db).await?)
    }

    // #[instrument(skip(self))]
    pub async fn add_items_to_list(
        &self,
        list: &list::Model,
        discord_user: i64,
        items: impl Iterator<Item = list_item::Model>,
    ) -> Result<u64> {
        let permission = self.get_permission(list.id, discord_user).await?;
        if permission < ListPermission::Write {
            return Err(
                ListError::Forbidden("Insufficient permissions to add items to list").into(),
            );
        }
        // for items that are already matching our list, we should update and insert
        let mut existing_list_items: HashMap<_, _> = list
            .find_related(list_item::Entity)
            .all(&self.db)
            .await?
            .into_iter()
            .map(|item| ((item.list_id, item.hq, item.item_id), item))
            .collect();

        let mut insert_queue = vec![];
        let mut updated_models = vec![];
        items.into_iter().for_each(|item| {
            let key = (list.id, item.hq, item.item_id);
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
                hq,
                quantity,
                acquired,
                target_price,
                ..
            } = item;
            let list_id = list.id;
            list_item::ActiveModel {
                id: Default::default(),
                item_id: ActiveValue::Set(item_id),
                list_id: ActiveValue::Set(list_id),
                hq: ActiveValue::Set(hq),
                quantity: ActiveValue::Set(quantity),
                acquired: ActiveValue::Set(acquired),
                target_price: ActiveValue::Set(target_price),
            }
        }))
        .exec_without_returning(&self.db)
        .await?;
        Ok(many)
    }

    #[instrument(skip(self))]
    pub async fn set_list_items_hq(
        &self,
        discord_user: i64,
        list_item_ids: &[i32],
        hq: Option<bool>,
    ) -> Result<Vec<i32>> {
        let items = list_item::Entity::find()
            .filter(list_item::Column::Id.is_in(list_item_ids.to_vec()))
            .all(&self.db)
            .await?;
        let list_ids: HashSet<i32> = items.iter().map(|i| i.list_id).collect();
        let list_ids_vec: Vec<i32> = list_ids.iter().copied().collect();
        for list_id in list_ids {
            let permission = self.get_permission(list_id, discord_user).await?;
            if permission < ListPermission::Write {
                return Err(
                    ListError::Forbidden("Insufficient permissions to update list items").into(),
                );
            }
        }

        list_item::Entity::update_many()
            .col_expr(list_item::Column::Hq, Expr::value(hq))
            .filter(list_item::Column::Id.is_in(list_item_ids.to_vec()))
            .exec(&self.db)
            .await?;
        Ok(list_ids_vec)
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
            .ok_or(ListError::BadRequest("No list item"))?;
        let permission = self.get_permission(list_item.list_id, discord_user).await?;
        if permission < ListPermission::Write {
            return Err(
                ListError::Forbidden("Insufficient permissions to remove item from list").into(),
            );
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
            .ok_or(ListError::NotFound)?;
        let permission = self.get_permission(list_id, discord_user).await?;
        if permission < ListPermission::Read {
            return Err(ListError::Forbidden("Insufficient permissions to read list").into());
        }
        let selector = AnySelector::try_from(&list)?;
        let result = world_cache.lookup_selector(&selector)?;
        let worlds = world_cache
            .get_all_worlds_in(&result)
            .ok_or(anyhow!("Unable to get worlds for list"))?;
        let list_items = list_item::Entity::find()
            .filter(list_item::Column::ListId.eq(list_id))
            .all(&self.db)
            .await?;
        let worlds = &worlds;
        try_join_all(list_items.into_iter().map(|item| async move {
            self.get_all_listings_in_worlds_with_retainers(worlds, ItemId(item.item_id))
                .await
                .map(|listings| (item, listings))
        }))
        .await
    }

    // --- Group Management ---

    pub async fn create_group(&self, name: String, owner_id: i64) -> Result<user_group::Model> {
        let txn = self.db.begin().await?;
        let group = user_group::ActiveModel {
            id: Default::default(),
            name: ActiveValue::Set(name),
            owner_id: ActiveValue::Set(owner_id),
        }
        .insert(&txn)
        .await?;
        user_group_member::ActiveModel {
            group_id: ActiveValue::Set(group.id),
            user_id: ActiveValue::Set(owner_id),
        }
        .insert(&txn)
        .await?;
        txn.commit().await?;
        Ok(group)
    }

    pub async fn delete_group(&self, group_id: i32, owner_id: i64) -> Result<()> {
        let group = user_group::Entity::find_by_id(group_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::BadRequest("Group not found"))?;
        if group.owner_id != owner_id {
            return Err(ListError::Forbidden("Only the owner can delete the group").into());
        }
        group.delete(&self.db).await?;
        Ok(())
    }

    pub async fn add_group_member(&self, group_id: i32, owner_id: i64, user_id: i64) -> Result<()> {
        let group = user_group::Entity::find_by_id(group_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::BadRequest("Group not found"))?;
        if group.owner_id != owner_id {
            return Err(ListError::Forbidden("Only the owner can add members").into());
        }
        user_group_member::ActiveModel {
            group_id: ActiveValue::Set(group_id),
            user_id: ActiveValue::Set(user_id),
        }
        .insert(&self.db)
        .await?;
        Ok(())
    }

    pub async fn remove_group_member(
        &self,
        group_id: i32,
        owner_id: i64,
        user_id: i64,
    ) -> Result<()> {
        let group = user_group::Entity::find_by_id(group_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::BadRequest("Group not found"))?;
        if group.owner_id != owner_id && user_id != owner_id {
            return Err(ListError::Forbidden(
                "Only the owner or the user themselves can remove a member",
            )
            .into());
        }
        user_group_member::Entity::delete_by_id((group_id, user_id))
            .exec(&self.db)
            .await?;
        Ok(())
    }

    pub async fn get_groups_for_user(&self, user_id: i64) -> Result<Vec<user_group::Model>> {
        let owned_groups = user_group::Entity::find()
            .filter(user_group::Column::OwnerId.eq(user_id))
            .all(&self.db)
            .await?;
        let member_groups = user_group::Entity::find()
            .inner_join(user_group_member::Entity)
            .filter(user_group_member::Column::UserId.eq(user_id))
            .all(&self.db)
            .await?;
        let mut all_groups = owned_groups;
        all_groups.extend(member_groups);
        all_groups.sort_by_key(|g| g.id);
        all_groups.dedup_by_key(|g| g.id);
        Ok(all_groups)
    }

    // --- Sharing Management ---

    pub async fn share_list_with_user(
        &self,
        list_id: i32,
        owner_id: i64,
        user_id: i64,
        permission: ListPermission,
    ) -> Result<()> {
        let current_perm = self.get_permission(list_id, owner_id).await?;
        if current_perm < ListPermission::Owner {
            return Err(ListError::Forbidden("Only the owner can share the list").into());
        }
        validate_share_permission(permission)?;
        list_shared_user::Entity::insert(list_shared_user::ActiveModel {
            list_id: ActiveValue::Set(list_id),
            user_id: ActiveValue::Set(user_id),
            permission: ActiveValue::Set(permission as i16),
        })
        .on_conflict(
            sea_orm::sea_query::OnConflict::columns([
                list_shared_user::Column::ListId,
                list_shared_user::Column::UserId,
            ])
            .update_column(list_shared_user::Column::Permission)
            .to_owned(),
        )
        .exec(&self.db)
        .await?;
        Ok(())
    }

    pub async fn share_list_with_group(
        &self,
        list_id: i32,
        owner_id: i64,
        group_id: i32,
        permission: ListPermission,
    ) -> Result<()> {
        let current_perm = self.get_permission(list_id, owner_id).await?;
        if current_perm < ListPermission::Owner {
            return Err(ListError::Forbidden("Only the owner can share the list").into());
        }
        validate_share_permission(permission)?;
        let group = user_group::Entity::find_by_id(group_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::BadRequest("Group not found"))?;
        if group.owner_id != owner_id {
            return Err(ListError::Forbidden(
                "Only the group owner can share a list with that group",
            )
            .into());
        }
        list_shared_group::Entity::insert(list_shared_group::ActiveModel {
            list_id: ActiveValue::Set(list_id),
            group_id: ActiveValue::Set(group_id),
            permission: ActiveValue::Set(permission as i16),
        })
        .on_conflict(
            sea_orm::sea_query::OnConflict::columns([
                list_shared_group::Column::ListId,
                list_shared_group::Column::GroupId,
            ])
            .update_column(list_shared_group::Column::Permission)
            .to_owned(),
        )
        .exec(&self.db)
        .await?;
        Ok(())
    }

    pub async fn unshare_list_from_user(
        &self,
        list_id: i32,
        owner_id: i64,
        user_id: i64,
    ) -> Result<()> {
        let current_perm = self.get_permission(list_id, owner_id).await?;
        if current_perm < ListPermission::Owner && owner_id != user_id {
            return Err(ListError::Forbidden("Only the owner can unshare the list").into());
        }
        list_shared_user::Entity::delete_by_id((list_id, user_id))
            .exec(&self.db)
            .await?;
        Ok(())
    }

    pub async fn unshare_list_from_group(
        &self,
        list_id: i32,
        owner_id: i64,
        group_id: i32,
    ) -> Result<()> {
        let current_perm = self.get_permission(list_id, owner_id).await?;
        if current_perm < ListPermission::Owner {
            return Err(ListError::Forbidden("Only the owner can unshare the list").into());
        }
        list_shared_group::Entity::delete_by_id((list_id, group_id))
            .exec(&self.db)
            .await?;
        Ok(())
    }

    // --- Invite Management ---

    pub async fn create_invite(
        &self,
        list_id: i32,
        owner_id: i64,
        permission: ListPermission,
        max_uses: Option<i32>,
    ) -> Result<list_invite::Model> {
        let current_perm = self.get_permission(list_id, owner_id).await?;
        if current_perm < ListPermission::Owner {
            return Err(ListError::Forbidden("Only the owner can create invites").into());
        }
        validate_share_permission(permission)?;
        if matches!(max_uses, Some(max_uses) if max_uses <= 0) {
            return Err(ListError::BadRequest("Invite max uses must be positive").into());
        }
        let id = new_invite_id()?;

        Ok(list_invite::ActiveModel {
            id: ActiveValue::Set(id),
            list_id: ActiveValue::Set(list_id),
            permission: ActiveValue::Set(permission as i16),
            max_uses: ActiveValue::Set(max_uses),
            uses: ActiveValue::Set(0),
        }
        .insert(&self.db)
        .await?)
    }

    pub async fn use_invite(
        &self,
        invite_id: String,
        user_id: i64,
    ) -> Result<list_shared_user::Model> {
        let txn = self.db.begin().await?;

        // Atomic conditional increment: only succeeds if the invite exists and
        // either has no max_uses cap or hasn't hit it yet. This closes the
        // TOCTOU window where two concurrent redemptions could both pass a
        // pre-check and then both increment.
        let update = list_invite::Entity::update_many()
            .col_expr(
                list_invite::Column::Uses,
                Expr::col(list_invite::Column::Uses).add(1),
            )
            .filter(list_invite::Column::Id.eq(invite_id.clone()))
            .filter(
                Condition::any()
                    .add(list_invite::Column::MaxUses.is_null())
                    .add(
                        Expr::col(list_invite::Column::Uses)
                            .lt(Expr::col(list_invite::Column::MaxUses)),
                    ),
            )
            .exec(&txn)
            .await?;

        if update.rows_affected == 0 {
            txn.rollback().await?;
            // Distinguish "not found" from "exhausted" so callers can give a
            // useful error message.
            let exists = list_invite::Entity::find_by_id(invite_id.clone())
                .one(&self.db)
                .await?
                .is_some();
            return Err(if exists {
                ListError::InviteExhausted.into()
            } else {
                ListError::InviteNotFound.into()
            });
        }

        // We held the row lock via the conditional update, so it's safe to
        // re-read for the list_id / permission needed below.
        let invite = list_invite::Entity::find_by_id(invite_id)
            .one(&txn)
            .await?
            .ok_or(ListError::InviteNotFound)?;

        list_shared_user::Entity::insert(list_shared_user::ActiveModel {
            list_id: ActiveValue::Set(invite.list_id),
            user_id: ActiveValue::Set(user_id),
            permission: ActiveValue::Set(invite.permission),
        })
        .on_conflict(
            sea_orm::sea_query::OnConflict::columns([
                list_shared_user::Column::ListId,
                list_shared_user::Column::UserId,
            ])
            .update_column(list_shared_user::Column::Permission)
            .to_owned(),
        )
        .exec(&txn)
        .await?;

        let shared = list_shared_user::Entity::find_by_id((invite.list_id, user_id))
            .one(&txn)
            .await?
            .ok_or(ListError::BadRequest(
                "Invite redemption did not create a share",
            ))?;

        txn.commit().await?;
        Ok(shared)
    }

    pub async fn delete_invite(&self, invite_id: String, owner_id: i64) -> Result<()> {
        let invite = list_invite::Entity::find_by_id(invite_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::InviteNotFound)?;
        let permission = self.get_permission(invite.list_id, owner_id).await?;
        if permission < ListPermission::Owner {
            return Err(ListError::Forbidden("Only the owner can delete invites").into());
        }
        invite.delete(&self.db).await?;
        Ok(())
    }

    pub async fn get_list_invites(
        &self,
        list_id: i32,
        user_id: i64,
    ) -> Result<Vec<list_invite::Model>> {
        let permission = self.get_permission(list_id, user_id).await?;
        if permission < ListPermission::Owner {
            return Err(ListError::Forbidden("Only the owner can view invites").into());
        }
        Ok(list_invite::Entity::find()
            .filter(list_invite::Column::ListId.eq(list_id))
            .all(&self.db)
            .await?)
    }

    pub async fn get_list_shared_users(
        &self,
        list_id: i32,
        user_id: i64,
    ) -> Result<Vec<ListSharedUserReturn>> {
        let permission = self.get_permission(list_id, user_id).await?;
        if permission < ListPermission::Owner {
            return Err(ListError::Forbidden("Only the owner can view shares").into());
        }
        Ok(list_shared_user::Entity::find()
            .filter(list_shared_user::Column::ListId.eq(list_id))
            .find_also_related(discord_user::Entity)
            .all(&self.db)
            .await?
            .into_iter()
            .filter_map(|(shared, user)| user.map(|u| ListSharedUserReturn(shared, u)))
            .collect())
    }

    pub async fn get_list_shared_groups(
        &self,
        list_id: i32,
        user_id: i64,
    ) -> Result<Vec<ListSharedGroupReturn>> {
        let permission = self.get_permission(list_id, user_id).await?;
        if permission < ListPermission::Owner {
            return Err(ListError::Forbidden("Only the owner can view shares").into());
        }
        Ok(list_shared_group::Entity::find()
            .filter(list_shared_group::Column::ListId.eq(list_id))
            .find_also_related(user_group::Entity)
            .all(&self.db)
            .await?
            .into_iter()
            .filter_map(|(shared, group)| group.map(|g| ListSharedGroupReturn(shared, g)))
            .collect())
    }

    pub async fn get_group_members(
        &self,
        group_id: i32,
        user_id: i64,
    ) -> Result<Vec<UserGroupMemberReturn>> {
        let group = user_group::Entity::find_by_id(group_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::BadRequest("Group not found"))?;

        // Owner sees all; otherwise the requester must already be a member.
        let is_member = user_group_member::Entity::find()
            .filter(user_group_member::Column::GroupId.eq(group_id))
            .filter(user_group_member::Column::UserId.eq(user_id))
            .one(&self.db)
            .await?
            .is_some();

        if group.owner_id != user_id && !is_member {
            return Err(ListError::Forbidden(
                "You must be a member of the group to see other members",
            )
            .into());
        }

        Ok(user_group_member::Entity::find()
            .filter(user_group_member::Column::GroupId.eq(group_id))
            .find_also_related(discord_user::Entity)
            .all(&self.db)
            .await?
            .into_iter()
            .filter_map(|(member, user)| user.map(|u| UserGroupMemberReturn(member, u)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_permission_accepts_only_read_or_write() {
        assert!(validate_share_permission(ListPermission::Read).is_ok());
        assert!(validate_share_permission(ListPermission::Write).is_ok());
        assert!(validate_share_permission(ListPermission::None).is_err());
        assert!(validate_share_permission(ListPermission::Owner).is_err());
    }

    #[test]
    fn invite_ids_are_hex_encoded_24_random_bytes() {
        let first = new_invite_id().unwrap();
        let second = new_invite_id().unwrap();
        assert_eq!(first.len(), 48);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }
}
