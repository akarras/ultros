//! `SeaORM` Entity. Generated by sea-orm-codegen 0.10.2

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "alert_retainer_undercut")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub alert_id: i32,
    pub margin_percent: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::alert::Entity",
        from = "Column::AlertId",
        to = "super::alert::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Alert,
}

impl Related<super::alert::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Alert.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
