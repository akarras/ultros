use sea_orm_migration::prelude::*;

/// Durable per-guild post-import reconciliation cooldown and follow-up state.
///
/// Importing a Discord role triggers an immediate background membership
/// reconcile bypassing the five-minute manual "Sync now" cooldown, guarded by
/// a short per-guild cooldown so bursts of role imports coalesce instead of
/// spamming full-guild member walks.
///
/// Storing the claim and any pending `follow_up_due_at` in Postgres (rather
/// than process-local RAM) ensures:
/// - Concurrent post-import triggers serialize atomically on `guild_id`.
/// - The cooldown survives `ultros-market` restarts and standby handoffs.
/// - A second role imported inside the cooldown window durably schedules a
///   single coalesced follow-up reconcile even if the first reconcile already
///   finished or the worker restarts before the timer fires.
///
/// Expand-only: touches no existing tables so already-running N-1 replicas are
/// unaffected.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TABLE IF NOT EXISTS guild_post_import_sync_claim (
                    guild_id         BIGINT PRIMARY KEY,
                    claimed_at       TIMESTAMPTZ NOT NULL,
                    follow_up_due_at TIMESTAMPTZ NULL
                );
                CREATE INDEX IF NOT EXISTS idx_guild_post_import_sync_claim_follow_up
                    ON guild_post_import_sync_claim (follow_up_due_at)
                    WHERE follow_up_due_at IS NOT NULL",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS guild_post_import_sync_claim")
            .await?;
        Ok(())
    }
}
