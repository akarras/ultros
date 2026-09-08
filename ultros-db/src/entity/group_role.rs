use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "group_role")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub group_id: i32,
    pub name: String,
    /// Discord role this was imported from. Unique per group when present.
    pub discord_role_id: Option<i64>,
    /// See `ultros_api_types::user::group::GroupRoleSource`.
    pub source: i16,
    /// See `ultros_api_types::user::group::GroupRoleSyncState`.
    pub sync_state: i16,
    pub last_synced_at: Option<DateTimeUtc>,
    pub position: i32,
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
    #[sea_orm(has_many = "super::group_role_member::Entity")]
    GroupRoleMember,
    #[sea_orm(has_many = "super::list_shared_role::Entity")]
    ListSharedRole,
}

impl Related<super::user_group::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UserGroup.def()
    }
}

impl Related<super::group_role_member::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GroupRoleMember.def()
    }
}

impl Related<super::list_shared_role::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ListSharedRole.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
