use sea_orm_migration::prelude::*;

/// Durable state for a full market sweep, so a deploy in the middle of one
/// resumes instead of throwing away hours of work.
///
/// `market_sweep` is the run; `market_sweep_world` is one row per world with
/// the resume cursor and the tallies recovered so far. The cursor is an *item
/// id* rather than a chunk index because the item list can change under a
/// sweep (a game-data bump adds items); an id still names the same resume
/// point afterwards, a chunk offset does not.
///
/// The partial unique index enforces at most one unfinished sweep across every
/// replica sharing the database — the same invariant the in-process sweep lock
/// gives one process, which stops becoming enough the moment the cursor is
/// shared.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(MarketSweep::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(MarketSweep::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(MarketSweep::StartedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(MarketSweep::FinishedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    // Channel `/rescan_market` was invoked from, so a sweep
                    // picked up by a later process reports where the operator
                    // who started it is looking.
                    .col(
                        ColumnDef::new(MarketSweep::DiscordChannelId)
                            .big_integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Postgres treats every unfinished row's index key as the same value
        // here, so a second replica's insert fails instead of starting a
        // competing sweep against the same cursor rows.
        manager
            .get_connection()
            .execute_unprepared(
                r#"CREATE UNIQUE INDEX IF NOT EXISTS idx_market_sweep_one_unfinished
                   ON market_sweep ((finished_at IS NULL)) WHERE finished_at IS NULL"#,
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(MarketSweepWorld::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(MarketSweepWorld::SweepId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(MarketSweepWorld::WorldId)
                            .integer()
                            .not_null(),
                    )
                    // First item id this world has *not* been swept for. 0
                    // means the world has not started.
                    .col(
                        ColumnDef::new(MarketSweepWorld::NextItemId)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(MarketSweepWorld::CompletedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(MarketSweepWorld::Changed)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(MarketSweepWorld::Noop)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(MarketSweepWorld::Failed)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(MarketSweepWorld::ChunksFailed)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    // Wall-clock spent on this world, accumulated across every
                    // process that worked on it — what the report's "slowest
                    // world" line reads.
                    .col(
                        ColumnDef::new(MarketSweepWorld::ElapsedMs)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .primary_key(
                        Index::create()
                            .col(MarketSweepWorld::SweepId)
                            .col(MarketSweepWorld::WorldId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(MarketSweepWorld::Table, MarketSweepWorld::SweepId)
                            .to(MarketSweep::Table, MarketSweep::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(MarketSweepWorld::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(MarketSweep::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum MarketSweep {
    Table,
    Id,
    StartedAt,
    FinishedAt,
    DiscordChannelId,
}

#[derive(DeriveIden)]
enum MarketSweepWorld {
    Table,
    SweepId,
    WorldId,
    NextItemId,
    CompletedAt,
    Changed,
    Noop,
    Failed,
    ChunksFailed,
    ElapsedMs,
}
