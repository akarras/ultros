//! Durable state for the full market sweep (`/rescan_market`).
//!
//! A sweep walks every marketable item on every world and takes hours, which
//! is longer than the gap between deploys — so the process running one is
//! reliably killed partway through. These rows are what lets the next process
//! pick the sweep back up instead of starting over: [`UltrosDb::active_market_sweep`]
//! finds the unfinished run, and its `market_sweep_world` rows say which
//! worlds are done and where each unfinished one left off.

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DbErr, EntityTrait, QueryFilter, Set,
    sea_query::OnConflict,
};

use crate::{
    UltrosDb,
    entity::{market_sweep, market_sweep_world},
};

impl UltrosDb {
    /// The sweep that is still in flight, if any. At most one row can be
    /// unfinished — the migration's partial unique index enforces it.
    pub async fn active_market_sweep(&self) -> Result<Option<market_sweep::Model>, DbErr> {
        market_sweep::Entity::find()
            .filter(market_sweep::Column::FinishedAt.is_null())
            .one(&self.db)
            .await
    }

    /// Returns the unfinished sweep, starting one when there is none.
    ///
    /// `discord_channel_id` is where progress should be reported; on an
    /// existing sweep it *replaces* the recorded channel, so re-running
    /// `/rescan_market` elsewhere moves the reporting to where the operator
    /// asked from rather than leaving it in a channel nobody is reading.
    ///
    /// The `bool` is true when an existing sweep was picked up.
    pub async fn begin_or_resume_market_sweep(
        &self,
        discord_channel_id: Option<i64>,
    ) -> Result<(market_sweep::Model, bool), DbErr> {
        if let Some(existing) = self.active_market_sweep().await? {
            let resumed = match discord_channel_id {
                Some(channel) if existing.discord_channel_id != Some(channel) => {
                    let mut active: market_sweep::ActiveModel = existing.into();
                    active.discord_channel_id = Set(Some(channel));
                    active.update(&self.db).await?
                }
                _ => existing,
            };
            return Ok((resumed, true));
        }
        let started = market_sweep::ActiveModel {
            id: ActiveValue::NotSet,
            started_at: Set(chrono::Utc::now().into()),
            finished_at: Set(None),
            discord_channel_id: Set(discord_channel_id),
        }
        .insert(&self.db)
        .await;
        match started {
            Ok(sweep) => Ok((sweep, false)),
            // Another replica inserted between the read and the write; the
            // partial unique index rejected ours. Its sweep is the one to
            // join, so re-read rather than reporting a failure.
            Err(error) => match self.active_market_sweep().await? {
                Some(existing) => Ok((existing, true)),
                None => Err(error),
            },
        }
    }

    /// Per-world progress recorded for a sweep so far.
    pub async fn market_sweep_progress(
        &self,
        sweep_id: i32,
    ) -> Result<Vec<market_sweep_world::Model>, DbErr> {
        market_sweep_world::Entity::find()
            .filter(market_sweep_world::Column::SweepId.eq(sweep_id))
            .all(&self.db)
            .await
    }

    /// Writes one world's resume cursor and running tallies. Called after
    /// every chunk, so an interrupted sweep loses at most one chunk of work.
    pub async fn record_market_sweep_progress(
        &self,
        progress: &market_sweep_world::Model,
    ) -> Result<(), DbErr> {
        let model = market_sweep_world::ActiveModel {
            sweep_id: Set(progress.sweep_id),
            world_id: Set(progress.world_id),
            next_item_id: Set(progress.next_item_id),
            completed_at: Set(progress.completed_at),
            changed: Set(progress.changed),
            noop: Set(progress.noop),
            failed: Set(progress.failed),
            chunks_failed: Set(progress.chunks_failed),
            elapsed_ms: Set(progress.elapsed_ms),
        };
        market_sweep_world::Entity::insert(model)
            .on_conflict(
                OnConflict::columns([
                    market_sweep_world::Column::SweepId,
                    market_sweep_world::Column::WorldId,
                ])
                .update_columns([
                    market_sweep_world::Column::NextItemId,
                    market_sweep_world::Column::CompletedAt,
                    market_sweep_world::Column::Changed,
                    market_sweep_world::Column::Noop,
                    market_sweep_world::Column::Failed,
                    market_sweep_world::Column::ChunksFailed,
                    market_sweep_world::Column::ElapsedMs,
                ])
                .to_owned(),
            )
            .exec(&self.db)
            .await?;
        Ok(())
    }

    /// Stamps a sweep finished, freeing the "one unfinished sweep" slot.
    pub async fn finish_market_sweep(&self, sweep_id: i32) -> Result<(), DbErr> {
        market_sweep::Entity::update_many()
            .col_expr(
                market_sweep::Column::FinishedAt,
                sea_orm::sea_query::Expr::value(Some(chrono::Utc::now().fixed_offset())),
            )
            .filter(market_sweep::Column::Id.eq(sweep_id))
            .exec(&self.db)
            .await?;
        Ok(())
    }
}
