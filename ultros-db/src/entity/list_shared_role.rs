use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "list_shared_role")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub list_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub role_id: i32,
    pub permission: i16,
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
    #[sea_orm(
        belongs_to = "super::group_role::Entity",
        from = "Column::RoleId",
        to = "super::group_role::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    GroupRole,
}

impl Related<super::list::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::List.def()
    }
}

impl Related<super::group_role::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GroupRole.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
