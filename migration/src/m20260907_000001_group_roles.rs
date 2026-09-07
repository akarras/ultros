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
