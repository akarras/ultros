//! `SeaORM` Entity. Generated by sea-orm-codegen 0.10.2

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "list")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub owner: i64,
    pub name: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::list_item::Entity")]
    ListItem,
    #[sea_orm(has_many = "super::price_alert::Entity")]
    PriceAlert,
}

impl Related<super::list_item::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ListItem.def()
    }
}

impl Related<super::price_alert::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PriceAlert.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
