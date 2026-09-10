//! `SeaORM` Entity. Hand-authored for the resumable full market sweep.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "market_sweep")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub started_at: DateTimeWithTimeZone,
    /// `None` while the sweep is still in flight. At most one row may be
    /// unfinished at a time — see the partial unique index in the migration.
    pub finished_at: Option<DateTimeWithTimeZone>,
    /// Discord channel `/rescan_market` was invoked from, so a sweep resumed
    /// by a later process keeps reporting where the operator is watching.
    pub discord_channel_id: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::market_sweep_world::Entity")]
    MarketSweepWorld,
}

impl Related<super::market_sweep_world::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MarketSweepWorld.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
