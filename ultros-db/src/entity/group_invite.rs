use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// An invite code that grants membership of a group. Unlike `list_invite`
/// there is no permission column — group membership is binary.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "group_invite")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub group_id: i32,
    pub max_uses: Option<i32>,
    pub uses: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user_group::Entity",
        from = "Column::GroupId",
        to = "super::user_group::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    UserGroup,
}

impl Related<super::user_group::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UserGroup.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
