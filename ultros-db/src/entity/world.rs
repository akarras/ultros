//! SeaORM Entity. Generated by sea-orm-codegen 0.9.2

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "world")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique)]
    pub name: String,
    pub datacenter_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::datacenter::Entity",
        from = "Column::DatacenterId",
        to = "super::datacenter::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Datacenter,
    #[sea_orm(has_many = "super::active_listing::Entity")]
    ActiveListing,
    #[sea_orm(has_many = "super::final_fantasy_character::Entity")]
    FinalFantasyCharacter,
    #[sea_orm(has_many = "super::retainer::Entity")]
    Retainer,
    #[sea_orm(has_many = "super::sale_history::Entity")]
    SaleHistory,
}

impl Related<super::datacenter::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Datacenter.def()
    }
}

impl Related<super::active_listing::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ActiveListing.def()
    }
}

impl Related<super::final_fantasy_character::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::FinalFantasyCharacter.def()
    }
}

impl Related<super::retainer::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Retainer.def()
    }
}

impl Related<super::sale_history::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SaleHistory.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}