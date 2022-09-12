//! SeaORM Entity. Generated by sea-orm-codegen 0.9.2

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "alert")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub owner: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::discord_user::Entity",
        from = "Column::Owner",
        to = "super::discord_user::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    DiscordUser,
    #[sea_orm(has_many = "super::alert_discord_destination::Entity")]
    AlertDiscordDestination,
    #[sea_orm(has_many = "super::alert_retainer_undercut::Entity")]
    AlertRetainerUndercut,
}

impl Related<super::discord_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DiscordUser.def()
    }
}

impl Related<super::alert_discord_destination::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AlertDiscordDestination.def()
    }
}

impl Related<super::alert_retainer_undercut::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AlertRetainerUndercut.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}