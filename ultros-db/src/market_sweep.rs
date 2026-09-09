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

#[cfg(test)]
mod tests {
    use super::*;
    use migration::{Migrator, MigratorTrait};
    use sea_orm::{ConnectOptions, ConnectionTrait, Database};

    /// Exercises the actual migration and typed helpers in a disposable
    /// namespace, never the default schema or a real unfinished sweep.
    #[tokio::test]
    #[ignore = "requires MIGRATION_TEST_DATABASE_URL"]
    async fn sweep_migration_and_restart_checkpoint_round_trip() {
        let url = std::env::var("MIGRATION_TEST_DATABASE_URL")
            .expect("set MIGRATION_TEST_DATABASE_URL for isolated database tests");
        let schema = format!(
            "sweep_test_{}_{}",
            std::process::id(),
            chrono::Utc::now().timestamp_micros().unsigned_abs()
        );
        let mut admin_options = ConnectOptions::new(url.clone());
        admin_options.max_connections(1).min_connections(1);
        let admin = Database::connect(admin_options).await.unwrap();
        admin
            .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        let mut options = ConnectOptions::new(url);
        options
            .max_connections(1)
            .min_connections(1)
            .set_schema_search_path(schema.clone());
        let connection = Database::connect(options.clone()).await.unwrap();
        let migration = Migrator::migrations()
            .into_iter()
            .find(|migration| migration.name() == "m20260908_000003_market_sweep")
            .expect("market sweep migration is registered");
        migration
            .up(&migration::SchemaManager::new(&connection))
            .await
            .unwrap();
        let db = UltrosDb::from_connection(connection.clone());
        let (run, resumed) = db.begin_or_resume_market_sweep(Some(123)).await.unwrap();
        assert!(!resumed);
        assert!(db.market_sweep_progress(run.id).await.unwrap().is_empty());

        // A second raw insertion must fail even without application locking.
        assert!(
            connection
                .execute_unprepared(
                    "INSERT INTO market_sweep (started_at) VALUES (CURRENT_TIMESTAMP)"
                )
                .await
                .is_err()
        );
        let mut progress = market_sweep_world::Model {
            sweep_id: run.id,
            world_id: 79,
            next_item_id: 101,
            completed_at: None,
            changed: 12,
            noop: 3,
            failed: 1,
            chunks_failed: 2,
            elapsed_ms: 5000,
        };
        db.record_market_sweep_progress(&progress).await.unwrap();
        progress.next_item_id = 201;
        progress.changed = 20;
        db.record_market_sweep_progress(&progress).await.unwrap();
        drop(db);
        connection.close().await.unwrap();

        // Reconnect to model a process restart, not an in-memory reload.
        let connection = Database::connect(options).await.unwrap();
        let db = UltrosDb::from_connection(connection.clone());
        let (restored, resumed) = db.begin_or_resume_market_sweep(Some(456)).await.unwrap();
        assert!(resumed);
        assert_eq!(restored.id, run.id);
        assert_eq!(restored.discord_channel_id, Some(456));
        assert_eq!(
            db.market_sweep_progress(run.id).await.unwrap(),
            vec![progress.clone()]
        );
        progress.completed_at = Some(chrono::Utc::now().fixed_offset());
        db.record_market_sweep_progress(&progress).await.unwrap();
        db.finish_market_sweep(run.id).await.unwrap();
        assert!(db.active_market_sweep().await.unwrap().is_none());
        let (next, resumed) = db.begin_or_resume_market_sweep(None).await.unwrap();
        assert!(!resumed);
        assert_ne!(next.id, run.id);
        migration
            .down(&migration::SchemaManager::new(&connection))
            .await
            .unwrap();
        assert!(
            connection
                .execute_unprepared("SELECT 1 FROM market_sweep_world")
                .await
                .is_err()
        );
        drop(db);
        connection.close().await.unwrap();
        admin
            .execute_unprepared(&format!("DROP SCHEMA {schema}"))
            .await
            .unwrap();
        admin.close().await.unwrap();
    }
}
