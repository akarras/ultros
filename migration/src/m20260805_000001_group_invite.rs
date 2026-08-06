use sea_orm_migration::prelude::*;

/// Shareable invite codes for groups, mirroring `list_invite`.
///
/// Deliberately has no `permission` column: a list share carries Read/Write,
/// but group membership is a single binary state, so there would be nothing to
/// store. If groups ever grow roles, that's a new column then rather than a
/// smallint nothing reads now.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(GroupInvite::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(GroupInvite::Id)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(GroupInvite::GroupId).integer().not_null())
                    .col(ColumnDef::new(GroupInvite::MaxUses).integer())
                    .col(
                        ColumnDef::new(GroupInvite::Uses)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // Deleting a group takes its invites with it, so a stale
                    // code can never resolve to a group that no longer exists.
                    .foreign_key(
                        ForeignKey::create()
                            .from(GroupInvite::Table, GroupInvite::GroupId)
                            .to(UserGroup::Table, UserGroup::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Listing a group's invites is the common read; without this it's a
        // sequential scan over every invite in the table.
        manager
            .create_index(
                Index::create()
                    .name("idx_group_invite_group_id")
                    .table(GroupInvite::Table)
                    .col(GroupInvite::GroupId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(GroupInvite::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum GroupInvite {
    Table,
    Id,
    GroupId,
    MaxUses,
    Uses,
}

#[derive(DeriveIden)]
enum UserGroup {
    Table,
    Id,
}
