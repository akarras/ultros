use sea_orm_migration::prelude::*;

/// Transactional outbox for mirroring committed `sale_history` inserts into
/// ClickHouse's `sales` table (`ReplacingMergeTree(inserted_at)`).
///
/// Replaces the lossy analyzer broadcast-bus dual-write so sales cannot be
/// dropped on broadcast lag, queue overflow, or process crash — and so
/// catch-up/manual `update_sales` writes feed the exact same path as live
/// WebSocket sales.
///
/// Self-contained payload columns avoid joining the ~1.9B-row `sale_history`
/// table on every drain cycle. `id` (`BIGSERIAL`) is used only to order and
/// delete acknowledged batches — never as a monotonic skip-ahead watermark —
/// so out-of-order commits and wrapped negative `sale_history.id` (`pg_id`)
/// values can never be skipped.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TABLE IF NOT EXISTS clickhouse_sale_outbox (
                    id                  BIGSERIAL PRIMARY KEY,
                    pg_id               INTEGER NOT NULL,
                    sold_date           TIMESTAMP NOT NULL,
                    item_id             INTEGER NOT NULL,
                    hq                  BOOLEAN NOT NULL,
                    world_id            INTEGER NOT NULL,
                    price_per_item      INTEGER NOT NULL,
                    quantity            INTEGER NOT NULL,
                    buying_character_id INTEGER NOT NULL,
                    buyer_name          TEXT NOT NULL DEFAULT '',
                    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
                )",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS clickhouse_sale_outbox")
            .await?;
        Ok(())
    }
}
