use sea_orm_migration::prelude::*;

/// Links a `user_group` to the Discord guild it was created from.
///
/// `guild_id` is nullable because most groups are hand-made and have no guild.
/// The unique index therefore relies on Postgres treating NULLs as distinct:
/// any number of manual groups may coexist, but a given guild can back at most
/// one group. Phase 2 (membership mirroring) reconciles against that guarantee.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(UserGroup::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(UserGroup::GuildId).big_integer().null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Denormalized so rendering the groups list never has to call Discord.
        // Cosmetic and allowed to go stale if the guild changes its icon.
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(UserGroup::Table)
                    .add_column_if_not_exists(ColumnDef::new(UserGroup::GuildIconUrl).text().null())
                    .to_owned(),
            )
            .await?;

        // 0 = Manual, 1 = DiscordGuild. Kept separate from `guild_id` so phase 2
        // can add a "membership is mirrored from the guild" value without another
        // migration, and so unlinking a group is a state change rather than a
        // lossy NULL-out.
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(UserGroup::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(UserGroup::Source)
                            .small_integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_user_group_guild_id")
                    .table(UserGroup::Table)
                    .col(UserGroup::GuildId)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_user_group_guild_id")
                    .table(UserGroup::Table)
                    .to_owned(),
            )
            .await?;

        for column in [
            UserGroup::Source,
            UserGroup::GuildIconUrl,
            UserGroup::GuildId,
        ] {
            manager
                .alter_table(
                    TableAlterStatement::new()
                        .table(UserGroup::Table)
                        .drop_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum UserGroup {
    Table,
    GuildId,
    GuildIconUrl,
    Source,
}
