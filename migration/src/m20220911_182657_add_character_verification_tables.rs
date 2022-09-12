use sea_orm_migration::prelude::*;

use crate::m20220101_000001_create_table::{DiscordUser, FinalFantasyCharacter};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
                            .integer()
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
            .create_table(
                Table::create()
                    .table(OwnedFfxivCharacter::Table)
                    .col(
                        ColumnDef::new(OwnedFfxivCharacter::FfxivCharacterId)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(OwnedFfxivCharacter::DiscordUserId)
                            .integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(
                        OwnedFfxivCharacter::Table,
                        OwnedFfxivCharacter::DiscordUserId,
                    )
                    .to(DiscordUser::Table, DiscordUser::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        manager
            .create_foreign_key(
                ForeignKeyCreateStatement::new()
                    .from(
                        OwnedFfxivCharacter::Table,
                        OwnedFfxivCharacter::FfxivCharacterId,
                    )
                    .to(FinalFantasyCharacter::Table, FinalFantasyCharacter::Id)
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(FfxivCharacterVerification::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(OwnedFfxivCharacter::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(Iden)]
enum FfxivCharacterVerification {
    Table,
    Id,
    DiscordUserId,
    FfxivCharacterId,
    Challenge,
}

#[derive(Iden)]
enum OwnedFfxivCharacter {
    Table,
    FfxivCharacterId,
    DiscordUserId,
}
