# Groups Stage 1: Schema and DB Layer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the roles data model, the group-membership invariant, the sync-facing DB primitives, and role-targeted list permissions, so stages 2 to 4 have a stable DB layer to build on.

**Architecture:** One migration adds three tables (`group_role`, `group_role_member`, `list_shared_role`) and two columns (`user_group.frozen_reason`, `user_group_member.source`). New DB code lives in a new `ultros-db/src/group_roles.rs` so `lists.rs` does not grow further; the existing group functions in `lists.rs` are modified in place where the invariant requires it. API types gain the role enums and structs that later stages serialise.

**Tech Stack:** Rust, sea-orm 1.x / sea-orm-migration, Postgres, `ultros-api-types` shared structs, `thiserror`.

**Spec:** `docs/superpowers/specs/2026-09-07-groups-roles-and-discord-sync-design.md`

## Global Constraints

- Run `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log` before every commit that touches Rust. Do not commit on a non-zero exit.
- On Windows prepend `/c/Strawberry/perl/bin:/c/Strawberry/c/bin:` to `PATH` in Git Bash before any cargo command that links `ultros` (OpenSSL vendored build).
- Live-DB tests in `ultros-db` are `#[ignore]`d by convention and run with `DATABASE_URL` pointed at a disposable database: `cargo test -p ultros-db <module> -- --ignored --test-threads=1`. Unit tests that need no DB are not ignored.
- Migrations are named `m20260907_00000N_<snake>` and registered in `migration/src/lib.rs` in date order.
- `user_group.source` encoding: `0 = Manual`, `1 = DiscordGuild`, `2 = DiscordGuildMirrored`.
- `user_group_member.source` encoding: `0 = Manual`, `1 = Synced`.
- `group_role.source` encoding: `0 = Manual`, `1 = DiscordRole`. `group_role.sync_state`: `0 = Synced`, `1 = Orphaned`.
- Invariant: every `group_role_member` row has a matching `user_group_member` row. Enforced in the DB layer, never left to callers.
- A `Synced` group member cannot be removed through `remove_group_member` by anyone; they leave by leaving the Discord role. (Otherwise reconciliation would re-add them within hours, which is worse UX than a clear refusal.)
- No user-facing strings are introduced in this stage; every message here is an API error string, which the repo keeps in English.

---

### Task 1: Migration

**Files:**
- Create: `migration/src/m20260907_000001_group_roles.rs`
- Modify: `migration/src/lib.rs` (add `mod` line after line 41 and `Box::new(...)` after the `m20260905_000001_list_item_acquired_integer` entry)

**Interfaces:**
- Produces: tables `group_role`, `group_role_member`, `list_shared_role`; columns `user_group.frozen_reason TEXT NULL`, `user_group_member.source SMALLINT NOT NULL DEFAULT 0`; indexes `idx_group_role_group_id`, `idx_group_role_group_discord_role` (unique), `idx_group_role_member_user_id`, `idx_list_shared_role_role_id`.

- [ ] **Step 1: Write the migration**

```rust
use sea_orm_migration::prelude::*;

/// Roles inside a group, plus the tables that hang off them.
///
/// `group_role.discord_role_id` is nullable because manual roles have no
/// Discord counterpart. The unique index on `(group_id, discord_role_id)`
/// relies on Postgres treating NULLs as distinct, so any number of manual
/// roles coexist while a Discord role can be imported into a group once.
///
/// `list_shared_role` is a separate table rather than a nullable column on
/// `list_shared_group`, whose primary key is `(list_id, group_id)`.
///
/// `user_group_member.source` records who added a member (owner/invite vs.
/// Discord sync) so reconciliation only ever removes the members it added.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(GroupRole::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(GroupRole::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(GroupRole::GroupId).integer().not_null())
                    .col(ColumnDef::new(GroupRole::Name).text().not_null())
                    .col(ColumnDef::new(GroupRole::DiscordRoleId).big_integer().null())
                    .col(
                        ColumnDef::new(GroupRole::Source)
                            .small_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(GroupRole::SyncState)
                            .small_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(GroupRole::LastSyncedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(GroupRole::Position)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(GroupRole::Table, GroupRole::GroupId)
                            .to(UserGroup::Table, UserGroup::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_group_role_group_id")
                    .table(GroupRole::Table)
                    .col(GroupRole::GroupId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_group_role_group_discord_role")
                    .table(GroupRole::Table)
                    .col(GroupRole::GroupId)
                    .col(GroupRole::DiscordRoleId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(GroupRoleMember::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(GroupRoleMember::RoleId).integer().not_null())
                    .col(
                        ColumnDef::new(GroupRoleMember::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(GroupRoleMember::RoleId)
                            .col(GroupRoleMember::UserId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(GroupRoleMember::Table, GroupRoleMember::RoleId)
                            .to(GroupRole::Table, GroupRole::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(GroupRoleMember::Table, GroupRoleMember::UserId)
                            .to(DiscordUser::Table, DiscordUser::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // "Which roles does this user hold" is asked per member row when
        // rendering a group, and by `get_permission` on every list read.
        manager
            .create_index(
                Index::create()
                    .name("idx_group_role_member_user_id")
                    .table(GroupRoleMember::Table)
                    .col(GroupRoleMember::UserId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ListSharedRole::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ListSharedRole::ListId).integer().not_null())
                    .col(ColumnDef::new(ListSharedRole::RoleId).integer().not_null())
                    .col(
                        ColumnDef::new(ListSharedRole::Permission)
                            .small_integer()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(ListSharedRole::ListId)
                            .col(ListSharedRole::RoleId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ListSharedRole::Table, ListSharedRole::ListId)
                            .to(List::Table, List::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ListSharedRole::Table, ListSharedRole::RoleId)
                            .to(GroupRole::Table, GroupRole::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_list_shared_role_role_id")
                    .table(ListSharedRole::Table)
                    .col(ListSharedRole::RoleId)
                    .to_owned(),
            )
            .await?;

        // Set when the bot was removed from the group's guild. Non-null means
        // "frozen": unlinked from Discord, members kept, banner shown.
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(UserGroup::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(UserGroup::FrozenReason).text().null(),
                    )
                    .to_owned(),
            )
            .await?;

        // 0 = Manual, 1 = Synced. Every existing row was added by a person.
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(UserGroupMember::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(UserGroupMember::Source)
                            .small_integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(UserGroupMember::Table)
                    .drop_column(UserGroupMember::Source)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(UserGroup::Table)
                    .drop_column(UserGroup::FrozenReason)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(ListSharedRole::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(GroupRoleMember::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(GroupRole::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum GroupRole {
    Table,
    Id,
    GroupId,
    Name,
    DiscordRoleId,
    Source,
    SyncState,
    LastSyncedAt,
    Position,
}

#[derive(DeriveIden)]
enum GroupRoleMember {
    Table,
    RoleId,
    UserId,
}

#[derive(DeriveIden)]
enum ListSharedRole {
    Table,
    ListId,
    RoleId,
    Permission,
}

#[derive(DeriveIden)]
enum UserGroup {
    Table,
    Id,
    FrozenReason,
}

#[derive(DeriveIden)]
enum UserGroupMember {
    Table,
    Source,
}

#[derive(DeriveIden)]
enum DiscordUser {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum List {
    Table,
    Id,
}
```

- [ ] **Step 2: Register it**

In `migration/src/lib.rs`, after `mod m20260905_000001_list_item_acquired_integer;` add:

```rust
mod m20260907_000001_group_roles;
```

and after `Box::new(m20260905_000001_list_item_acquired_integer::Migration),` add:

```rust
            Box::new(m20260907_000001_group_roles::Migration),
```

- [ ] **Step 3: Compile**

Run: `cargo check -p migration`
Expected: no errors.

- [ ] **Step 4: Apply against a disposable DB and verify**

Run (with `DATABASE_URL` set to a scratch Postgres):

```bash
cargo run -p migration -- up
psql "$DATABASE_URL" -c '\d group_role' -c '\d group_role_member' -c '\d list_shared_role' -c '\d user_group_member'
cargo run -p migration -- down -n 1
cargo run -p migration -- up
```

Expected: the three tables exist with the indexes named above; `user_group_member` shows `source smallint not null default 0`; down then up succeeds without error.

- [ ] **Step 5: Commit**

```bash
git add migration/src/m20260907_000001_group_roles.rs migration/src/lib.rs
git commit -m "Add group_role, group_role_member, list_shared_role tables

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: API types

**Files:**
- Modify: `ultros-api-types/src/user/group.rs`

**Interfaces:**
- Produces:
  - `GroupSource::DiscordGuildMirrored = 2`
  - `UserGroup.frozen_reason: Option<String>`
  - `GroupMemberSource { Manual = 0, Synced = 1 }` with `From<i16>`
  - `UserGroupMember.source: GroupMemberSource`, `UserGroupMember.roles: Vec<i32>`
  - `GroupRoleSource { Manual = 0, DiscordRole = 1 }` with `From<i16>`
  - `GroupRoleSyncState { Synced = 0, Orphaned = 1 }` with `From<i16>`
  - `GroupRole { id, group_id, name, discord_role_id, source, sync_state, last_synced_at: Option<DateTime<Utc>>, position, member_count: i64 }`
  - `CreateGroupRole { name }`, `RenameGroupRole { name }`, `ImportDiscordRole { discord_role_id }`

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` in `ultros-api-types/src/user/group.rs`:

```rust
    #[test]
    fn group_source_mirrored_round_trips() {
        assert_eq!(GroupSource::from(2), GroupSource::DiscordGuildMirrored);
        assert_eq!(GroupSource::DiscordGuildMirrored as i16, 2);
    }

    #[test]
    fn member_source_round_trips_and_defaults_to_manual() {
        use super::GroupMemberSource;
        assert_eq!(GroupMemberSource::from(1), GroupMemberSource::Synced);
        assert_eq!(GroupMemberSource::from(0), GroupMemberSource::Manual);
        // Unknown values degrade to Manual: sync must never gain the right to
        // remove a member it does not positively know it added.
        assert_eq!(GroupMemberSource::from(7), GroupMemberSource::Manual);
    }

    #[test]
    fn role_enums_round_trip() {
        use super::{GroupRoleSource, GroupRoleSyncState};
        assert_eq!(GroupRoleSource::from(1), GroupRoleSource::DiscordRole);
        assert_eq!(GroupRoleSource::from(0), GroupRoleSource::Manual);
        assert_eq!(GroupRoleSource::from(9), GroupRoleSource::Manual);
        assert_eq!(GroupRoleSyncState::from(1), GroupRoleSyncState::Orphaned);
        assert_eq!(GroupRoleSyncState::from(0), GroupRoleSyncState::Synced);
        // An unknown state is treated as orphaned so a future value never
        // makes the UI claim a role is healthy.
        assert_eq!(GroupRoleSyncState::from(9), GroupRoleSyncState::Orphaned);
    }

    #[test]
    fn group_role_serialises_last_synced_as_rfc3339_or_null() {
        use super::{GroupRole, GroupRoleSource, GroupRoleSyncState};
        let role = GroupRole {
            id: 1,
            group_id: 2,
            name: "Officers".to_string(),
            discord_role_id: Some(123),
            source: GroupRoleSource::DiscordRole,
            sync_state: GroupRoleSyncState::Synced,
            last_synced_at: None,
            position: 3,
            member_count: 4,
        };
        let json = serde_json::to_string(&role).unwrap();
        assert!(json.contains(r#""last_synced_at":null"#));
        let back: GroupRole = serde_json::from_str(&json).unwrap();
        assert_eq!(back, role);
    }
```

Also extend the existing `group_source_round_trips_through_its_database_representation` array to include `GroupSource::DiscordGuildMirrored`.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p ultros-api-types group`
Expected: compile errors for the missing variants and types.

- [ ] **Step 3: Implement**

Replace the top of `ultros-api-types/src/user/group.rs` (everything above `pub struct UserGroupMember`) with:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How a group's membership is maintained. Stored as a `smallint` on
/// `user_group.source`, mirroring the `ListPermission` idiom.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum GroupSource {
    /// Members are added and removed by the owner. The default.
    Manual = 0,
    /// Created from a Discord guild. The guild link supplies the group's
    /// identity (name, icon); membership is still owner-managed.
    DiscordGuild = 1,
    /// At least one Discord role is imported, so sync owns part of the
    /// membership.
    DiscordGuildMirrored = 2,
}

impl From<i16> for GroupSource {
    fn from(value: i16) -> Self {
        match value {
            1 => GroupSource::DiscordGuild,
            2 => GroupSource::DiscordGuildMirrored,
            _ => GroupSource::Manual,
        }
    }
}

/// Who put a member into a group. Sync only ever removes `Synced` members.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum GroupMemberSource {
    /// Added by the owner, redeemed an invite, or created the group.
    Manual = 0,
    /// Added by Discord reconciliation or a gateway event.
    Synced = 1,
}

impl From<i16> for GroupMemberSource {
    fn from(value: i16) -> Self {
        match value {
            1 => GroupMemberSource::Synced,
            _ => GroupMemberSource::Manual,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum GroupRoleSource {
    Manual = 0,
    DiscordRole = 1,
}

impl From<i16> for GroupRoleSource {
    fn from(value: i16) -> Self {
        match value {
            1 => GroupRoleSource::DiscordRole,
            _ => GroupRoleSource::Manual,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum GroupRoleSyncState {
    Synced = 0,
    /// The Discord role or guild behind this role no longer exists. Members
    /// are kept; nothing will update them again.
    Orphaned = 1,
}

impl From<i16> for GroupRoleSyncState {
    fn from(value: i16) -> Self {
        match value {
            0 => GroupRoleSyncState::Synced,
            _ => GroupRoleSyncState::Orphaned,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserGroup {
    pub id: i32,
    pub name: String,
    pub owner_id: i64,
    /// Set when the group was created from a Discord guild.
    pub guild_id: Option<i64>,
    /// Guild icon captured at creation time. May be stale; render a fallback
    /// rather than treating this as authoritative.
    pub guild_icon_url: Option<String>,
    pub source: GroupSource,
    /// Set when the bot was removed from the guild. The group keeps its
    /// members but is no longer linked to Discord.
    pub frozen_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct GroupRole {
    pub id: i32,
    pub group_id: i32,
    pub name: String,
    pub discord_role_id: Option<i64>,
    pub source: GroupRoleSource,
    pub sync_state: GroupRoleSyncState,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub position: i32,
    pub member_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateGroupRole {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RenameGroupRole {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ImportDiscordRole {
    pub discord_role_id: i64,
}
```

Then change `UserGroupMember` to:

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserGroupMember {
    pub group_id: i32,
    pub user_id: i64,
    pub username: String,
    pub source: GroupMemberSource,
    /// Ids of the `GroupRole`s this member holds within the group.
    pub roles: Vec<i32>,
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ultros-api-types group`
Expected: all pass. `cargo check -p ultros-db` will now fail on `From<user_group::Model>` and `UserGroupMemberReturn`; Task 3 fixes that.

- [ ] **Step 5: Commit**

```bash
git add ultros-api-types/src/user/group.rs
git commit -m "Add group role and member-source API types

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Entities and type conversions

**Files:**
- Create: `ultros-db/src/entity/group_role.rs`, `ultros-db/src/entity/group_role_member.rs`, `ultros-db/src/entity/list_shared_role.rs`
- Modify: `ultros-db/src/entity/user_group.rs` (add `frozen_reason`, `GroupRole` relation), `ultros-db/src/entity/user_group_member.rs` (add `source`), `ultros-db/src/entity/mod.rs`, `ultros-db/src/entity/prelude.rs`, `ultros-db/src/common_type_conversions.rs`, `ultros-db/src/lists.rs` (the three `insert_group`/`add_group_member`/`use_group_invite` ActiveModel literals gain `source`), `ultros-db/src/lib.rs` (any `user_group_member::ActiveModel` literal, check with grep)

**Interfaces:**
- Produces: entities `group_role`, `group_role_member`, `list_shared_role`; `UserGroupMemberReturn(pub user_group_member::Model, pub discord_user::Model, pub Vec<i32>)`; `GroupRoleReturn(pub group_role::Model, pub i64)` converting to `GroupRole`; `ListSharedRoleReturn(pub list_shared_role::Model, pub group_role::Model, pub user_group::Model)` converting to `ListSharedRole` (Task 6 adds that API type).

- [ ] **Step 1: Write the entities**

`ultros-db/src/entity/group_role.rs`:

```rust
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "group_role")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub group_id: i32,
    pub name: String,
    /// Discord role this was imported from. Unique per group when present.
    pub discord_role_id: Option<i64>,
    /// See `ultros_api_types::user::group::GroupRoleSource`.
    pub source: i16,
    /// See `ultros_api_types::user::group::GroupRoleSyncState`.
    pub sync_state: i16,
    pub last_synced_at: Option<DateTimeUtc>,
    pub position: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user_group::Entity",
        from = "Column::GroupId",
        to = "super::user_group::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    UserGroup,
    #[sea_orm(has_many = "super::group_role_member::Entity")]
    GroupRoleMember,
    #[sea_orm(has_many = "super::list_shared_role::Entity")]
    ListSharedRole,
}

impl Related<super::user_group::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UserGroup.def()
    }
}

impl Related<super::group_role_member::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GroupRoleMember.def()
    }
}

impl Related<super::list_shared_role::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ListSharedRole.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
```

`ultros-db/src/entity/group_role_member.rs`:

```rust
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "group_role_member")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub role_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::group_role::Entity",
        from = "Column::RoleId",
        to = "super::group_role::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    GroupRole,
    #[sea_orm(
        belongs_to = "super::discord_user::Entity",
        from = "Column::UserId",
        to = "super::discord_user::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    DiscordUser,
}

impl Related<super::group_role::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GroupRole.def()
    }
}

impl Related<super::discord_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DiscordUser.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
```

`ultros-db/src/entity/list_shared_role.rs`:

```rust
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "list_shared_role")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub list_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub role_id: i32,
    pub permission: i16,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::list::Entity",
        from = "Column::ListId",
        to = "super::list::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    List,
    #[sea_orm(
        belongs_to = "super::group_role::Entity",
        from = "Column::RoleId",
        to = "super::group_role::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    GroupRole,
}

impl Related<super::list::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::List.def()
    }
}

impl Related<super::group_role::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GroupRole.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
```

- [ ] **Step 2: Update the existing entities**

In `ultros-db/src/entity/user_group.rs` add after `pub source: i16,`:

```rust
    /// Set when the bot was removed from the guild; see the API type doc.
    pub frozen_reason: Option<String>,
```

and add to `Relation`:

```rust
    #[sea_orm(has_many = "super::group_role::Entity")]
    GroupRole,
```

with:

```rust
impl Related<super::group_role::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GroupRole.def()
    }
}
```

In `ultros-db/src/entity/user_group_member.rs` add after `pub user_id: i64,`:

```rust
    /// See `ultros_api_types::user::group::GroupMemberSource`.
    pub source: i16,
```

In `ultros-db/src/entity/mod.rs` add, keeping alphabetical order:

```rust
pub mod group_role;
pub mod group_role_member;
pub mod list_shared_role;
```

In `ultros-db/src/entity/prelude.rs` add, keeping alphabetical order:

```rust
pub use super::group_role::Entity as GroupRole;
pub use super::group_role_member::Entity as GroupRoleMember;
pub use super::list_shared_role::Entity as ListSharedRole;
```

- [ ] **Step 3: Update the conversions**

In `ultros-db/src/common_type_conversions.rs`, change the `From<user_group::Model> for UserGroup` impl to destructure and pass `frozen_reason`:

```rust
impl From<user_group::Model> for UserGroup {
    fn from(value: user_group::Model) -> Self {
        let user_group::Model {
            id,
            name,
            owner_id,
            guild_id,
            guild_icon_url,
            source,
            frozen_reason,
        } = value;
        Self {
            id,
            name,
            owner_id,
            guild_id,
            guild_icon_url,
            source: source.into(),
            frozen_reason,
        }
    }
}
```

Replace `UserGroupMemberReturn` and its impl with:

```rust
/// A group member joined to their user row, plus the ids of the roles they
/// hold in that group.
pub struct UserGroupMemberReturn(
    pub user_group_member::Model,
    pub discord_user::Model,
    pub Vec<i32>,
);

impl From<UserGroupMemberReturn> for UserGroupMember {
    fn from(UserGroupMemberReturn(member, user, roles): UserGroupMemberReturn) -> Self {
        Self {
            group_id: member.group_id,
            user_id: member.user_id,
            username: user.username,
            source: member.source.into(),
            roles,
        }
    }
}

/// A role plus its member count, which is a separate aggregate query.
pub struct GroupRoleReturn(pub group_role::Model, pub i64);

impl From<GroupRoleReturn> for GroupRole {
    fn from(GroupRoleReturn(role, member_count): GroupRoleReturn) -> Self {
        Self {
            id: role.id,
            group_id: role.group_id,
            name: role.name,
            discord_role_id: role.discord_role_id,
            source: role.source.into(),
            sync_state: role.sync_state.into(),
            last_synced_at: role.last_synced_at,
            position: role.position,
            member_count,
        }
    }
}
```

Add `group_role` to the `entity::{...}` import at the top of that file and `GroupRole` to the `ultros_api_types::user::group::{...}` import.

- [ ] **Step 4: Fix the ActiveModel literals**

Run: `grep -rn "user_group_member::ActiveModel {" ultros-db/src`

Every hit (expected: `insert_group` and `add_group_member` and `use_group_invite` in `lists.rs`) gains:

```rust
            source: ActiveValue::Set(GroupMemberSource::Manual as i16),
```

Add `GroupMemberSource` to the `ultros_api_types::user::group::{...}` import in `lists.rs`.

Every `user_group::ActiveModel {` literal (expected: `insert_group` only) gains:

```rust
            frozen_reason: ActiveValue::Set(None),
```

- [ ] **Step 5: Compile the workspace**

Run: `cargo check --workspace`
Expected: clean. (`ultros` and the frontend only consume `UserGroup`/`UserGroupMember` through `From` and field reads, so no other edits.)

- [ ] **Step 6: Commit**

```bash
git add ultros-db/src/entity ultros-db/src/common_type_conversions.rs ultros-db/src/lists.rs
git commit -m "Add group role entities and thread member source through conversions

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Test fixtures and the membership invariant on existing functions

**Files:**
- Create: `ultros-db/src/group_roles.rs` (module skeleton + `tests` fixtures)
- Modify: `ultros-db/src/lib.rs` (add `mod group_roles;` next to `mod lists;`), `ultros-db/src/lists.rs` (`add_group_member`, `remove_group_member`, `get_group_members`)

**Interfaces:**
- Consumes: `UltrosDb::get_or_create_discord_user(user_id: u64, name: String) -> Result<discord_user::Model>` (existing, `discord.rs`).
- Produces:
  - `UltrosDb::add_group_member(&self, group_id: i32, owner_id: i64, user_id: i64, display_name: Option<String>) -> Result<()>`: upserts `discord_user` when `display_name` is `Some`, sets `source = Manual` on insert and on conflict.
  - `UltrosDb::remove_group_member(&self, group_id: i32, requester_id: i64, user_id: i64) -> Result<()>`: refuses `Synced` members with `GroupError::ManagedByDiscord`; deletes the member's `group_role_member` rows in the same transaction.
  - `UltrosDb::get_group_members(...)` now returns `Vec<UserGroupMemberReturn>` with role ids populated.
  - `GroupError` enum in `group_roles.rs`.
  - Test helpers in `group_roles::tests`: `test_db()`, `fresh_user(db, label) -> discord_user::Model`, `group_with_owner(db) -> (user_group::Model, discord_user::Model)`, `is_group_member(db, group_id, user_id) -> bool`, `is_role_member(db, role_id, user_id) -> bool`, `member_source(db, group_id, user_id) -> Option<i16>`.

- [ ] **Step 1: Create the module with the error type and fixtures**

`ultros-db/src/group_roles.rs`:

```rust
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
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, TransactionTrait, sea_query::OnConflict,
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

impl UltrosDb {}

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

    pub(crate) async fn group_with_owner(db: &UltrosDb) -> (user_group::Model, discord_user::Model) {
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
}
```

Add `mod group_roles;` to `ultros-db/src/lib.rs` beside `mod lists;` and `pub use group_roles::GroupError;` beside the existing `pub use lists::ListError;` (grep for `ListError` in `lib.rs` to find the spot; if it is not re-exported there, add both re-exports next to each other).

- [ ] **Step 2: Write the failing tests for the existing functions**

Append inside `group_roles::tests`:

```rust
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

        let result = db.add_group_member(group.id, owner.id, unknown_id, None).await;

        assert!(result.is_err(), "no discord_user row and nothing to create one from");
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
```

These reference `create_group_role` and `add_group_role_member`, which Task 5 adds. Until then this task's tests will not compile; that is expected and the reason Tasks 4 and 5 commit together after Task 5's Step 6. Proceed to Step 3 anyway so the production code is in place.

- [ ] **Step 3: Change `add_group_member` in `lists.rs`**

Replace the whole function with:

```rust
    /// Owner adds a member by Discord id. When `display_name` is given the
    /// `discord_user` row is upserted first, so people who have never logged
    /// into Ultros can be added; without it the foreign key requires that they
    /// already have a row. Re-adding an existing member marks them `Manual`,
    /// meaning the owner wants them kept even if Discord sync would drop them.
    pub async fn add_group_member(
        &self,
        group_id: i32,
        owner_id: i64,
        user_id: i64,
        display_name: Option<String>,
    ) -> Result<()> {
        let group = user_group::Entity::find_by_id(group_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::BadRequest("Group not found"))?;
        if group.owner_id != owner_id {
            return Err(ListError::Forbidden("Only the owner can add members").into());
        }
        if let Some(name) = display_name {
            self.get_or_create_discord_user(user_id as u64, name).await?;
        }
        user_group_member::Entity::insert(user_group_member::ActiveModel {
            group_id: ActiveValue::Set(group_id),
            user_id: ActiveValue::Set(user_id),
            source: ActiveValue::Set(GroupMemberSource::Manual as i16),
        })
        .on_conflict(
            sea_orm::sea_query::OnConflict::columns([
                user_group_member::Column::GroupId,
                user_group_member::Column::UserId,
            ])
            .update_column(user_group_member::Column::Source)
            .to_owned(),
        )
        .exec(&self.db)
        .await?;
        Ok(())
    }
```

- [ ] **Step 4: Change `remove_group_member` in `lists.rs`**

Replace the whole function with:

```rust
    /// Owner removes a member, or a member leaves. Synced members are refused:
    /// reconciliation would put them straight back, so the honest answer is
    /// "change their Discord role". Role memberships go in the same
    /// transaction so the group-membership invariant holds.
    pub async fn remove_group_member(
        &self,
        group_id: i32,
        requester_id: i64,
        user_id: i64,
    ) -> Result<()> {
        let group = user_group::Entity::find_by_id(group_id)
            .one(&self.db)
            .await?
            .ok_or(ListError::BadRequest("Group not found"))?;
        if group.owner_id != requester_id && user_id != requester_id {
            return Err(ListError::Forbidden(
                "Only the owner or the user themselves can remove a member",
            )
            .into());
        }
        let Some(member) = user_group_member::Entity::find_by_id((group_id, user_id))
            .one(&self.db)
            .await?
        else {
            return Ok(());
        };
        if GroupMemberSource::from(member.source) == GroupMemberSource::Synced {
            return Err(crate::group_roles::GroupError::ManagedByDiscord.into());
        }
        let txn = self.db.begin().await?;
        self.remove_user_from_all_roles_in_group(&txn, group_id, user_id)
            .await?;
        user_group_member::Entity::delete_by_id((group_id, user_id))
            .exec(&txn)
            .await?;
        txn.commit().await?;
        Ok(())
    }
```

`remove_user_from_all_roles_in_group` is defined in Task 5.

- [ ] **Step 5: Change `get_group_members` in `lists.rs`**

Replace the final `Ok(...)` expression of the function (after the membership check) with:

```rust
        let members = user_group_member::Entity::find()
            .filter(user_group_member::Column::GroupId.eq(group_id))
            .find_also_related(discord_user::Entity)
            .all(&self.db)
            .await?;

        // One query for every (role, user) pair in the group, then bucket by
        // user. Avoids a per-member query and keeps ordering by role position
        // so chips render in the same order everywhere.
        let role_rows: Vec<(i64, i32)> = group_role_member::Entity::find()
            .select_only()
            .column(group_role_member::Column::UserId)
            .column(group_role_member::Column::RoleId)
            .join(JoinType::InnerJoin, group_role_member::Relation::GroupRole.def())
            .filter(group_role::Column::GroupId.eq(group_id))
            .order_by_asc(group_role::Column::Position)
            .order_by_asc(group_role::Column::Id)
            .into_tuple()
            .all(&self.db)
            .await?;
        let mut roles_by_user: HashMap<i64, Vec<i32>> = HashMap::new();
        for (user_id, role_id) in role_rows {
            roles_by_user.entry(user_id).or_default().push(role_id);
        }

        Ok(members
            .into_iter()
            .filter_map(|(member, user)| {
                let roles = roles_by_user.remove(&member.user_id).unwrap_or_default();
                user.map(|u| UserGroupMemberReturn(member, u, roles))
            })
            .collect())
```

Add `group_role, group_role_member` to the `entity::{...}` import in `lists.rs`.

- [ ] **Step 6: Fix the `ultros` crate call site**

In `ultros/src/web.rs` the `add_group_member` handler now needs the fourth argument. Change it to:

```rust
pub(crate) async fn add_group_member(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path((group_id, member_id)): Path<(i32, i64)>,
    body: Option<Json<AddGroupMember>>,
) -> Result<Json<()>, ApiError> {
    let display_name = body.and_then(|Json(b)| b.display_name);
    db.add_group_member(group_id, user.id as i64, member_id, display_name)
        .await?;
    Ok(Json(()))
}
```

and add to `ultros-api-types/src/user/group.rs`:

```rust
/// Optional body for adding a member by id. `display_name` lets the server
/// create the user's row when they have never logged in.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AddGroupMember {
    #[serde(default)]
    pub display_name: Option<String>,
}
```

Import `AddGroupMember` in `web.rs`'s `ultros_api_types::user::group::{...}` list. `Option<Json<T>>` as an axum extractor yields `None` when there is no body, so the existing frontend call that sends nothing keeps working.

Also map `GroupError` to HTTP in `ultros/src/web/error.rs`: find where `ListError` is downcast to a status (grep `ListError::Forbidden`) and add the parallel arms:

```rust
            Some(GroupError::NotFound) | Some(GroupError::RoleNotFound) => StatusCode::NOT_FOUND,
            Some(GroupError::Forbidden(_)) => StatusCode::FORBIDDEN,
            Some(GroupError::BadRequest(_))
            | Some(GroupError::ManagedByDiscord)
            | Some(GroupError::RoleManagedByDiscord) => StatusCode::BAD_REQUEST,
```

using the same downcast shape the file already uses for `ListError`.

- [ ] **Step 7: Continue to Task 5**

Do not run tests or commit yet; the code references Task 5's functions.

---

### Task 5: Role CRUD and role membership

**Files:**
- Modify: `ultros-db/src/group_roles.rs`

**Interfaces:**
- Produces on `UltrosDb`:
  - `create_group_role(&self, group_id: i32, owner_id: i64, name: String) -> Result<group_role::Model>`
  - `import_discord_role(&self, group_id: i32, owner_id: i64, discord_role_id: i64, name: String, position: i32) -> Result<group_role::Model>` (flips `user_group.source` to `DiscordGuildMirrored`; refuses frozen or non-guild groups)
  - `rename_group_role(&self, group_id: i32, owner_id: i64, role_id: i32, name: String) -> Result<group_role::Model>`
  - `delete_group_role(&self, group_id: i32, owner_id: i64, role_id: i32) -> Result<()>` (reverts `source` to `DiscordGuild` when the last synced role goes)
  - `get_group_roles(&self, group_id: i32, user_id: i64) -> Result<Vec<GroupRoleReturn>>` (member-gated, ordered by position then id)
  - `get_group_role_members(&self, group_id: i32, user_id: i64, role_id: i32) -> Result<Vec<UserGroupMemberReturn>>`
  - `add_group_role_member(&self, group_id: i32, owner_id: i64, role_id: i32, user_id: i64) -> Result<()>` (manual roles only; inserts group membership with `Manual` if absent)
  - `remove_group_role_member(&self, group_id: i32, owner_id: i64, role_id: i32, user_id: i64) -> Result<()>` (manual roles only; group membership untouched)
  - `pub(crate) async fn remove_user_from_all_roles_in_group(&self, txn: &DatabaseTransaction, group_id: i32, user_id: i64) -> Result<()>`
  - private `load_owned_group(&self, group_id, owner_id) -> Result<user_group::Model>` and `load_role_in_group(&self, group_id, role_id) -> Result<group_role::Model>`

- [ ] **Step 1: Write the failing tests**

Append inside `group_roles::tests`:

```rust
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
        let result = db.create_group_role(group.id, owner.id, "   ".to_string()).await;
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

        db.delete_group_role(group.id, owner.id, role.id).await.unwrap();

        assert!(is_group_member(&db, group.id, member.id).await);
        assert!(group_role::Entity::find_by_id(role.id)
            .one(&db.db)
            .await
            .unwrap()
            .is_none());
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

        db.delete_group_role(group.id, owner.id, role.id).await.unwrap();
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
```

- [ ] **Step 2: Implement**

Replace the empty `impl UltrosDb {}` in `group_roles.rs` with:

```rust
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
    pub async fn delete_group_role(&self, group_id: i32, owner_id: i64, role_id: i32) -> Result<()> {
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
    pub async fn get_group_roles(&self, group_id: i32, user_id: i64) -> Result<Vec<GroupRoleReturn>> {
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
            .join(sea_orm::JoinType::InnerJoin, group_role_member::Relation::GroupRole.def())
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
        .do_nothing()
        .to_owned(),
    )
    .do_nothing()
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
        .do_nothing()
        .to_owned(),
    )
    .do_nothing()
    .exec(txn)
    .await?;
    Ok(())
}
```

Notes for the implementer:
- `.do_nothing()` after `.on_conflict(...)` is sea-orm's `Insert::do_nothing`, which makes a conflicting insert return `TryInsertResult::Conflicted` instead of `RecordNotInserted`. If the workspace sea-orm version lacks it, use the `update_column(<pk column>)` no-op trick already used in `use_group_invite`.
- `Column::max()` / `Column::count()` come from `sea_orm::ColumnTrait`; `.group_by` from `QuerySelect`; `.count(&txn)` from `PaginatorTrait`. Add `PaginatorTrait` and `RelationTrait` to the imports if the compiler asks.

- [ ] **Step 3: Compile the workspace**

Run: `cargo check --workspace --all-targets`
Expected: clean.

- [ ] **Step 4: Run the live-DB tests**

Run (with `DATABASE_URL` set to a scratch Postgres that has had `cargo run -p migration -- up` applied):

```bash
cargo test -p ultros-db group_roles -- --ignored --test-threads=1
```

Expected: every test in `group_roles::tests` passes, including Task 4's five.

- [ ] **Step 5: Run the existing group tests to confirm no regressions**

```bash
cargo test -p ultros-db group_member_tests -- --ignored --test-threads=1
```

Expected: these three tests use synthetic ids with no `discord_user` row and were failing on the foreign key before this change too. Fix them as part of this task: replace each `let owner_id = 900N;` / `let member_id = 900N;` pair with `fresh_user` calls from `crate::group_roles::tests` (they are `pub(crate)`), pass `None` as the new fourth argument to `add_group_member`, and confirm all three pass.

- [ ] **Step 6: CI check and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`. Then:

```bash
git add ultros-db/src/group_roles.rs ultros-db/src/lib.rs ultros-db/src/lists.rs ultros/src/web.rs ultros/src/web/error.rs ultros-api-types/src/user/group.rs
git commit -m "Add group roles with the group-membership invariant

Roles live inside a group. Adding a user to a role adds them to the group;
removing them from the group removes every role. Members added by Discord
sync cannot be removed by hand. The add-member path can now create the
discord_user row so people who never logged in can be added.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Role-targeted list shares and `get_permission`

**Files:**
- Modify: `ultros-api-types/src/list.rs` (add `ListSharedRole`, `ShareListRole`)
- Modify: `ultros-db/src/common_type_conversions.rs` (add `ListSharedRoleReturn`)
- Modify: `ultros-db/src/lists.rs` (`get_permission`, `share_list_with_role`, `unshare_list_from_role`, `get_list_shared_roles`)
- Test: `ultros-db/src/lists.rs` new `#[cfg(test)] mod role_share_tests`

**Interfaces:**
- Produces:
  - `ListSharedRole { list_id, role_id, role_name, group_id, group_name, permission }`, `ShareListRole { role_id, permission }` (API types)
  - `UltrosDb::share_list_with_role(&self, list_id: i32, owner_id: i64, role_id: i32, permission: ListPermission) -> Result<()>`
  - `UltrosDb::unshare_list_from_role(&self, list_id: i32, owner_id: i64, role_id: i32) -> Result<()>`
  - `UltrosDb::get_list_shared_roles(&self, list_id: i32, user_id: i64) -> Result<Vec<ListSharedRoleReturn>>`
  - `get_permission` considers `list_shared_role` via `group_role_member`.

- [ ] **Step 1: Add the API types**

In `ultros-api-types/src/list.rs` after `ShareListGroup`:

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ListSharedRole {
    pub list_id: i32,
    pub role_id: i32,
    pub role_name: String,
    pub group_id: i32,
    pub group_name: String,
    pub permission: ListPermission,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ShareListRole {
    pub role_id: i32,
    pub permission: ListPermission,
}
```

- [ ] **Step 2: Add the conversion**

In `ultros-db/src/common_type_conversions.rs` after `ListSharedGroupReturn`:

```rust
pub struct ListSharedRoleReturn(
    pub list_shared_role::Model,
    pub group_role::Model,
    pub user_group::Model,
);

impl From<ListSharedRoleReturn> for ListSharedRole {
    fn from(ListSharedRoleReturn(shared, role, group): ListSharedRoleReturn) -> Self {
        Self {
            list_id: shared.list_id,
            role_id: shared.role_id,
            role_name: role.name,
            group_id: group.id,
            group_name: group.name,
            permission: shared.permission.into(),
        }
    }
}
```

Add `list_shared_role` to the entity import and `ListSharedRole` to the `ultros_api_types::list::{...}` import.

- [ ] **Step 3: Write the failing tests**

Append to `ultros-db/src/lists.rs`:

```rust
/// Run with a disposable database:
///
/// ```bash
/// cargo test -p ultros-db role_share_tests -- --ignored --test-threads=1
/// ```
#[cfg(test)]
mod role_share_tests {
    use super::*;
    use crate::group_roles::tests::{fresh_user, group_with_owner, test_db};

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn a_role_share_grants_permission_to_role_members_only() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let in_role = fresh_user(&db, "in-role").await;
        let in_group_only = fresh_user(&db, "in-group").await;
        db.add_group_member(group.id, owner.id, in_group_only.id, None)
            .await
            .unwrap();
        let role = db
            .create_group_role(group.id, owner.id, "Officers".to_string())
            .await
            .unwrap();
        db.add_group_role_member(group.id, owner.id, role.id, in_role.id)
            .await
            .unwrap();
        let list = db
            .create_list(owner.clone(), "Officer list".to_string(), None)
            .await
            .unwrap();

        db.share_list_with_role(list.id, owner.id, role.id, ListPermission::Write)
            .await
            .unwrap();

        assert_eq!(
            db.get_permission(list.id, in_role.id).await.unwrap(),
            ListPermission::Write
        );
        assert_eq!(
            db.get_permission(list.id, in_group_only.id).await.unwrap(),
            ListPermission::None
        );
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn role_and_group_shares_take_the_maximum() {
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
        let list = db
            .create_list(owner.clone(), "Mixed list".to_string(), None)
            .await
            .unwrap();
        db.share_list_with_group(list.id, owner.id, group.id, ListPermission::Read)
            .await
            .unwrap();
        db.share_list_with_role(list.id, owner.id, role.id, ListPermission::Write)
            .await
            .unwrap();

        assert_eq!(
            db.get_permission(list.id, member.id).await.unwrap(),
            ListPermission::Write
        );
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn unsharing_a_role_revokes_and_listing_shares_names_the_group() {
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
        let list = db
            .create_list(owner.clone(), "Revoke list".to_string(), None)
            .await
            .unwrap();
        db.share_list_with_role(list.id, owner.id, role.id, ListPermission::Read)
            .await
            .unwrap();

        let shares = db.get_list_shared_roles(list.id, owner.id).await.unwrap();
        assert_eq!(shares.len(), 1);
        assert_eq!(shares[0].1.name, "Officers");
        assert_eq!(shares[0].2.id, group.id);

        db.unshare_list_from_role(list.id, owner.id, role.id)
            .await
            .unwrap();
        assert_eq!(
            db.get_permission(list.id, member.id).await.unwrap(),
            ListPermission::None
        );
        assert!(db.get_list_shared_roles(list.id, owner.id).await.unwrap().is_empty());
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn only_the_group_owner_can_share_to_its_roles() {
        let db = test_db().await;
        let (group, owner) = group_with_owner(&db).await;
        let stranger = fresh_user(&db, "stranger").await;
        let role = db
            .create_group_role(group.id, owner.id, "Officers".to_string())
            .await
            .unwrap();
        let list = db
            .create_list(stranger.clone(), "Stranger list".to_string(), None)
            .await
            .unwrap();

        let result = db
            .share_list_with_role(list.id, stranger.id, role.id, ListPermission::Read)
            .await;
        assert!(matches!(
            result.unwrap_err().downcast_ref::<ListError>(),
            Some(ListError::Forbidden(_))
        ));
    }
}
```

- [ ] **Step 4: Run to verify they fail**

Run: `cargo test -p ultros-db role_share_tests -- --ignored --test-threads=1`
Expected: compile errors, the functions do not exist.

- [ ] **Step 5: Extend `get_permission`**

In `lists.rs`, after the `owned_group_perms` loop and before the final `Ok(max_permission)`, add:

```rust
        // Role shares: list -> role -> role member. Same shape as the group
        // join above; a member holding several shared roles gets the max.
        let role_perms: Vec<i16> = list_shared_role::Entity::find()
            .select_only()
            .column(list_shared_role::Column::Permission)
            .join(
                JoinType::InnerJoin,
                list_shared_role::Relation::GroupRole.def(),
            )
            .join(
                JoinType::InnerJoin,
                group_role::Relation::GroupRoleMember.def(),
            )
            .filter(list_shared_role::Column::ListId.eq(list_id))
            .filter(group_role_member::Column::UserId.eq(user_id))
            .into_tuple()
            .all(&self.db)
            .await?;

        for perm in role_perms {
            let p = ListPermission::from(perm);
            if p > max_permission {
                max_permission = p;
            }
        }
```

Add `list_shared_role` to the entity import and `ListSharedRoleReturn` to the conversions import.

- [ ] **Step 6: Add the share functions**

After `unshare_list_from_group` in `lists.rs`:

```rust
    /// Share a list with one role of a group the caller owns. Mirrors
    /// `share_list_with_group`: only the list owner may share, and only into
    /// a group they also own, so a member cannot fan a list out to a group
    /// they merely belong to.
    pub async fn share_list_with_role(
        &self,
        list_id: i32,
        owner_id: i64,
        role_id: i32,
        permission: ListPermission,
    ) -> Result<()> {
        let current_perm = self.get_permission(list_id, owner_id).await?;
        if current_perm < ListPermission::Owner {
            return Err(ListError::Forbidden("Only the owner can share the list").into());
        }
        validate_share_permission(permission)?;
        let (role, group) = group_role::Entity::find_by_id(role_id)
            .find_also_related(user_group::Entity)
            .one(&self.db)
            .await?
            .ok_or(ListError::BadRequest("Role not found"))?;
        let group = group.ok_or(ListError::BadRequest("Role not found"))?;
        if group.owner_id != owner_id {
            return Err(ListError::Forbidden(
                "Only the group owner can share a list with that group's roles",
            )
            .into());
        }
        list_shared_role::Entity::insert(list_shared_role::ActiveModel {
            list_id: ActiveValue::Set(list_id),
            role_id: ActiveValue::Set(role.id),
            permission: ActiveValue::Set(permission as i16),
        })
        .on_conflict(
            sea_orm::sea_query::OnConflict::columns([
                list_shared_role::Column::ListId,
                list_shared_role::Column::RoleId,
            ])
            .update_column(list_shared_role::Column::Permission)
            .to_owned(),
        )
        .exec(&self.db)
        .await?;
        Ok(())
    }

    pub async fn unshare_list_from_role(
        &self,
        list_id: i32,
        owner_id: i64,
        role_id: i32,
    ) -> Result<()> {
        let current_perm = self.get_permission(list_id, owner_id).await?;
        if current_perm < ListPermission::Owner {
            return Err(ListError::Forbidden("Only the owner can unshare the list").into());
        }
        list_shared_role::Entity::delete_by_id((list_id, role_id))
            .exec(&self.db)
            .await?;
        Ok(())
    }
```

After `get_list_shared_groups`:

```rust
    pub async fn get_list_shared_roles(
        &self,
        list_id: i32,
        user_id: i64,
    ) -> Result<Vec<ListSharedRoleReturn>> {
        let permission = self.get_permission(list_id, user_id).await?;
        if permission < ListPermission::Owner {
            return Err(ListError::Forbidden("Only the owner can view shares").into());
        }
        let shares = list_shared_role::Entity::find()
            .filter(list_shared_role::Column::ListId.eq(list_id))
            .find_also_related(group_role::Entity)
            .all(&self.db)
            .await?;
        let group_ids: Vec<i32> = shares
            .iter()
            .filter_map(|(_, role)| role.as_ref().map(|r| r.group_id))
            .collect();
        let groups: HashMap<i32, user_group::Model> = user_group::Entity::find()
            .filter(user_group::Column::Id.is_in(group_ids))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|g| (g.id, g))
            .collect();
        Ok(shares
            .into_iter()
            .filter_map(|(shared, role)| {
                let role = role?;
                let group = groups.get(&role.group_id)?.clone();
                Some(ListSharedRoleReturn(shared, role, group))
            })
            .collect())
    }
```

- [ ] **Step 7: Run the tests**

```bash
cargo test -p ultros-db role_share_tests -- --ignored --test-threads=1
cargo test -p ultros-db group_roles -- --ignored --test-threads=1
cargo test -p ultros-api-types
```

Expected: all pass.

- [ ] **Step 8: CI check and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`.

```bash
git add ultros-api-types/src/list.rs ultros-db/src/common_type_conversions.rs ultros-db/src/lists.rs
git commit -m "Let lists be shared with a group role

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Sync-facing primitives

These are the DB operations stage 4's reconciliation and gateway handlers call. They take no owner id: the caller is the bot, and authorization is "the data came from Discord". Building them now means the invariant and the `Synced`-only removal rule are tested here, next to the code that enforces them.

**Files:**
- Modify: `ultros-db/src/group_roles.rs`

**Interfaces:**
- Produces on `UltrosDb`:
  - `pub struct SyncedRole { pub role_id: i32, pub group_id: i32, pub discord_role_id: i64 }`
  - `pub struct RoleSyncPlan { pub role_id: i32, pub adds: Vec<(i64, String)>, pub removes: Vec<i64> }` (`adds` are `(discord user id, display name)`)
  - `pub struct SyncSummary { pub added: usize, pub removed: usize, pub left_group: usize }`
  - `guild_has_synced_roles(&self, guild_id: i64) -> Result<bool>`
  - `guilds_with_synced_roles(&self) -> Result<Vec<i64>>`
  - `synced_roles_for_guild(&self, guild_id: i64) -> Result<Vec<SyncedRole>>`
  - `role_member_ids(&self, role_id: i32) -> Result<HashSet<i64>>`
  - `apply_role_sync(&self, group_id: i32, plans: Vec<RoleSyncPlan>) -> Result<SyncSummary>` (one transaction; stamps `last_synced_at = now` on every planned role)
  - `mark_role_orphaned(&self, guild_id: i64, discord_role_id: i64) -> Result<()>`
  - `freeze_group_for_guild(&self, guild_id: i64, reason: String) -> Result<Option<i32>>` (returns the frozen group id if one existed)

- [ ] **Step 1: Write the failing tests**

Append inside `group_roles::tests`:

```rust
    async fn guild_group_with_synced_role(
        db: &UltrosDb,
    ) -> (user_group::Model, discord_user::Model, group_role::Model, i64) {
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
                RoleSyncPlan { role_id: role_a.id, adds: vec![(user, "U".to_string())], removes: vec![] },
                RoleSyncPlan { role_id: role_b.id, adds: vec![(user, "U".to_string())], removes: vec![] },
            ],
        )
        .await
        .unwrap();

        db.apply_role_sync(
            group.id,
            vec![RoleSyncPlan { role_id: role_a.id, adds: vec![], removes: vec![user] }],
        )
        .await
        .unwrap();
        assert!(is_group_member(&db, group.id, user).await, "still holds role_b");

        db.apply_role_sync(
            group.id,
            vec![RoleSyncPlan { role_id: role_b.id, adds: vec![], removes: vec![user] }],
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
        assert!(db.guilds_with_synced_roles().await.unwrap().contains(&guild_id));
    }

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn orphaning_a_role_keeps_its_members() {
        let db = test_db().await;
        let (group, _owner, role, guild_id) = guild_group_with_synced_role(&db).await;
        let user = next_id();
        db.apply_role_sync(
            group.id,
            vec![RoleSyncPlan { role_id: role.id, adds: vec![(user, "U".to_string())], removes: vec![] }],
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
            db.synced_roles_for_guild(guild_id).await.unwrap().is_empty(),
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
            vec![RoleSyncPlan { role_id: role.id, adds: vec![(user, "U".to_string())], removes: vec![] }],
        )
        .await
        .unwrap();

        let frozen = db
            .freeze_group_for_guild(guild_id, "The Ultros bot was removed from the server".to_string())
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

        // The guild slot is free again: a new group can be linked to it.
        db.create_group_from_guild("Relinked".to_string(), owner.id, guild_id, None)
            .await
            .unwrap();
        assert_eq!(db.freeze_group_for_guild(next_id(), "x".to_string()).await.unwrap(), None);
    }
```

- [ ] **Step 2: Implement**

Add above `impl UltrosDb` in `group_roles.rs`:

```rust
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
}
```

Add inside `impl UltrosDb`:

```rust
    pub async fn guild_has_synced_roles(&self, guild_id: i64) -> Result<bool> {
        Ok(!self.synced_roles_for_guild(guild_id).await?.is_empty())
    }

    pub async fn guilds_with_synced_roles(&self) -> Result<Vec<i64>> {
        let ids: Vec<i64> = user_group::Entity::find()
            .select_only()
            .column(user_group::Column::GuildId)
            .distinct()
            .join(sea_orm::JoinType::InnerJoin, user_group::Relation::GroupRole.def())
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
            .join(sea_orm::JoinType::InnerJoin, group_role::Relation::UserGroup.def())
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
    /// leave the role; a user who then holds no synced role in the group and
    /// was sync-added leaves the group too. Manual members are never removed.
    pub async fn apply_role_sync(
        &self,
        group_id: i32,
        plans: Vec<RoleSyncPlan>,
    ) -> Result<SyncSummary> {
        let mut summary = SyncSummary::default();
        let txn = self.db.begin().await?;
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
            let still_held: Vec<i64> = group_role_member::Entity::find()
                .select_only()
                .column(group_role_member::Column::UserId)
                .distinct()
                .join(sea_orm::JoinType::InnerJoin, group_role_member::Relation::GroupRole.def())
                .filter(group_role::Column::GroupId.eq(group_id))
                .filter(group_role::Column::Source.eq(GroupRoleSource::DiscordRole as i16))
                .filter(group_role_member::Column::UserId.is_in(removed_users.iter().copied()))
                .into_tuple()
                .all(&txn)
                .await?;
            let still_held: HashSet<i64> = still_held.into_iter().collect();
            let candidates: Vec<i64> = removed_users
                .into_iter()
                .filter(|id| !still_held.contains(id))
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

    /// The Discord role behind a synced role was deleted. Members are kept;
    /// the role just stops updating.
    pub async fn mark_role_orphaned(&self, guild_id: i64, discord_role_id: i64) -> Result<()> {
        let role_ids: Vec<i32> = group_role::Entity::find()
            .select_only()
            .column(group_role::Column::Id)
            .join(sea_orm::JoinType::InnerJoin, group_role::Relation::UserGroup.def())
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

    /// The bot left the guild. Unlink the group, keep every member, turn
    /// synced roles into orphaned manual roles, and record why. Returns the
    /// affected group id, or `None` if no group was linked to that guild.
    pub async fn freeze_group_for_guild(&self, guild_id: i64, reason: String) -> Result<Option<i32>> {
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
        let mut active: user_group::ActiveModel = group.clone().into();
        active.guild_id = ActiveValue::Set(None);
        active.source = ActiveValue::Set(GroupSource::Manual as i16);
        active.frozen_reason = ActiveValue::Set(Some(reason));
        active.update(&txn).await?;
        txn.commit().await?;
        Ok(Some(group.id))
    }
```

Add `use sea_orm::RelationTrait;` (for `.def()`) and `PaginatorTrait` to the imports if the compiler asks. `.distinct()` is on `QuerySelect`.

- [ ] **Step 3: Compile and run**

```bash
cargo check --workspace --all-targets
cargo test -p ultros-db group_roles -- --ignored --test-threads=1
```

Expected: clean compile, all `group_roles` tests pass.

- [ ] **Step 4: CI check and commit**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`.

```bash
git add ultros-db/src/group_roles.rs
git commit -m "Add sync-facing group role primitives

apply_role_sync, freeze_group_for_guild, and mark_role_orphaned are the
DB half of Discord membership sync. Sync only ever removes members it
added; freezing keeps everyone and releases the guild slot.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Group detail read and PR

**Files:**
- Modify: `ultros-db/src/group_roles.rs` (add `get_group_detail`)
- Modify: `ultros-api-types/src/user/group.rs` (add `UserGroupDetail`)
- Modify: `ultros-db/src/common_type_conversions.rs`

**Interfaces:**
- Produces:
  - `UserGroupDetail { group: UserGroup, roles: Vec<GroupRole>, member_count: i64 }` (API type)
  - `UltrosDb::get_group_detail(&self, group_id: i32, user_id: i64) -> Result<(user_group::Model, Vec<GroupRoleReturn>, i64)>` (member-gated)
  - `UserGroup` also gains `member_count` on the summary list? No: `get_groups` stays as is; the summary grid in stage 3 calls a new `GET /group/summary` built in stage 2 on top of `group_member_counts`. Produce that too: `UltrosDb::group_member_counts(&self, group_ids: &[i32]) -> Result<HashMap<i32, i64>>`.

- [ ] **Step 1: Write the failing test**

Append inside `group_roles::tests`:

```rust
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
```

- [ ] **Step 2: Implement**

In `ultros-api-types/src/user/group.rs`:

```rust
/// Everything the group page needs in one round trip.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserGroupDetail {
    pub group: UserGroup,
    pub roles: Vec<GroupRole>,
    pub member_count: i64,
}
```

In `group_roles.rs` inside `impl UltrosDb`:

```rust
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
```

- [ ] **Step 3: Run the tests**

```bash
cargo test -p ultros-db group_roles -- --ignored --test-threads=1
```

Expected: all pass.

- [ ] **Step 4: CI check, commit, open the PR**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`.

```bash
git add ultros-db/src/group_roles.rs ultros-api-types/src/user/group.rs ultros-db/src/common_type_conversions.rs
git commit -m "Add group detail and member-count reads

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

Rebase onto current `main` (never merge main in), push, and open the PR against `main` so CI runs:

```bash
git fetch origin main && git rebase origin/main
git push -u origin claude/groups-feature-completion-2b10db
gh pr create --base main --title "Groups stage 1: roles schema, membership invariant, role list shares" --body-file - <<'EOF'
Stage 1 of 4 for the groups completion work. Spec: `docs/superpowers/specs/2026-09-07-groups-roles-and-discord-sync-design.md`.

Refs #1062, #1077.

- Migration: `group_role`, `group_role_member`, `list_shared_role`; `user_group.frozen_reason`; `user_group_member.source`.
- Roles inside a group with the invariant "every role member is a group member", enforced in the DB layer.
- Members added by Discord sync are marked `Synced` and cannot be removed by hand; sync never removes `Manual` members.
- Add-member can now create the `discord_user` row, so people who never logged in can be added (fixes the opaque foreign-key failure).
- Lists can be shared with a role; `get_permission` takes the max across user, group, and role shares.
- Sync-facing primitives (`apply_role_sync`, `freeze_group_for_guild`, `mark_role_orphaned`) ready for stage 4.

No user-facing change yet. Live-DB tests: `cargo test -p ultros-db group_roles role_share_tests group_member_tests -- --ignored --test-threads=1`.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
```

---

## Self-review against the spec

- **Section 1 (data model)**: Task 1 creates every table and column named in the spec with the same names and encodings. Task 3 mirrors them as entities. The invariant is implemented in Tasks 4 and 5 and tested in both directions.
- **Section 2 (sync engine)**: DB side only, by design of the stage split. `apply_role_sync`, `freeze_group_for_guild`, `mark_role_orphaned`, `synced_roles_for_guild`, `guilds_with_synced_roles`, and `guild_has_synced_roles` (Task 7) cover every DB operation the reconciliation and event handlers in section 2 need. Display-name refresh is tested. The `@everyone` case needs no DB support beyond a role row whose `discord_role_id` equals the guild id.
- **Section 3 (API)**: DB functions exist for every endpoint except member search, which is Discord-side and belongs to stage 2. The `add_group_member` body change is done here because the DB signature changed.
- **Section 4 (frontend)**: none in this stage; `UserGroupDetail` and `group_member_counts` are the reads it will use.
- **Section 5 (edge cases)**: concurrent duplicate import is covered by the unique index plus the pre-check; deleting a role keeps members (tested); manual member surviving a sync removal (tested).
- **Section 6 (testing)**: every listed DB test exists. The live-DB `#[ignore]` convention is the crate's existing one; the plan also repairs the three pre-existing `group_member_tests` that could never have passed against the foreign key.
- **Type consistency**: `UserGroupMemberReturn` is a three-field tuple everywhere after Task 3; `RoleSyncPlan.adds` is `Vec<(i64, String)>` in Task 7's interface, implementation, and tests; `GroupRoleReturn(model, i64)` is consumed as `.0` / `.1` consistently.
- **Deviation from spec to record**: the spec said a `Synced` member "cannot be removed by the owner through this endpoint". This plan extends that to self-removal too (Global Constraints explains why). The spec file gets a one-line amendment in stage 3 when the frontend hides the leave button for synced members.
