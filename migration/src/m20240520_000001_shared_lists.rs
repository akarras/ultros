use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(UserGroup::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(UserGroup::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(UserGroup::Name).string().not_null())
                    .col(ColumnDef::new(UserGroup::OwnerId).big_unsigned().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(UserGroup::Table, UserGroup::OwnerId)
                            .to(DiscordUser::Table, DiscordUser::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(UserGroupMember::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(UserGroupMember::GroupId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(UserGroupMember::UserId)
                            .big_unsigned()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(UserGroupMember::GroupId)
                            .col(UserGroupMember::UserId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(UserGroupMember::Table, UserGroupMember::GroupId)
                            .to(UserGroup::Table, UserGroup::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(UserGroupMember::Table, UserGroupMember::UserId)
                            .to(DiscordUser::Table, DiscordUser::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ListSharedUser::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ListSharedUser::ListId).integer().not_null())
                    .col(
                        ColumnDef::new(ListSharedUser::UserId)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ListSharedUser::Permission)
                            .small_integer()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(ListSharedUser::ListId)
                            .col(ListSharedUser::UserId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ListSharedUser::Table, ListSharedUser::ListId)
                            .to(List::Table, List::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ListSharedUser::Table, ListSharedUser::UserId)
                            .to(DiscordUser::Table, DiscordUser::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ListSharedGroup::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ListSharedGroup::ListId).integer().not_null())
                    .col(
                        ColumnDef::new(ListSharedGroup::GroupId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ListSharedGroup::Permission)
                            .small_integer()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(ListSharedGroup::ListId)
                            .col(ListSharedGroup::GroupId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ListSharedGroup::Table, ListSharedGroup::ListId)
                            .to(List::Table, List::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ListSharedGroup::Table, ListSharedGroup::GroupId)
                            .to(UserGroup::Table, UserGroup::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(ListInvite::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ListInvite::Id)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ListInvite::ListId).integer().not_null())
                    .col(
                        ColumnDef::new(ListInvite::Permission)
                            .small_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ListInvite::MaxUses).integer())
                    .col(
                        ColumnDef::new(ListInvite::Uses)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ListInvite::Table, ListInvite::ListId)
                            .to(List::Table, List::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ListInvite::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ListSharedGroup::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ListSharedUser::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(UserGroupMember::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(UserGroup::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum UserGroup {
    Table,
    Id,
    Name,
    OwnerId,
}

#[derive(DeriveIden)]
enum UserGroupMember {
    Table,
    GroupId,
    UserId,
}

#[derive(DeriveIden)]
enum ListSharedUser {
    Table,
    ListId,
    UserId,
    Permission,
}

#[derive(DeriveIden)]
enum ListSharedGroup {
    Table,
    ListId,
    GroupId,
    Permission,
}

#[derive(DeriveIden)]
enum ListInvite {
    Table,
    Id,
    ListId,
    Permission,
    MaxUses,
    Uses,
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
