//! `SeaORM` Entity. Hand-authored for the resumable full market sweep.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "market_sweep_world")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub sweep_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub world_id: i32,
    /// First item id this world has *not* been swept for, in the sweep's
    /// ascending item order. 0 means the world has not been started.
    pub next_item_id: i32,
    /// Set once every chunk of the world has been attempted; a world with a
    /// completion stamp is skipped on resume.
    pub completed_at: Option<DateTimeWithTimeZone>,
    pub changed: i64,
    pub noop: i64,
    pub failed: i64,
    pub chunks_failed: i64,
    /// Wall-clock spent on this world, accumulated across every process that
    /// has worked on it.
    pub elapsed_ms: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::market_sweep::Entity",
        from = "Column::SweepId",
        to = "super::market_sweep::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    MarketSweep,
}

impl Related<super::market_sweep::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MarketSweep.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
