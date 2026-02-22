use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "user_group")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub owner_id: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::discord_user::Entity",
        from = "Column::OwnerId",
        to = "super::discord_user::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    DiscordUser,
    #[sea_orm(has_many = "super::user_group_member::Entity")]
    UserGroupMember,
    #[sea_orm(has_many = "super::list_shared_group::Entity")]
    ListSharedGroup,
}

impl Related<super::discord_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DiscordUser.def()
    }
}

impl Related<super::user_group_member::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UserGroupMember.def()
    }
}

impl Related<super::list_shared_group::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ListSharedGroup.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
