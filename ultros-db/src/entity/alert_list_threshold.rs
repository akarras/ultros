//! `SeaORM` Entity. Hand-authored to mirror the `alert_item_threshold` shape.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "alert_list_threshold")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub alert_id: i32,
    pub list_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::alert::Entity",
        from = "Column::AlertId",
        to = "super::alert::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Alert,
    #[sea_orm(
        belongs_to = "super::list::Entity",
        from = "Column::ListId",
        to = "super::list::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    List,
}

impl Related<super::alert::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Alert.def()
    }
}

impl Related<super::list::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::List.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
