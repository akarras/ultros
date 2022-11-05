//! SeaORM Entity. Generated by sea-orm-codegen 0.9.2

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "retainer")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub world_id: i32,
    pub name: String,
    pub retainer_city_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::retainer_city::Entity",
        from = "Column::RetainerCityId",
        to = "super::retainer_city::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    RetainerCity,
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
    #[sea_orm(has_many = "super::active_listing::Entity")]
    ActiveListing,
}

impl Related<super::retainer_city::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RetainerCity.def()
    }
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

impl Related<super::active_listing::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ActiveListing.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
