//! `SeaORM` Entity. Hand-authored for the shared-list activity feed.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "list_activity")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub list_id: i32,
    pub actor_user_id: i64,
    pub actor_username: String,
    pub kind: String,
    pub list_item_id: Option<i32>,
    pub item_id: Option<i32>,
    #[sea_orm(column_type = "Json")]
    pub payload: Json,
    pub message: String,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::list::Entity",
        from = "Column::ListId",
        to = "super::list::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    List,
}

impl Related<super::list::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::List.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
