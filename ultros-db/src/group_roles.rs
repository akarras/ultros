//! Roles inside a group, the group-membership invariant, and the DB side of
//! Discord membership sync.
//!
//! Invariant enforced here and in `lists.rs`'s group functions: every
//! `group_role_member` row has a matching `user_group_member` row. Adding to a
//! role adds to the group; removing from the group removes from every role.

use crate::{
    UltrosDb,
    common_type_conversions::GroupRoleReturn,
    entity::{discord_user, group_role, group_role_member, user_group, user_group_member},
};
use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseTransaction, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, RelationTrait, SqlErr, TransactionTrait,
    sea_query::OnConflict,
};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use ultros_api_types::user::group::{
    GroupMemberSource, GroupRoleSource, GroupRoleSyncState, GroupSource,
};

#[derive(Debug, Error)]
pub enum GroupError {
    #[error("Group not found")]
    NotFound,
    #[error("Role not found")]
    RoleNotFound,
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0}")]
    BadRequest(&'static str),
    #[error("That member is managed by Discord; change their Discord role instead")]
    ManagedByDiscord,
    #[error("That role is managed by Discord; its members come from the Discord role")]
    RoleManagedByDiscord,
}

/// A Discord-backed role that reconciliation should keep in step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncedRole {
    pub role_id: i32,
    pub group_id: i32,
    pub discord_role_id: i64,
}

/// The changes reconciliation (or a gateway event) wants applied to one role.
#[derive(Debug, Clone, Default)]
pub struct RoleSyncPlan {
    pub role_id: i32,
    /// `(discord user id, display name)`.
    pub adds: Vec<(i64, String)>,
    pub removes: Vec<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncSummary {
    pub added: usize,
    pub removed: usize,
    /// Members who left the group entirely because they held no other
    /// synced role and were themselves sync-added.
    pub left_group: usize,
    /// Members who held no other synced role but held a manual role, so
    /// their membership was handed over to `Manual` instead of being
    /// removed.
    pub handed_over: usize,
}

/// Both member-search paths cap at ten rows; the spec fixes the number so the
/// picker looks the same whether it is backed by Discord or by our own table.
pub const MEMBER_SEARCH_LIMIT: u64 = 10;

/// How many rows one `refresh_member_display_names` statement carries. Sized
/// to keep the parameter count well inside Postgres' 65535 limit (two bound
/// parameters per row) while still making a whole-server refresh a handful of
/// round trips rather than thousands.
const DISPLAY_NAME_REFRESH_CHUNK: usize = 500;

/// Neutralise the wildcards a user can type so `%` in a search box matches a
/// literal `%` instead of every row. Postgres `LIKE`/`ILIKE` take `\` as the
/// default escape character, so the backslash itself has to be doubled first.
fn escape_like(query: &str) -> String {
    query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn clean_role_name(name: String) -> Result<String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(GroupError::BadRequest("Role name cannot be blank").into());
    }
    if name.chars().count() > 100 {
        return Err(GroupError::BadRequest("Role name is too long").into());
    }
    Ok(name)
}

impl UltrosDb {
    async fn load_owned_group(&self, group_id: i32, owner_id: i64) -> Result<user_group::Model> {
        let group = user_group::Entity::find_by_id(group_id)
            .one(&self.db)
            .await?
            .ok_or(GroupError::NotFound)?;
        if group.owner_id != owner_id {
            return Err(GroupError::Forbidden("Only the group owner can manage roles").into());
        }
        Ok(group)
    }

    async fn require_group_member(&self, group_id: i32, user_id: i64) -> Result<user_group::Model> {
        let group = user_group::Entity::find_by_id(group_id)
            .one(&self.db)
            .await?
            .ok_or(GroupError::NotFound)?;
        if group.owner_id == user_id {
            return Ok(group);
        }
        let is_member = user_group_member::Entity::find_by_id((group_id, user_id))
            .one(&self.db)
            .await?
            .is_some();
        if !is_member {
            return Err(GroupError::Forbidden("You must be a member of the group").into());
        }
        Ok(group)
    }

    async fn load_role_in_group(&self, group_id: i32, role_id: i32) -> Result<group_role::Model> {
        group_role::Entity::find_by_id(role_id)
            .filter(group_role::Column::GroupId.eq(group_id))
            .one(&self.db)
            .await?
            .ok_or_else(|| GroupError::RoleNotFound.into())
    }

    /// The group, but only for its owner. The HTTP layer needs the row itself
    /// (`guild_id`, `frozen_reason`) before it can decide whether a request
    /// goes to Discord or stays local, and it must not leak a group's
    /// existence to a non-owner, so the check and the fetch belong together.
    pub async fn get_owned_group(&self, group_id: i32, owner_id: i64) -> Result<user_group::Model> {
        self.load_owned_group(group_id, owner_id).await
    }

    /// Owner-only candidate search for a group with no Discord link: a
    /// case-insensitive prefix match over people who have logged into Ultros.
    /// Capped at [`MEMBER_SEARCH_LIMIT`] to match Discord's own search.
    pub async fn search_group_member_candidates(
        &self,
        group_id: i32,
        owner_id: i64,
        query: &str,
    ) -> Result<Vec<discord_user::Model>> {
        self.load_owned_group(group_id, owner_id).await?;
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        Ok(discord_user::Entity::find()
            .filter(discord_user::Column::Username.ilike(format!("{}%", escape_like(query))))
            .order_by_asc(discord_user::Column::Username)
            .limit(MEMBER_SEARCH_LIMIT)
            .all(&self.db)
            .await?)
    }

    /// Which of these Discord ids already have an Ultros account. Drives the
    /// "not on Ultros yet" hint on Discord-backed member search; adding them
    /// still works, so this is a hint and not a filter.
    pub async fn discord_users_present(&self, user_ids: &[i64]) -> Result<HashSet<i64>> {
        if user_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let ids: Vec<i64> = discord_user::Entity::find()
            .select_only()
            .column(discord_user::Column::Id)
            .filter(discord_user::Column::Id.is_in(user_ids.iter().copied()))
            .into_tuple()
            .all(&self.db)
            .await?;
        Ok(ids.into_iter().collect())
    }

    pub async fn create_group_role(
        &self,
        group_id: i32,
        owner_id: i64,
        name: String,
    ) -> Result<group_role::Model> {
        self.load_owned_group(group_id, owner_id).await?;
        let name = clean_role_name(name)?;
        // Manual roles sort after imported ones, which carry Discord's
        // position; a fresh manual role goes to the end.
        let next_position: Option<i32> = group_role::Entity::find()
            .select_only()
            .column_as(group_role::Column::Position.max(), "max_position")
            .filter(group_role::Column::GroupId.eq(group_id))
            .into_tuple()
            .one(&self.db)
            .await?
            .flatten();
        Ok(group_role::ActiveModel {
            id: Default::default(),
            group_id: ActiveValue::Set(group_id),
            name: ActiveValue::Set(name),
            discord_role_id: ActiveValue::Set(None),
            source: ActiveValue::Set(GroupRoleSource::Manual as i16),
            sync_state: ActiveValue::Set(GroupRoleSyncState::Synced as i16),
            last_synced_at: ActiveValue::Set(None),
            position: ActiveValue::Set(next_position.unwrap_or(0) + 1),
        }
        .insert(&self.db)
        .await?)
    }

    /// Import a Discord role into a guild-linked group. The caller has
    /// verified the role exists in the guild; this layer only knows ids.
    /// Membership is filled in by reconciliation, not here.
    pub async fn import_discord_role(
        &self,
        group_id: i32,
        owner_id: i64,
        discord_role_id: i64,
        name: String,
        position: i32,
    ) -> Result<group_role::Model> {
        let group = self.load_owned_group(group_id, owner_id).await?;
        if group.guild_id.is_none() || group.frozen_reason.is_some() {
            return Err(GroupError::BadRequest(
                "Only a group linked to a Discord server can import roles",
            )
            .into());
        }
        let name = clean_role_name(name)?;
        let already = group_role::Entity::find()
            .filter(group_role::Column::GroupId.eq(group_id))
            .filter(group_role::Column::DiscordRoleId.eq(discord_role_id))
            .one(&self.db)
            .await?
            .is_some();
        if already {
            return Err(GroupError::BadRequest("That Discord role is already imported").into());
        }
        let txn = self.db.begin().await?;
        let role = group_role::ActiveModel {
            id: Default::default(),
            group_id: ActiveValue::Set(group_id),
            name: ActiveValue::Set(name),
            discord_role_id: ActiveValue::Set(Some(discord_role_id)),
            source: ActiveValue::Set(GroupRoleSource::DiscordRole as i16),
            sync_state: ActiveValue::Set(GroupRoleSyncState::Synced as i16),
            last_synced_at: ActiveValue::Set(None),
            position: ActiveValue::Set(position),
        }
        .insert(&txn)
        .await
        // Two owners importing the same Discord role at the same time both
        // pass the `already` check above, then race into the unique index on
        // `(group_id, discord_role_id)`. The loser gets the same 400 the
        // pre-check would have produced, not a 500.
        .map_err(|error| match error.sql_err() {
            Some(SqlErr::UniqueConstraintViolation(_)) => anyhow::Error::from(
                GroupError::BadRequest("That Discord role is already imported"),
            ),
            _ => anyhow::Error::from(error),
        })?;
        user_group::Entity::update_many()
            .col_expr(
                user_group::Column::Source,
                sea_orm::sea_query::Expr::value(GroupSource::DiscordGuildMirrored as i16),
            )
            .filter(user_group::Column::Id.eq(group_id))
            .exec(&txn)
            .await?;
        txn.commit().await?;
        Ok(role)
    }

    pub async fn rename_group_role(
        &self,
        group_id: i32,
        owner_id: i64,
        role_id: i32,
        name: String,
    ) -> Result<group_role::Model> {
        self.load_owned_group(group_id, owner_id).await?;
        let role = self.load_role_in_group(group_id, role_id).await?;
        let name = clean_role_name(name)?;
        let mut active: group_role::ActiveModel = role.into();
        active.name = ActiveValue::Set(name);
        Ok(active.update(&self.db).await?)
    }

    /// Delete a role. Members stay in the group; only the role goes. If it
    /// was the group's last synced role the group drops back to
    /// `DiscordGuild`, since sync no longer owns any of its membership.
    pub async fn delete_group_role(
        &self,
        group_id: i32,
        owner_id: i64,
        role_id: i32,
    ) -> Result<()> {
        let group = self.load_owned_group(group_id, owner_id).await?;
        let role = self.load_role_in_group(group_id, role_id).await?;
        let txn = self.db.begin().await?;
        group_role::Entity::delete_by_id(role.id).exec(&txn).await?;
        if GroupSource::from(group.source) == GroupSource::DiscordGuildMirrored {
            let remaining_synced = group_role::Entity::find()
                .filter(group_role::Column::GroupId.eq(group_id))
                .filter(group_role::Column::Source.eq(GroupRoleSource::DiscordRole as i16))
                .count(&txn)
                .await?;
            if remaining_synced == 0 {
                user_group::Entity::update_many()
                    .col_expr(
                        user_group::Column::Source,
                        sea_orm::sea_query::Expr::value(GroupSource::DiscordGuild as i16),
                    )
                    .filter(user_group::Column::Id.eq(group_id))
                    .exec(&txn)
                    .await?;
            }
        }
        txn.commit().await?;
        Ok(())
    }

    /// Roles of a group with member counts, for members of that group.
    pub async fn get_group_roles(
        &self,
        group_id: i32,
        user_id: i64,
    ) -> Result<Vec<GroupRoleReturn>> {
        self.require_group_member(group_id, user_id).await?;
        let roles = group_role::Entity::find()
            .filter(group_role::Column::GroupId.eq(group_id))
            .order_by_asc(group_role::Column::Position)
            .order_by_asc(group_role::Column::Id)
            .all(&self.db)
            .await?;
        let counts: Vec<(i32, i64)> = group_role_member::Entity::find()
            .select_only()
            .column(group_role_member::Column::RoleId)
            .column_as(group_role_member::Column::UserId.count(), "member_count")
            .join(
                sea_orm::JoinType::InnerJoin,
                group_role_member::Relation::GroupRole.def(),
            )
            .filter(group_role::Column::GroupId.eq(group_id))
            .group_by(group_role_member::Column::RoleId)
            .into_tuple()
            .all(&self.db)
            .await?;
        let counts: HashMap<i32, i64> = counts.into_iter().collect();
        Ok(roles
            .into_iter()
            .map(|role| {
                let count = counts.get(&role.id).copied().unwrap_or(0);
                GroupRoleReturn(role, count)
            })
            .collect())
    }

    pub async fn get_group_role_members(
        &self,
        group_id: i32,
        user_id: i64,
        role_id: i32,
    ) -> Result<Vec<crate::common_type_conversions::UserGroupMemberReturn>> {
        self.require_group_member(group_id, user_id).await?;
        self.load_role_in_group(group_id, role_id).await?;
        let in_role: HashSet<i64> = group_role_member::Entity::find()
            .select_only()
            .column(group_role_member::Column::UserId)
            .filter(group_role_member::Column::RoleId.eq(role_id))
            .into_tuple()
            .all(&self.db)
            .await?
            .into_iter()
            .collect();
        Ok(self
            .get_group_members(group_id, user_id)
            .await?
            .into_iter()
            .filter(|m| in_role.contains(&m.0.user_id))
            .collect())
    }

    /// Owner adds a user to a manual role. The user joins the group as a
    /// `Manual` member if they were not already in it (the invariant).
    pub async fn add_group_role_member(
        &self,
        group_id: i32,
        owner_id: i64,
        role_id: i32,
        user_id: i64,
    ) -> Result<()> {
        self.load_owned_group(group_id, owner_id).await?;
        let role = self.load_role_in_group(group_id, role_id).await?;
        if GroupRoleSource::from(role.source) == GroupRoleSource::DiscordRole {
            return Err(GroupError::RoleManagedByDiscord.into());
        }
        let txn = self.db.begin().await?;
        ensure_group_member(&txn, group_id, user_id, GroupMemberSource::Manual).await?;
        insert_role_member(&txn, role_id, user_id).await?;
        txn.commit().await?;
        Ok(())
    }

    pub async fn remove_group_role_member(
        &self,
        group_id: i32,
        owner_id: i64,
        role_id: i32,
        user_id: i64,
    ) -> Result<()> {
        self.load_owned_group(group_id, owner_id).await?;
        let role = self.load_role_in_group(group_id, role_id).await?;
        if GroupRoleSource::from(role.source) == GroupRoleSource::DiscordRole {
            return Err(GroupError::RoleManagedByDiscord.into());
        }
        group_role_member::Entity::delete_by_id((role_id, user_id))
            .exec(&self.db)
            .await?;
        Ok(())
    }

    /// Half of the invariant: leaving the group leaves every role in it.
    pub(crate) async fn remove_user_from_all_roles_in_group(
        &self,
        txn: &DatabaseTransaction,
        group_id: i32,
        user_id: i64,
    ) -> Result<()> {
        let role_ids: Vec<i32> = group_role::Entity::find()
            .select_only()
            .column(group_role::Column::Id)
            .filter(group_role::Column::GroupId.eq(group_id))
            .into_tuple()
            .all(txn)
            .await?;
        if role_ids.is_empty() {
            return Ok(());
        }
        group_role_member::Entity::delete_many()
            .filter(group_role_member::Column::UserId.eq(user_id))
            .filter(group_role_member::Column::RoleId.is_in(role_ids))
            .exec(txn)
            .await?;
        Ok(())
    }

    pub async fn guild_has_synced_roles(&self, guild_id: i64) -> Result<bool> {
        Ok(!self.synced_roles_for_guild(guild_id).await?.is_empty())
    }

    pub async fn guilds_with_synced_roles(&self) -> Result<Vec<i64>> {
        let ids: Vec<i64> = user_group::Entity::find()
            .select_only()
            .column(user_group::Column::GuildId)
            .distinct()
            .join(
                sea_orm::JoinType::InnerJoin,
                user_group::Relation::GroupRole.def(),
            )
            .filter(user_group::Column::GuildId.is_not_null())
            .filter(user_group::Column::FrozenReason.is_null())
            .filter(group_role::Column::Source.eq(GroupRoleSource::DiscordRole as i16))
            .filter(group_role::Column::SyncState.eq(GroupRoleSyncState::Synced as i16))
            .into_tuple()
            .all(&self.db)
            .await?;
        Ok(ids)
    }

    pub async fn synced_roles_for_guild(&self, guild_id: i64) -> Result<Vec<SyncedRole>> {
        let rows: Vec<(i32, i32, i64)> = group_role::Entity::find()
            .select_only()
            .column(group_role::Column::Id)
            .column(group_role::Column::GroupId)
            .column(group_role::Column::DiscordRoleId)
            .join(
                sea_orm::JoinType::InnerJoin,
                group_role::Relation::UserGroup.def(),
            )
            .filter(user_group::Column::GuildId.eq(guild_id))
            .filter(user_group::Column::FrozenReason.is_null())
            .filter(group_role::Column::Source.eq(GroupRoleSource::DiscordRole as i16))
            .filter(group_role::Column::SyncState.eq(GroupRoleSyncState::Synced as i16))
            .filter(group_role::Column::DiscordRoleId.is_not_null())
            .into_tuple()
            .all(&self.db)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(role_id, group_id, discord_role_id)| SyncedRole {
                role_id,
                group_id,
                discord_role_id,
            })
            .collect())
    }

    pub async fn role_member_ids(&self, role_id: i32) -> Result<HashSet<i64>> {
        let ids: Vec<i64> = group_role_member::Entity::find()
            .select_only()
            .column(group_role_member::Column::UserId)
            .filter(group_role_member::Column::RoleId.eq(role_id))
            .into_tuple()
            .all(&self.db)
            .await?;
        Ok(ids.into_iter().collect())
    }

    /// Apply reconciliation output for one group in a single transaction.
    ///
    /// Adds upsert the user's row (refreshing the display name), join the
    /// role, and join the group as `Synced` if not already a member. Removes
    /// leave the role; a user who then holds no synced role in the group
    /// leaves the group too, unless they still hold a manual role in it, in
    /// which case their membership is handed over to `Manual` instead.
    /// Manual members are never removed. Every `plan.role_id` must belong to
    /// `group_id`, or the whole call is rejected with `RoleNotFound`.
    pub async fn apply_role_sync(
        &self,
        group_id: i32,
        plans: Vec<RoleSyncPlan>,
    ) -> Result<SyncSummary> {
        let mut summary = SyncSummary::default();
        let txn = self.db.begin().await?;

        let group_role_ids: HashSet<i32> = group_role::Entity::find()
            .select_only()
            .column(group_role::Column::Id)
            .filter(group_role::Column::GroupId.eq(group_id))
            .into_tuple()
            .all(&txn)
            .await?
            .into_iter()
            .collect();
        if plans.iter().any(|p| !group_role_ids.contains(&p.role_id)) {
            return Err(GroupError::RoleNotFound.into());
        }

        let mut touched_roles = Vec::with_capacity(plans.len());
        let mut removed_users: HashSet<i64> = HashSet::new();

        for plan in plans {
            touched_roles.push(plan.role_id);
            for (user_id, display_name) in plan.adds {
                discord_user::Entity::insert(discord_user::ActiveModel {
                    id: ActiveValue::Set(user_id),
                    username: ActiveValue::Set(display_name),
                })
                .on_conflict(
                    OnConflict::column(discord_user::Column::Id)
                        .update_column(discord_user::Column::Username)
                        .to_owned(),
                )
                .exec(&txn)
                .await?;
                ensure_group_member(&txn, group_id, user_id, GroupMemberSource::Synced).await?;
                insert_role_member(&txn, plan.role_id, user_id).await?;
                summary.added += 1;
            }
            if !plan.removes.is_empty() {
                let result = group_role_member::Entity::delete_many()
                    .filter(group_role_member::Column::RoleId.eq(plan.role_id))
                    .filter(group_role_member::Column::UserId.is_in(plan.removes.clone()))
                    .exec(&txn)
                    .await?;
                summary.removed += result.rows_affected as usize;
                removed_users.extend(plan.removes);
            }
        }

        if !removed_users.is_empty() {
            // Users still holding any synced role in this group stay.
            let still_synced: Vec<i64> = group_role_member::Entity::find()
                .select_only()
                .column(group_role_member::Column::UserId)
                .distinct()
                .join(
                    sea_orm::JoinType::InnerJoin,
                    group_role_member::Relation::GroupRole.def(),
                )
                .filter(group_role::Column::GroupId.eq(group_id))
                .filter(group_role::Column::Source.eq(GroupRoleSource::DiscordRole as i16))
                .filter(group_role_member::Column::UserId.is_in(removed_users.iter().copied()))
                .into_tuple()
                .all(&txn)
                .await?;
            let still_synced: HashSet<i64> = still_synced.into_iter().collect();

            // Of the rest, anyone still holding a manual role in this group
            // gets handed over to the owner's manual placement instead of
            // being evicted.
            let remaining: Vec<i64> = removed_users
                .into_iter()
                .filter(|id| !still_synced.contains(id))
                .collect();
            let still_manual: Vec<i64> = if remaining.is_empty() {
                Vec::new()
            } else {
                group_role_member::Entity::find()
                    .select_only()
                    .column(group_role_member::Column::UserId)
                    .distinct()
                    .join(
                        sea_orm::JoinType::InnerJoin,
                        group_role_member::Relation::GroupRole.def(),
                    )
                    .filter(group_role::Column::GroupId.eq(group_id))
                    .filter(group_role::Column::Source.eq(GroupRoleSource::Manual as i16))
                    .filter(group_role_member::Column::UserId.is_in(remaining.iter().copied()))
                    .into_tuple()
                    .all(&txn)
                    .await?
            };
            let still_manual: HashSet<i64> = still_manual.into_iter().collect();

            let handed_over: Vec<i64> = remaining
                .iter()
                .copied()
                .filter(|id| still_manual.contains(id))
                .collect();
            if !handed_over.is_empty() {
                let result = user_group_member::Entity::update_many()
                    .col_expr(
                        user_group_member::Column::Source,
                        sea_orm::sea_query::Expr::value(GroupMemberSource::Manual as i16),
                    )
                    .filter(user_group_member::Column::GroupId.eq(group_id))
                    .filter(user_group_member::Column::UserId.is_in(handed_over.iter().copied()))
                    .filter(user_group_member::Column::Source.eq(GroupMemberSource::Synced as i16))
                    .exec(&txn)
                    .await?;
                summary.handed_over = result.rows_affected as usize;
            }

            let candidates: Vec<i64> = remaining
                .into_iter()
                .filter(|id| !still_manual.contains(id))
                .collect();
            if !candidates.is_empty() {
                let result = user_group_member::Entity::delete_many()
                    .filter(user_group_member::Column::GroupId.eq(group_id))
                    .filter(user_group_member::Column::UserId.is_in(candidates))
                    .filter(user_group_member::Column::Source.eq(GroupMemberSource::Synced as i16))
                    .exec(&txn)
                    .await?;
                summary.left_group = result.rows_affected as usize;
            }
        }

        if !touched_roles.is_empty() {
            group_role::Entity::update_many()
                .col_expr(
                    group_role::Column::LastSyncedAt,
                    sea_orm::sea_query::Expr::value(chrono::Utc::now()),
                )
                .filter(group_role::Column::Id.is_in(touched_roles))
                .exec(&txn)
                .await?;
        }

        txn.commit().await?;
        Ok(summary)
    }

    /// Refresh the stored display name of users we already have a row for.
    ///
    /// Reconciliation is the only place that sees a member's current Discord
    /// name outside of login, so without this a member who renames themselves
    /// keeps a stale name everywhere they appear until they next log in.
    /// [`apply_role_sync`](Self::apply_role_sync) already refreshes the names
    /// it adds; this covers the members it did not have to touch, which in a
    /// steady-state guild is nearly all of them.
    ///
    /// Callers must pass ids that already exist in `discord_user`: the
    /// statement is an upsert, so an unknown id would mint a row for someone
    /// who has no reason to be in our database yet.
    pub async fn refresh_member_display_names(&self, users: &[(i64, String)]) -> Result<()> {
        // One statement per chunk rather than per user: a whole-server role on
        // a large guild is thousands of rows, and this runs on every cycle.
        for chunk in users.chunks(DISPLAY_NAME_REFRESH_CHUNK) {
            discord_user::Entity::insert_many(chunk.iter().map(|(id, username)| {
                discord_user::ActiveModel {
                    id: ActiveValue::Set(*id),
                    username: ActiveValue::Set(username.clone()),
                }
            }))
            .on_conflict(
                OnConflict::column(discord_user::Column::Id)
                    .update_column(discord_user::Column::Username)
                    .to_owned(),
            )
            .exec(&self.db)
            .await?;
        }
        Ok(())
    }

    /// The Discord role behind a synced role was deleted. Members are kept;
    /// the role just stops updating.
    pub async fn mark_role_orphaned(&self, guild_id: i64, discord_role_id: i64) -> Result<()> {
        let role_ids: Vec<i32> = group_role::Entity::find()
            .select_only()
            .column(group_role::Column::Id)
            .join(
                sea_orm::JoinType::InnerJoin,
                group_role::Relation::UserGroup.def(),
            )
            .filter(user_group::Column::GuildId.eq(guild_id))
            .filter(group_role::Column::DiscordRoleId.eq(discord_role_id))
            .into_tuple()
            .all(&self.db)
            .await?;
        if role_ids.is_empty() {
            return Ok(());
        }
        group_role::Entity::update_many()
            .col_expr(
                group_role::Column::SyncState,
                sea_orm::sea_query::Expr::value(GroupRoleSyncState::Orphaned as i16),
            )
            .filter(group_role::Column::Id.is_in(role_ids))
            .exec(&self.db)
            .await?;
        Ok(())
    }

    /// Everything the group page needs in one round trip: the group itself,
    /// its roles with member counts, and the group's own member count.
    pub async fn get_group_detail(
        &self,
        group_id: i32,
        user_id: i64,
    ) -> Result<(user_group::Model, Vec<GroupRoleReturn>, i64)> {
        let group = self.require_group_member(group_id, user_id).await?;
        let roles = self.get_group_roles(group_id, user_id).await?;
        let member_count = self
            .group_member_counts(&[group_id])
            .await?
            .remove(&group_id)
            .unwrap_or(0);
        Ok((group, roles, member_count))
    }

    /// Member counts for a batch of groups, e.g. for the group summary list.
    pub async fn group_member_counts(&self, group_ids: &[i32]) -> Result<HashMap<i32, i64>> {
        if group_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows: Vec<(i32, i64)> = user_group_member::Entity::find()
            .select_only()
            .column(user_group_member::Column::GroupId)
            .column_as(user_group_member::Column::UserId.count(), "member_count")
            .filter(user_group_member::Column::GroupId.is_in(group_ids.iter().copied()))
            .group_by(user_group_member::Column::GroupId)
            .into_tuple()
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().collect())
    }

    /// The bot left the guild. Unlink the group, keep every member, turn
    /// synced roles into orphaned manual roles, and record why. Returns the
    /// affected group id, or `None` if no group was linked to that guild.
    pub async fn freeze_group_for_guild(
        &self,
        guild_id: i64,
        reason: String,
    ) -> Result<Option<i32>> {
        let Some(group) = user_group::Entity::find()
            .filter(user_group::Column::GuildId.eq(guild_id))
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let txn = self.db.begin().await?;
        group_role::Entity::update_many()
            .col_expr(
                group_role::Column::Source,
                sea_orm::sea_query::Expr::value(GroupRoleSource::Manual as i16),
            )
            .col_expr(
                group_role::Column::SyncState,
                sea_orm::sea_query::Expr::value(GroupRoleSyncState::Orphaned as i16),
            )
            .filter(group_role::Column::GroupId.eq(group.id))
            .filter(group_role::Column::Source.eq(GroupRoleSource::DiscordRole as i16))
            .exec(&txn)
            .await?;
        // There is no more Discord link to sync from, so a synced member's
        // membership becomes the owner's own to manage (and to remove).
        user_group_member::Entity::update_many()
            .col_expr(
                user_group_member::Column::Source,
                sea_orm::sea_query::Expr::value(GroupMemberSource::Manual as i16),
            )
            .filter(user_group_member::Column::GroupId.eq(group.id))
            .filter(user_group_member::Column::Source.eq(GroupMemberSource::Synced as i16))
            .exec(&txn)
            .await?;
        let mut active: user_group::ActiveModel = group.clone().into();
        active.guild_id = ActiveValue::Set(None);
        active.source = ActiveValue::Set(GroupSource::Manual as i16);
        active.frozen_reason = ActiveValue::Set(Some(reason));
        active.update(&txn).await?;
        txn.commit().await?;
        Ok(Some(group.id))
    }
}

/// Insert the group membership if missing. An existing row is left alone so
/// a `Manual` member is never downgraded to `Synced` by a sync pass, and a
/// `Synced` member is only promoted by an explicit owner action elsewhere.
pub(crate) async fn ensure_group_member(
    txn: &DatabaseTransaction,
    group_id: i32,
    user_id: i64,
    source: GroupMemberSource,
) -> Result<()> {
    user_group_member::Entity::insert(user_group_member::ActiveModel {
        group_id: ActiveValue::Set(group_id),
        user_id: ActiveValue::Set(user_id),
        source: ActiveValue::Set(source as i16),
    })
    .on_conflict(
        OnConflict::columns([
            user_group_member::Column::GroupId,
            user_group_member::Column::UserId,
        ])
        // No-op update (a PK column set to itself) rather than `.do_nothing()`:
        // that helper is deprecated in this sea-orm version and denied by
        // `-D warnings`. This has the same effect as a silent no-op insert.
        .update_column(user_group_member::Column::UserId)
        .to_owned(),
    )
    .exec(txn)
    .await?;
    Ok(())
}

pub(crate) async fn insert_role_member(
    txn: &DatabaseTransaction,
    role_id: i32,
    user_id: i64,
) -> Result<()> {
    group_role_member::Entity::insert(group_role_member::ActiveModel {
        role_id: ActiveValue::Set(role_id),
        user_id: ActiveValue::Set(user_id),
    })
    .on_conflict(
        OnConflict::columns([
            group_role_member::Column::RoleId,
            group_role_member::Column::UserId,
        ])
        .update_column(group_role_member::Column::UserId)
        .to_owned(),
    )
    .exec(txn)
    .await?;
    Ok(())
}

/// Live-DB tests. Run with a disposable database:
///
/// ```bash
/// cargo test -p ultros-db group_roles -- --ignored --test-threads=1
/// ```
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    pub(crate) async fn test_db() -> UltrosDb {
        UltrosDb::connect().await.expect("connect to test DB")
    }

    /// Ids start from the current time so re-running against the same
    /// database never collides with a previous run's rows.
    static NEXT_ID: AtomicI64 = AtomicI64::new(0);

    fn next_id() -> i64 {
        let base = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as i64;
        base + NEXT_ID.fetch_add(1, Ordering::Relaxed)
    }

    /// A real `discord_user` row, because `user_group_member.user_id` is a
    /// foreign key and synthetic ids would fail the insert.
    pub(crate) async fn fresh_user(db: &UltrosDb, label: &str) -> discord_user::Model {
        let id = next_id();
        db.get_or_create_discord_user(id as u64, format!("{label}-{id}"))
            .await
            .unwrap()
    }

    pub(crate) async fn group_with_owner(
        db: &UltrosDb,
    ) -> (user_group::Model, discord_user::Model) {
        let owner = fresh_user(db, "owner").await;
        let group = db
            .create_group("Roles test group".to_string(), owner.id)
            .await
            .unwrap();
        (group, owner)
    }

    pub(crate) async fn is_group_member(db: &UltrosDb, group_id: i32, user_id: i64) -> bool {
        user_group_member::Entity::find_by_id((group_id, user_id))
            .one(&db.db)
            .await
            .unwrap()
            .is_some()
    }

    pub(crate) async fn member_source(db: &UltrosDb, group_id: i32, user_id: i64) -> Option<i16> {
        user_group_member::Entity::find_by_id((group_id, user_id))
            .one(&db.db)
            .await
            .unwrap()
            .map(|m| m.source)
    }

    pub(crate) async fn is_role_member(db: &UltrosDb, role_id: i32, user_id: i64) -> bool {
        group_role_member::Entity::find_by_id((role_id, user_id))
            .one(&db.db)
            .await
            .unwrap()
            .is_some()
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn add_member_creates_the_user_row_when_given_a_display_name() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let unknown_id = next_id();

        db.add_group_member(group.id, owner.id, unknown_id, Some("Newcomer".to_string()))
            .await
            .unwrap();

        assert!(is_group_member(&db, group.id, unknown_id).await);
        let user = discord_user::Entity::find_by_id(unknown_id)
            .one(&db.db)
            .await
            .unwrap()
            .expect("discord_user row was upserted");
        assert_eq!(user.username, "Newcomer");
        assert_eq!(
            member_source(&db, group.id, unknown_id).await,
            Some(GroupMemberSource::Manual as i16)
        );
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn add_member_without_display_name_still_fails_for_unknown_users() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let unknown_id = next_id();

        let result = db
            .add_group_member(group.id, owner.id, unknown_id, None)
            .await;

        assert!(
            result.is_err(),
            "no discord_user row and nothing to create one from"
        );
        assert!(!is_group_member(&db, group.id, unknown_id).await);
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn removing_a_group_member_also_removes_their_role_memberships() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let member = fresh_user(&db, "member").await;
        let role = db
            .create_group_role(group.id, owner.id, "Crafters".to_string())
            .await
            .unwrap();
        db.add_group_role_member(group.id, owner.id, role.id, member.id)
            .await
            .unwrap();
        assert!(is_role_member(&db, role.id, member.id).await);

        db.remove_group_member(group.id, owner.id, member.id)
            .await
            .unwrap();

        assert!(!is_group_member(&db, group.id, member.id).await);
        assert!(!is_role_member(&db, role.id, member.id).await);
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn synced_members_cannot_be_removed_by_owner_or_themselves() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let member = fresh_user(&db, "synced").await;
        user_group_member::ActiveModel {
            group_id: ActiveValue::Set(group.id),
            user_id: ActiveValue::Set(member.id),
            source: ActiveValue::Set(GroupMemberSource::Synced as i16),
        }
        .insert(&db.db)
        .await
        .unwrap();

        let by_owner = db.remove_group_member(group.id, owner.id, member.id).await;
        let by_self = db.remove_group_member(group.id, member.id, member.id).await;

        assert!(matches!(
            by_owner.unwrap_err().downcast_ref::<GroupError>(),
            Some(GroupError::ManagedByDiscord)
        ));
        assert!(matches!(
            by_self.unwrap_err().downcast_ref::<GroupError>(),
            Some(GroupError::ManagedByDiscord)
        ));
        assert!(is_group_member(&db, group.id, member.id).await);
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn get_group_members_reports_role_ids_and_source() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let member = fresh_user(&db, "member").await;
        let role = db
            .create_group_role(group.id, owner.id, "Officers".to_string())
            .await
            .unwrap();
        db.add_group_role_member(group.id, owner.id, role.id, member.id)
            .await
            .unwrap();

        let members = db.get_group_members(group.id, owner.id).await.unwrap();
        let row = members
            .iter()
            .find(|m| m.0.user_id == member.id)
            .expect("member is listed");
        assert_eq!(row.2, vec![role.id]);
        assert_eq!(row.0.source, GroupMemberSource::Manual as i16);
        let owner_row = members.iter().find(|m| m.0.user_id == owner.id).unwrap();
        assert!(owner_row.2.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn only_the_owner_creates_roles() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let member = fresh_user(&db, "member").await;
        db.add_group_member(group.id, owner.id, member.id, None)
            .await
            .unwrap();

        let by_member = db
            .create_group_role(group.id, member.id, "Nope".to_string())
            .await;
        assert!(matches!(
            by_member.unwrap_err().downcast_ref::<GroupError>(),
            Some(GroupError::Forbidden(_))
        ));

        let role = db
            .create_group_role(group.id, owner.id, "Officers".to_string())
            .await
            .unwrap();
        assert_eq!(role.source, GroupRoleSource::Manual as i16);
        assert_eq!(role.sync_state, GroupRoleSyncState::Synced as i16);
        assert_eq!(role.discord_role_id, None);
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn blank_role_names_are_rejected() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let result = db
            .create_group_role(group.id, owner.id, "   ".to_string())
            .await;
        assert!(matches!(
            result.unwrap_err().downcast_ref::<GroupError>(),
            Some(GroupError::BadRequest(_))
        ));
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn adding_to_a_role_adds_to_the_group_as_manual() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let member = fresh_user(&db, "member").await;
        let role = db
            .create_group_role(group.id, owner.id, "Crafters".to_string())
            .await
            .unwrap();
        assert!(!is_group_member(&db, group.id, member.id).await);

        db.add_group_role_member(group.id, owner.id, role.id, member.id)
            .await
            .unwrap();

        assert!(is_role_member(&db, role.id, member.id).await);
        assert_eq!(
            member_source(&db, group.id, member.id).await,
            Some(GroupMemberSource::Manual as i16)
        );
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn removing_from_a_role_keeps_group_membership() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let member = fresh_user(&db, "member").await;
        let role = db
            .create_group_role(group.id, owner.id, "Crafters".to_string())
            .await
            .unwrap();
        db.add_group_role_member(group.id, owner.id, role.id, member.id)
            .await
            .unwrap();

        db.remove_group_role_member(group.id, owner.id, role.id, member.id)
            .await
            .unwrap();

        assert!(!is_role_member(&db, role.id, member.id).await);
        assert!(is_group_member(&db, group.id, member.id).await);
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn deleting_a_role_keeps_group_members() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let member = fresh_user(&db, "member").await;
        let role = db
            .create_group_role(group.id, owner.id, "Temp".to_string())
            .await
            .unwrap();
        db.add_group_role_member(group.id, owner.id, role.id, member.id)
            .await
            .unwrap();

        db.delete_group_role(group.id, owner.id, role.id)
            .await
            .unwrap();

        assert!(is_group_member(&db, group.id, member.id).await);
        assert!(
            group_role::Entity::find_by_id(role.id)
                .one(&db.db)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn synced_roles_refuse_manual_membership_changes() {
        let db = test_db().await;
        let owner = fresh_user(&db, "owner").await;
        let guild_id = next_id();
        let group = db
            .create_group_from_guild("Guild group".to_string(), owner.id, guild_id, None)
            .await
            .unwrap();
        let member = fresh_user(&db, "member").await;
        let role = db
            .import_discord_role(group.id, owner.id, next_id(), "Raiders".to_string(), 5)
            .await
            .unwrap();
        assert_eq!(role.source, GroupRoleSource::DiscordRole as i16);

        let add = db
            .add_group_role_member(group.id, owner.id, role.id, member.id)
            .await;
        assert!(matches!(
            add.unwrap_err().downcast_ref::<GroupError>(),
            Some(GroupError::RoleManagedByDiscord)
        ));
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn importing_flips_group_source_and_deleting_the_last_import_flips_it_back() {
        let db = test_db().await;
        let owner = fresh_user(&db, "owner").await;
        let group = db
            .create_group_from_guild("Guild group".to_string(), owner.id, next_id(), None)
            .await
            .unwrap();
        assert_eq!(group.source, GroupSource::DiscordGuild as i16);

        let role = db
            .import_discord_role(group.id, owner.id, next_id(), "Raiders".to_string(), 1)
            .await
            .unwrap();
        let reloaded = user_group::Entity::find_by_id(group.id)
            .one(&db.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.source, GroupSource::DiscordGuildMirrored as i16);

        db.delete_group_role(group.id, owner.id, role.id)
            .await
            .unwrap();
        let reloaded = user_group::Entity::find_by_id(group.id)
            .one(&db.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.source, GroupSource::DiscordGuild as i16);
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn importing_the_same_discord_role_twice_is_a_bad_request() {
        let db = test_db().await;
        let owner = fresh_user(&db, "owner").await;
        let group = db
            .create_group_from_guild("Guild group".to_string(), owner.id, next_id(), None)
            .await
            .unwrap();
        let discord_role_id = next_id();
        db.import_discord_role(group.id, owner.id, discord_role_id, "A".to_string(), 1)
            .await
            .unwrap();

        let again = db
            .import_discord_role(group.id, owner.id, discord_role_id, "A".to_string(), 1)
            .await;
        assert!(matches!(
            again.unwrap_err().downcast_ref::<GroupError>(),
            Some(GroupError::BadRequest(_))
        ));
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn manual_groups_cannot_import_discord_roles() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let result = db
            .import_discord_role(group.id, owner.id, next_id(), "A".to_string(), 1)
            .await;
        assert!(matches!(
            result.unwrap_err().downcast_ref::<GroupError>(),
            Some(GroupError::BadRequest(_))
        ));
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn get_group_roles_counts_members_and_is_member_gated() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let member = fresh_user(&db, "member").await;
        let outsider = fresh_user(&db, "outsider").await;
        let role = db
            .create_group_role(group.id, owner.id, "Crafters".to_string())
            .await
            .unwrap();
        db.add_group_role_member(group.id, owner.id, role.id, member.id)
            .await
            .unwrap();

        let roles = db.get_group_roles(group.id, member.id).await.unwrap();
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].0.id, role.id);
        assert_eq!(roles[0].1, 1, "one member in the role");

        let denied = db.get_group_roles(group.id, outsider.id).await;
        assert!(matches!(
            denied.unwrap_err().downcast_ref::<GroupError>(),
            Some(GroupError::Forbidden(_))
        ));
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn rename_trims_and_rejects_blank() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let role = db
            .create_group_role(group.id, owner.id, "Old".to_string())
            .await
            .unwrap();

        let renamed = db
            .rename_group_role(group.id, owner.id, role.id, "  New  ".to_string())
            .await
            .unwrap();
        assert_eq!(renamed.name, "New");

        let blank = db
            .rename_group_role(group.id, owner.id, role.id, "".to_string())
            .await;
        assert!(blank.is_err());
    }

    async fn guild_group_with_synced_role(
        db: &UltrosDb,
    ) -> (
        user_group::Model,
        discord_user::Model,
        group_role::Model,
        i64,
    ) {
        let owner = fresh_user(db, "owner").await;
        let guild_id = next_id();
        let group = db
            .create_group_from_guild("Sync group".to_string(), owner.id, guild_id, None)
            .await
            .unwrap();
        let role = db
            .import_discord_role(group.id, owner.id, next_id(), "Raiders".to_string(), 1)
            .await
            .unwrap();
        (group, owner, role, guild_id)
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn apply_role_sync_adds_unknown_users_as_synced_members() {
        let db = test_db().await;
        let (group, _owner, role, _guild) = guild_group_with_synced_role(&db).await;
        let newcomer = next_id();

        let summary = db
            .apply_role_sync(
                group.id,
                vec![RoleSyncPlan {
                    role_id: role.id,
                    adds: vec![(newcomer, "Raider One".to_string())],
                    removes: vec![],
                }],
            )
            .await
            .unwrap();

        assert_eq!(summary.added, 1);
        assert!(is_role_member(&db, role.id, newcomer).await);
        assert_eq!(
            member_source(&db, group.id, newcomer).await,
            Some(GroupMemberSource::Synced as i16)
        );
        let user = discord_user::Entity::find_by_id(newcomer)
            .one(&db.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(user.username, "Raider One");
        let stamped = group_role::Entity::find_by_id(role.id)
            .one(&db.db)
            .await
            .unwrap()
            .unwrap();
        assert!(stamped.last_synced_at.is_some());
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn apply_role_sync_refreshes_display_names() {
        let db = test_db().await;
        let (group, _owner, role, _guild) = guild_group_with_synced_role(&db).await;
        let user = fresh_user(&db, "stale").await;

        db.apply_role_sync(
            group.id,
            vec![RoleSyncPlan {
                role_id: role.id,
                adds: vec![(user.id, "Fresh Name".to_string())],
                removes: vec![],
            }],
        )
        .await
        .unwrap();

        let reloaded = discord_user::Entity::find_by_id(user.id)
            .one(&db.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.username, "Fresh Name");
    }

    /// The members a sync pass does *not* have to touch still need their names
    /// kept current, which is the only thing that fixes a stale username for
    /// someone who has not logged in since they renamed themselves.
    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn bulk_refresh_updates_names_without_touching_membership() {
        let db = test_db().await;
        let (group, _owner, role, _guild) = guild_group_with_synced_role(&db).await;
        let first = fresh_user(&db, "stale-one").await;
        let second = fresh_user(&db, "stale-two").await;
        db.apply_role_sync(
            group.id,
            vec![RoleSyncPlan {
                role_id: role.id,
                adds: vec![
                    (first.id, first.username.clone()),
                    (second.id, second.username.clone()),
                ],
                removes: vec![],
            }],
        )
        .await
        .unwrap();

        db.refresh_member_display_names(&[
            (first.id, "Renamed One".to_string()),
            (second.id, "Renamed Two".to_string()),
        ])
        .await
        .unwrap();

        for (id, expected) in [(first.id, "Renamed One"), (second.id, "Renamed Two")] {
            let reloaded = discord_user::Entity::find_by_id(id)
                .one(&db.db)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(reloaded.username, expected);
        }
        // Names are all it changes: nobody joins or leaves.
        assert_eq!(db.role_member_ids(role.id).await.unwrap().len(), 2);
        assert_eq!(
            db.group_member_counts(&[group.id])
                .await
                .unwrap()
                .remove(&group.id),
            Some(3),
            "the owner plus the two synced members"
        );
    }

    /// An empty refresh must not issue a statement at all — `insert_many` with
    /// no rows is not a valid query.
    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn refreshing_nothing_is_a_no_op() {
        let db = test_db().await;
        db.refresh_member_display_names(&[]).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn apply_role_sync_rejects_a_role_from_another_group() {
        let db = test_db().await;
        let (group_a, _owner_a, _role_a, _guild_a) = guild_group_with_synced_role(&db).await;
        let (_group_b, _owner_b, role_b, _guild_b) = guild_group_with_synced_role(&db).await;
        let user = next_id();

        let result = db
            .apply_role_sync(
                group_a.id,
                vec![RoleSyncPlan {
                    role_id: role_b.id,
                    adds: vec![(user, "X".to_string())],
                    removes: vec![],
                }],
            )
            .await;

        assert!(matches!(
            result.unwrap_err().downcast_ref::<GroupError>(),
            Some(GroupError::RoleNotFound)
        ));
        assert!(!is_role_member(&db, role_b.id, user).await);
        assert!(!is_group_member(&db, group_a.id, user).await);
        assert!(!is_group_member(&db, _group_b.id, user).await);
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn sync_removal_hands_over_members_who_hold_a_manual_role() {
        let db = test_db().await;
        let (group, owner, synced_role, _guild) = guild_group_with_synced_role(&db).await;
        let u = next_id();
        db.apply_role_sync(
            group.id,
            vec![RoleSyncPlan {
                role_id: synced_role.id,
                adds: vec![(u, "U".to_string())],
                removes: vec![],
            }],
        )
        .await
        .unwrap();
        assert_eq!(
            member_source(&db, group.id, u).await,
            Some(GroupMemberSource::Synced as i16)
        );

        let manual_role = db
            .create_group_role(group.id, owner.id, "Manual role".to_string())
            .await
            .unwrap();
        db.add_group_role_member(group.id, owner.id, manual_role.id, u)
            .await
            .unwrap();
        assert_eq!(
            member_source(&db, group.id, u).await,
            Some(GroupMemberSource::Synced as i16),
            "the invariant helper never downgrades or upgrades an existing member"
        );

        let summary = db
            .apply_role_sync(
                group.id,
                vec![RoleSyncPlan {
                    role_id: synced_role.id,
                    adds: vec![],
                    removes: vec![u],
                }],
            )
            .await
            .unwrap();

        assert_eq!(summary.handed_over, 1);
        assert_eq!(summary.left_group, 0);
        assert!(is_group_member(&db, group.id, u).await);
        assert!(is_role_member(&db, manual_role.id, u).await);
        assert!(!is_role_member(&db, synced_role.id, u).await);
        assert_eq!(
            member_source(&db, group.id, u).await,
            Some(GroupMemberSource::Manual as i16)
        );

        db.remove_group_member(group.id, owner.id, u).await.unwrap();
        assert!(!is_role_member(&db, manual_role.id, u).await);
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn re_adding_a_synced_member_by_hand_promotes_them_to_manual() {
        let db = test_db().await;
        let (group, owner, role, _guild) = guild_group_with_synced_role(&db).await;
        let u = next_id();
        db.apply_role_sync(
            group.id,
            vec![RoleSyncPlan {
                role_id: role.id,
                adds: vec![(u, "U".to_string())],
                removes: vec![],
            }],
        )
        .await
        .unwrap();
        assert_eq!(
            member_source(&db, group.id, u).await,
            Some(GroupMemberSource::Synced as i16)
        );

        db.add_group_member(group.id, owner.id, u, None)
            .await
            .unwrap();

        assert_eq!(
            member_source(&db, group.id, u).await,
            Some(GroupMemberSource::Manual as i16)
        );
        assert!(is_group_member(&db, group.id, u).await);
        assert!(db.remove_group_member(group.id, owner.id, u).await.is_ok());
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn sync_removal_drops_synced_members_but_keeps_manual_ones() {
        let db = test_db().await;
        let (group, owner, role, _guild) = guild_group_with_synced_role(&db).await;
        let synced = next_id();
        let manual = fresh_user(&db, "manual").await;
        db.add_group_member(group.id, owner.id, manual.id, None)
            .await
            .unwrap();
        db.apply_role_sync(
            group.id,
            vec![RoleSyncPlan {
                role_id: role.id,
                adds: vec![
                    (synced, "Synced".to_string()),
                    (manual.id, "Manual".to_string()),
                ],
                removes: vec![],
            }],
        )
        .await
        .unwrap();
        assert!(is_role_member(&db, role.id, manual.id).await);
        assert_eq!(
            member_source(&db, group.id, manual.id).await,
            Some(GroupMemberSource::Manual as i16),
            "sync never downgrades a manual member"
        );

        let summary = db
            .apply_role_sync(
                group.id,
                vec![RoleSyncPlan {
                    role_id: role.id,
                    adds: vec![],
                    removes: vec![synced, manual.id],
                }],
            )
            .await
            .unwrap();

        assert_eq!(summary.removed, 2);
        assert_eq!(summary.left_group, 1);
        assert!(!is_role_member(&db, role.id, synced).await);
        assert!(!is_group_member(&db, group.id, synced).await);
        assert!(!is_role_member(&db, role.id, manual.id).await);
        assert!(is_group_member(&db, group.id, manual.id).await);
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn a_synced_member_in_two_roles_stays_until_the_last_role_goes() {
        let db = test_db().await;
        let (group, owner, role_a, _guild) = guild_group_with_synced_role(&db).await;
        let role_b = db
            .import_discord_role(group.id, owner.id, next_id(), "Healers".to_string(), 2)
            .await
            .unwrap();
        let user = next_id();
        db.apply_role_sync(
            group.id,
            vec![
                RoleSyncPlan {
                    role_id: role_a.id,
                    adds: vec![(user, "U".to_string())],
                    removes: vec![],
                },
                RoleSyncPlan {
                    role_id: role_b.id,
                    adds: vec![(user, "U".to_string())],
                    removes: vec![],
                },
            ],
        )
        .await
        .unwrap();

        db.apply_role_sync(
            group.id,
            vec![RoleSyncPlan {
                role_id: role_a.id,
                adds: vec![],
                removes: vec![user],
            }],
        )
        .await
        .unwrap();
        assert!(
            is_group_member(&db, group.id, user).await,
            "still holds role_b"
        );

        db.apply_role_sync(
            group.id,
            vec![RoleSyncPlan {
                role_id: role_b.id,
                adds: vec![],
                removes: vec![user],
            }],
        )
        .await
        .unwrap();
        assert!(!is_group_member(&db, group.id, user).await);
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn guild_lookups_see_only_synced_roles() {
        let db = test_db().await;
        let (group, owner, role, guild_id) = guild_group_with_synced_role(&db).await;
        db.create_group_role(group.id, owner.id, "Manual".to_string())
            .await
            .unwrap();

        assert!(db.guild_has_synced_roles(guild_id).await.unwrap());
        assert!(!db.guild_has_synced_roles(next_id()).await.unwrap());
        let synced = db.synced_roles_for_guild(guild_id).await.unwrap();
        assert_eq!(synced.len(), 1);
        assert_eq!(synced[0].role_id, role.id);
        assert_eq!(synced[0].discord_role_id, role.discord_role_id.unwrap());
        assert!(
            db.guilds_with_synced_roles()
                .await
                .unwrap()
                .contains(&guild_id)
        );
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn orphaning_a_role_keeps_its_members() {
        let db = test_db().await;
        let (group, _owner, role, guild_id) = guild_group_with_synced_role(&db).await;
        let user = next_id();
        db.apply_role_sync(
            group.id,
            vec![RoleSyncPlan {
                role_id: role.id,
                adds: vec![(user, "U".to_string())],
                removes: vec![],
            }],
        )
        .await
        .unwrap();

        db.mark_role_orphaned(guild_id, role.discord_role_id.unwrap())
            .await
            .unwrap();

        let reloaded = group_role::Entity::find_by_id(role.id)
            .one(&db.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.sync_state, GroupRoleSyncState::Orphaned as i16);
        assert!(is_role_member(&db, role.id, user).await);
        assert!(
            db.synced_roles_for_guild(guild_id)
                .await
                .unwrap()
                .is_empty(),
            "orphaned roles are not reconciled again"
        );
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn freezing_unlinks_the_guild_and_keeps_everyone() {
        let db = test_db().await;
        let (group, owner, role, guild_id) = guild_group_with_synced_role(&db).await;
        let user = next_id();
        db.apply_role_sync(
            group.id,
            vec![RoleSyncPlan {
                role_id: role.id,
                adds: vec![(user, "U".to_string())],
                removes: vec![],
            }],
        )
        .await
        .unwrap();

        let frozen = db
            .freeze_group_for_guild(
                guild_id,
                "The Ultros bot was removed from the server".to_string(),
            )
            .await
            .unwrap();
        assert_eq!(frozen, Some(group.id));

        let reloaded = user_group::Entity::find_by_id(group.id)
            .one(&db.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reloaded.guild_id, None);
        assert_eq!(reloaded.source, GroupSource::Manual as i16);
        assert!(reloaded.frozen_reason.is_some());
        assert!(is_group_member(&db, group.id, user).await);
        assert!(is_group_member(&db, group.id, owner.id).await);
        let role = group_role::Entity::find_by_id(role.id)
            .one(&db.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(role.source, GroupRoleSource::Manual as i16);
        assert_eq!(role.sync_state, GroupRoleSyncState::Orphaned as i16);
        assert_eq!(
            member_source(&db, group.id, user).await,
            Some(GroupMemberSource::Manual as i16),
            "a frozen group has no Discord link left to sync from"
        );
        db.remove_group_member(group.id, owner.id, user)
            .await
            .unwrap();
        assert!(!is_group_member(&db, group.id, user).await);

        // The guild slot is free again: a new group can be linked to it.
        db.create_group_from_guild("Relinked".to_string(), owner.id, guild_id, None)
            .await
            .unwrap();
        assert_eq!(
            db.freeze_group_for_guild(next_id(), "x".to_string())
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn group_detail_bundles_roles_and_member_count() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let member = fresh_user(&db, "member").await;
        let role = db
            .create_group_role(group.id, owner.id, "Officers".to_string())
            .await
            .unwrap();
        db.add_group_role_member(group.id, owner.id, role.id, member.id)
            .await
            .unwrap();

        let (detail_group, roles, member_count) =
            db.get_group_detail(group.id, member.id).await.unwrap();
        assert_eq!(detail_group.id, group.id);
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].1, 1);
        assert_eq!(member_count, 2, "owner plus one member");

        let counts = db.group_member_counts(&[group.id]).await.unwrap();
        assert_eq!(counts.get(&group.id), Some(&2));
        assert!(db.group_member_counts(&[]).await.unwrap().is_empty());
    }

    /// A `%` typed into the member-search box has to match a literal `%`. Left
    /// unescaped it is a wildcard, so a single character would return the
    /// whole `discord_user` table to whoever owns any group.
    #[test]
    fn like_wildcards_typed_by_a_user_are_escaped() {
        assert_eq!(escape_like("bob"), "bob");
        assert_eq!(escape_like("50%"), "50\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        // The backslash is escaped first, or escaping the wildcards would
        // themselves be undone by a backslash the user typed.
        assert_eq!(escape_like("a\\%"), "a\\\\\\%");
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn member_search_is_owner_only_and_matches_a_case_insensitive_prefix() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let stranger = fresh_user(&db, "stranger").await;
        let id = next_id();
        let target = db
            .get_or_create_discord_user(id as u64, format!("Zaraband-{id}"))
            .await
            .unwrap();

        let hits = db
            .search_group_member_candidates(group.id, owner.id, "zara")
            .await
            .unwrap();
        assert!(
            hits.iter().any(|u| u.id == target.id),
            "a lowercase prefix must match a capitalised username"
        );

        assert!(
            db.search_group_member_candidates(group.id, owner.id, "   ")
                .await
                .unwrap()
                .is_empty(),
            "a blank query is an empty result, not every user"
        );

        assert!(
            db.search_group_member_candidates(group.id, stranger.id, "zara")
                .await
                .is_err(),
            "only the group owner may search for candidates"
        );
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn member_search_never_returns_more_than_the_cap() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let prefix = format!("capped{}", next_id());
        for index in 0..(MEMBER_SEARCH_LIMIT + 5) {
            let id = next_id();
            db.get_or_create_discord_user(id as u64, format!("{prefix}-{index}"))
                .await
                .unwrap();
        }

        let hits = db
            .search_group_member_candidates(group.id, owner.id, &prefix)
            .await
            .unwrap();
        assert_eq!(hits.len() as u64, MEMBER_SEARCH_LIMIT);
    }

    /// Drives the "not on Ultros yet" hint on Discord-backed search, so a
    /// missing id has to come back missing rather than defaulting to present.
    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn discord_users_present_reports_only_ids_with_a_row() {
        let db = test_db().await;
        let known = fresh_user(&db, "known").await;
        let unknown = next_id();

        let present = db
            .discord_users_present(&[known.id, unknown])
            .await
            .unwrap();
        assert!(present.contains(&known.id));
        assert!(!present.contains(&unknown));
        assert!(db.discord_users_present(&[]).await.unwrap().is_empty());
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn get_owned_group_refuses_a_non_owner() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let member = fresh_user(&db, "member").await;
        db.add_group_member(group.id, owner.id, member.id, None)
            .await
            .unwrap();

        assert_eq!(
            db.get_owned_group(group.id, owner.id).await.unwrap().id,
            group.id
        );
        // Membership is not ownership: the Discord-facing endpoints hang off
        // this check, and a member must not be able to reach the guild.
        assert!(db.get_owned_group(group.id, member.id).await.is_err());
    }
}
