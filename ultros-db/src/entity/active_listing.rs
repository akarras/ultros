//! SeaORM Entity. Generated by sea-orm-codegen 0.9.2

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "active_listing")]
pub struct Model {
    #[sea_orm(primary_key, unique)]
    pub id: i32,
    pub world_id: i32,
    pub item_id: i32,
    pub retainer_id: i32,
    pub price_per_unit: i32,
    pub quantity: i32,
    pub hq: bool,
    #[sea_orm(primary_key, auto_increment = false)]
    pub timestamp: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::retainer::Entity",
        from = "Column::RetainerId",
        to = "super::retainer::Column::Id",
        on_update = "NoAction",
        on_delete = "NoAction"
    )]
    Retainer,
    #[sea_orm(
        belongs_to = "super::world::Entity",
        from = "Column::WorldId",
        to = "super::world::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    World,
    #[sea_orm(has_one = "super::materia_listing::Entity")]
    MateriaListing,
}

impl Related<super::retainer::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Retainer.def()
    }
}

impl Related<super::world::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::World.def()
    }
}

impl Related<super::materia_listing::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MateriaListing.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
