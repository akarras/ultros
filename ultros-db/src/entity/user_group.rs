use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "user_group")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub owner_id: i64,
    /// Discord guild this group was created from, if any. Unique when present.
    pub guild_id: Option<i64>,
    /// Denormalized guild icon, refreshed on creation. Cosmetic only.
    pub guild_icon_url: Option<String>,
    /// How membership is maintained. See `ultros_api_types::user::group::GroupSource`.
    pub source: i16,
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
    #[sea_orm(has_many = "super::group_invite::Entity")]
    GroupInvite,
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

impl Related<super::group_invite::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GroupInvite.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
