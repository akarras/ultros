//! `SeaORM` Entity. Generated by sea-orm-codegen 0.11.1

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "sale_history")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub quantity: i32,
    pub price_per_item: i32,
    pub buying_character_id: i32,
    pub hq: bool,
    pub sold_item_id: i32,
    pub sold_date: DateTime,
    pub world_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::unknown_final_fantasy_character::Entity",
        from = "Column::BuyingCharacterId",
        to = "super::unknown_final_fantasy_character::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    UnknownFinalFantasyCharacter,
    #[sea_orm(
        belongs_to = "super::world::Entity",
        from = "Column::WorldId",
        to = "super::world::Column::Id",
        on_update = "NoAction",
        on_delete = "NoAction"
    )]
    World,
}

impl Related<super::unknown_final_fantasy_character::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UnknownFinalFantasyCharacter.def()
    }
}

impl Related<super::world::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::World.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
