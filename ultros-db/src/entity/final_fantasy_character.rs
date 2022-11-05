//! SeaORM Entity. Generated by sea-orm-codegen 0.9.2

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "final_fantasy_character")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i32,
    pub first_name: String,
    pub last_name: String,
    pub world_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::world::Entity",
        from = "Column::WorldId",
        to = "super::world::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    World,
    #[sea_orm(has_many = "super::owned_retainers::Entity")]
    OwnedRetainers,
    #[sea_orm(has_many = "super::owned_ffxiv_character::Entity")]
    OwnedFfxivCharacter,
}

impl Related<super::world::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::World.def()
    }
}

impl Related<super::owned_retainers::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::OwnedRetainers.def()
    }
}

impl Related<super::owned_ffxiv_character::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::OwnedFfxivCharacter.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
