use sea_orm_migration::prelude::*;

use crate::m20220101_000001_create_table::{DiscordUser, FinalFantasyCharacter};
use crate::m20220911_182657_add_character_verification_tables::FfxivCharacterVerification;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Character claims are per-user bookkeeping, not proof of identity: a
        // claim only groups a user's own retainers under a character. With
        // `ffxiv_character_id` as the sole primary key, the first person to
        // claim a character locked everyone else out of it forever, which
        // becomes a squatting hazard now that claiming no longer requires the
        // Lodestone bio challenge. The composite key lets any number of users
        // claim the same character independently.
        //
        // Every ownership query already filters on both columns (see
        // `user_owns_character` / `get_all_characters_for_discord_user`), so the
        // permission gate on retainer assignment is unaffected.
        db.execute_unprepared(
            r#"ALTER TABLE owned_ffxiv_character
                DROP CONSTRAINT IF EXISTS owned_ffxiv_character_pkey"#,
        )
        .await?;
        db.execute_unprepared(
            r#"ALTER TABLE owned_ffxiv_character
                ADD PRIMARY KEY (ffxiv_character_id, discord_user_id)"#,
        )
        .await?;

        // The Lodestone bio challenge is gone, so the challenge table has no
        // remaining writers or readers.
        manager
            .drop_table(
                Table::drop()
                    .table(FfxivCharacterVerification::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Going back to an exclusive claim needs the extra claimants gone
        // first; keep the lowest discord_user_id per character arbitrarily,
        // since nothing recorded who claimed first.
        db.execute_unprepared(
            r#"DELETE FROM owned_ffxiv_character a
                USING owned_ffxiv_character b
                WHERE a.ffxiv_character_id = b.ffxiv_character_id
                  AND a.discord_user_id > b.discord_user_id"#,
        )
        .await?;
        db.execute_unprepared(
            r#"ALTER TABLE owned_ffxiv_character
                DROP CONSTRAINT IF EXISTS owned_ffxiv_character_pkey"#,
        )
        .await?;
        db.execute_unprepared(
            r#"ALTER TABLE owned_ffxiv_character
                ADD PRIMARY KEY (ffxiv_character_id)"#,
        )
        .await?;

        manager
            .create_table(
                Table::create()
                    .table(FfxivCharacterVerification::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FfxivCharacterVerification::Id)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(FfxivCharacterVerification::DiscordUserId)
                            .big_unsigned()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(FfxivCharacterVerification::FfxivCharacterId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(FfxivCharacterVerification::Challenge)
                            .string()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(
                        FfxivCharacterVerification::Table,
                        FfxivCharacterVerification::DiscordUserId,
                    )
                    .to(DiscordUser::Table, DiscordUser::Id)
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(
                        FfxivCharacterVerification::Table,
                        FfxivCharacterVerification::FfxivCharacterId,
                    )
                    .to(FinalFantasyCharacter::Table, FinalFantasyCharacter::Id)
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
