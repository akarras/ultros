use sea_orm_migration::prelude::*;

use crate::{
    m20220101_000001_create_table::FinalFantasyCharacter,
    m20220911_182657_add_character_verification_tables::FfxivCharacterVerification,
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
                    .name("ffxiv_character_to_character_fk")
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                TableAlterStatement::new()
                    .table(FfxivCharacterVerification::Table)
                    .drop_column(FfxivCharacterVerification::Id)
                    .add_column(
                        ColumnDef::new(FfxivCharacterVerification::Id)
                            .primary_key()
                            .auto_increment()
                            .integer(),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_foreign_key(
                ForeignKeyDropStatement::new()
                    .table(FfxivCharacterVerification::Table)
                    .name("ffxiv_character_to_character_fk")
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
enum Post {
    Table,
    Id,
    Title,
    Text,
}
