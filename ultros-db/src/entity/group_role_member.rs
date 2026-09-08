use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "group_role_member")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub role_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: i64,
    /// When this membership was created. Reconciliation compares it against
    /// the time its Discord snapshot was taken so a stale plan cannot remove
    /// somebody a gateway event added while the snapshot was being fetched.
    pub added_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::group_role::Entity",
        from = "Column::RoleId",
        to = "super::group_role::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    GroupRole,
    #[sea_orm(
        belongs_to = "super::discord_user::Entity",
        from = "Column::UserId",
        to = "super::discord_user::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    DiscordUser,
}

impl Related<super::group_role::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::GroupRole.def()
    }
}

impl Related<super::discord_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DiscordUser.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
