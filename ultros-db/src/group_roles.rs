//! Roles inside a group, the group-membership invariant, and the DB side of
//! Discord membership sync.
//!
//! Invariant enforced here and in `lists.rs`'s group functions: every
//! `group_role_member` row has a matching `user_group_member` row. Adding to a
//! role adds to the group; removing from the group removes from every role.

use crate::{
    UltrosDb,
    common_type_conversions::GroupRoleReturn,
    entity::{group_role, group_role_member, user_group, user_group_member},
};
use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseTransaction, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, RelationTrait, TransactionTrait, sea_query::OnConflict,
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
        .await?;
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
    use crate::entity::discord_user;
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
}
