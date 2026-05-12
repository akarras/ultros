//! `SeaORM` Entity. Hand-written to match
//! `migration/src/m20260512_000001_push_subscription.rs`.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "push_subscription")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i64,
    #[sea_orm(column_type = "Text")]
    pub endpoint: String,
    #[sea_orm(column_type = "Text")]
    pub p256dh: String,
    #[sea_orm(column_type = "Text")]
    pub auth: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub user_agent: Option<String>,
    pub created_at: DateTimeUtc,
    pub last_seen_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::discord_user::Entity",
        from = "Column::UserId",
        to = "super::discord_user::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    DiscordUser,
}

impl Related<super::discord_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DiscordUser.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
