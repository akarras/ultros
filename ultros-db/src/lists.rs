use crate::{
    UltrosDb,
    common::try_update_value::ActiveValueCmpSet,
    common_type_conversions::{
        ListSharedGroupReturn, ListSharedUserReturn, UserGroupMemberReturn,
    },
    entity::{
        active_listing, discord_user, list, list_invite, list_item, list_shared_group,
        list_shared_user, retainer, user_group, user_group_member,
    },
    world_data::world_cache::{AnySelector, WorldCache},
};
use anyhow::Result;
use anyhow::anyhow;
use futures::future::try_join_all;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityTrait, IntoActiveModel, JoinType, ModelTrait,
    QueryFilter, QuerySelect, RelationTrait, TransactionTrait,
};
use std::{collections::HashMap, sync::Arc};
use tracing::instrument;
use ultros_api_types::list::ListPermission;
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
    pub async fn get_permission(&self, list_id: i32, user_id: i64) -> Result<ListPermission> {
        let list = list::Entity::find_by_id(list_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("List not found"))?;

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

        // Check shared groups
        let shared_groups = list_shared_group::Entity::find()
            .filter(list_shared_group::Column::ListId.eq(list_id))
            .all(&self.db)
            .await?;

        for shared_group in shared_groups {
            // check if user is in group
            let is_member = user_group_member::Entity::find()
                .filter(user_group_member::Column::GroupId.eq(shared_group.group_id))
                .filter(user_group_member::Column::UserId.eq(user_id))
                .one(&self.db)
                .await?
                .is_some();

            if is_member {
                let perm = ListPermission::from(shared_group.permission);
                if perm > max_permission {
                    max_permission = perm;
                }
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
        if permission < ListPermission::Write {
            return Err(anyhow!("Insufficient permissions to update list"));
        }
        let list = list::Entity::find_by_id(list_id)
            .one(&self.db)
            .await?
            .ok_or(anyhow!("Unable to find list"))?;
        let mut model = list.into_active_model();
        update(&mut model);
        Ok(model.update(&self.db).await?)
    }

    /// Deletes the given list assuming that it is owned by the Discord user
    #[instrument(skip(self))]
    pub async fn delete_list(&self, list_id: i32, discord_user: i64) -> Result<()> {
        let permission = self.get_permission(list_id, discord_user).await?;
        if permission < ListPermission::Owner {
            return Err(anyhow!("Insufficient permissions to delete list"));
        }
        let list = list::Entity::find_by_id(list_id)
            .one(&self.db)
            .await?
            .ok_or(anyhow::anyhow!("Failed to find list with that ID"))?;
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

    pub async fn get_list(&self, list_id: i32, discord_user: i64) -> Result<list::Model> {
        let permission = self.get_permission(list_id, discord_user).await?;
        if permission < ListPermission::Read {
            return Err(anyhow!("Insufficient permissions to read list"));
        }
        let list = list::Entity::find_by_id(list_id)
            .one(&self.db)
            .await?
            .ok_or(anyhow!("List not found"))?;
        Ok(list)
    }

    pub async fn get_list_items(
        &self,
        list_id: i32,
        discord_user: i64,
    ) -> Result<Vec<list_item::Model>> {
        let permission = self.get_permission(list_id, discord_user).await?;
        if permission < ListPermission::Read {
            return Err(anyhow!("Insufficient permissions to read list items"));
        }
        Ok(list_item::Entity::find()
            .filter(list_item::Column::ListId.eq(list_id))
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
        acquired: Option<i32>,
    ) -> Result<list_item::Model> {
        let permission = self.get_permission(list.id, discord_user).await?;
        if permission < ListPermission::Write {
            return Err(anyhow::anyhow!(
                "Insufficient permissions to add item to list"
            ));
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
            return Err(anyhow!("Insufficient permissions to update list item"));
        }
        let mut item = list_item::Entity::find_by_id(updated_item.id)
            .one(&self.db)
            .await?
            .ok_or(anyhow!("Item not found"))?
            .into_active_model();
        item.hq.cmp_set_value(updated_item.hq);
        item.quantity.cmp_set_value(updated_item.quantity);
        if item.is_changed() {
            Ok(item.update(&self.db).await?)
        } else {
            Ok(updated_item)
        }
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
            return Err(anyhow::anyhow!(
                "Insufficient permissions to add items to list"
            ));
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
        let permission = self.get_permission(list_item.list_id, discord_user).await?;
        if permission < ListPermission::Write {
            return Err(anyhow!("Insufficient permissions to remove item from list"));
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
        let permission = self.get_permission(list_id, discord_user).await?;
        if permission < ListPermission::Read {
            return Err(anyhow!("Insufficient permissions to read list"));
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
        Ok(user_group::ActiveModel {
            id: Default::default(),
            name: ActiveValue::Set(name),
            owner_id: ActiveValue::Set(owner_id),
        }
        .insert(&self.db)
        .await?)
    }

    pub async fn delete_group(&self, group_id: i32, owner_id: i64) -> Result<()> {
        let group = user_group::Entity::find_by_id(group_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("Group not found"))?;
        if group.owner_id != owner_id {
            return Err(anyhow!("Only the owner can delete the group"));
        }
        group.delete(&self.db).await?;
        Ok(())
    }

    pub async fn add_group_member(&self, group_id: i32, owner_id: i64, user_id: i64) -> Result<()> {
        let group = user_group::Entity::find_by_id(group_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("Group not found"))?;
        if group.owner_id != owner_id {
            return Err(anyhow!("Only the owner can add members"));
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
            .ok_or_else(|| anyhow!("Group not found"))?;
        if group.owner_id != owner_id && user_id != owner_id {
            return Err(anyhow!(
                "Only the owner or the user themselves can remove a member"
            ));
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
            return Err(anyhow!("Only the owner can share the list"));
        }
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
            return Err(anyhow!("Only the owner can share the list"));
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
            return Err(anyhow!("Only the owner can unshare the list"));
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
            return Err(anyhow!("Only the owner can unshare the list"));
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
            return Err(anyhow!("Only the owner can create invites"));
        }
        // generate a random 16-char string for the invite ID
        let id: String = std::iter::repeat_with(fastrand::alphanumeric)
            .take(16)
            .collect();

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
        let invite = list_invite::Entity::find_by_id(invite_id.clone())
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("Invite not found"))?;

        if invite
            .max_uses
            .is_some_and(|max_uses| invite.uses >= max_uses)
        {
            return Err(anyhow!("Invite has reached max uses"));
        }

        let txn = self.db.begin().await?;

        // increment uses
        let mut invite_active: list_invite::ActiveModel = invite.clone().into_active_model();
        invite_active.uses = ActiveValue::Set(invite.uses + 1);
        invite_active.update(&txn).await?;

        // share list
        let shared = list_shared_user::ActiveModel {
            list_id: ActiveValue::Set(invite.list_id),
            user_id: ActiveValue::Set(user_id),
            permission: ActiveValue::Set(invite.permission),
        }
        .insert(&txn)
        .await?;

        txn.commit().await?;
        Ok(shared)
    }

    pub async fn delete_invite(&self, invite_id: String, owner_id: i64) -> Result<()> {
        let invite = list_invite::Entity::find_by_id(invite_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("Invite not found"))?;
        let permission = self.get_permission(invite.list_id, owner_id).await?;
        if permission < ListPermission::Owner {
            return Err(anyhow!("Only the owner can delete invites"));
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
            return Err(anyhow!("Only the owner can view invites"));
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
            return Err(anyhow!("Only the owner can view shares"));
        }
        Ok(list_shared_user::Entity::find()
            .filter(list_shared_user::Column::ListId.eq(list_id))
            .find_also_related(discord_user::Entity)
            .all(&self.db)
            .await?
            .into_iter()
            .map(|(shared, user)| ListSharedUserReturn(shared, user.unwrap()))
            .collect())
    }

    pub async fn get_list_shared_groups(
        &self,
        list_id: i32,
        user_id: i64,
    ) -> Result<Vec<ListSharedGroupReturn>> {
        let permission = self.get_permission(list_id, user_id).await?;
        if permission < ListPermission::Owner {
            return Err(anyhow!("Only the owner can view shares"));
        }
        Ok(list_shared_group::Entity::find()
            .filter(list_shared_group::Column::ListId.eq(list_id))
            .find_also_related(user_group::Entity)
            .all(&self.db)
            .await?
            .into_iter()
            .map(|(shared, group)| ListSharedGroupReturn(shared, group.unwrap()))
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
            .ok_or_else(|| anyhow!("Group not found"))?;

        // check if user is member or owner
        let is_member = user_group_member::Entity::find()
            .filter(user_group_member::Column::GroupId.eq(group_id))
            .filter(user_group_member::Column::UserId.eq(user_id))
            .one(&self.db)
            .await?
            .is_some();

        if group.owner_id != user_id && !is_member {
            return Err(anyhow!(
                "You must be a member of the group to see other members"
            ));
        }

        Ok(user_group_member::Entity::find()
            .filter(user_group_member::Column::GroupId.eq(group_id))
            .find_also_related(discord_user::Entity)
            .all(&self.db)
            .await?
            .into_iter()
            .map(|(member, user)| UserGroupMemberReturn(member, user.unwrap()))
            .collect())
    }
}
