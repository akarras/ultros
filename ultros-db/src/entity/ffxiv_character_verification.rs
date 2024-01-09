//! `SeaORM` Entity. Generated by sea-orm-codegen 0.11.1

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "ffxiv_character_verification")]
pub struct Model {
    #[sea_orm(unique)]
    pub discord_user_id: i64,
    pub ffxiv_character_id: i32,
    pub challenge: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::discord_user::Entity",
        from = "Column::DiscordUserId",
        to = "super::discord_user::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    DiscordUser,
    #[sea_orm(
        belongs_to = "super::final_fantasy_character::Entity",
        from = "Column::FfxivCharacterId",
        to = "super::final_fantasy_character::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    FinalFantasyCharacter,
}

impl Related<super::discord_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DiscordUser.def()
    }
}

impl Related<super::final_fantasy_character::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::FinalFantasyCharacter.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
