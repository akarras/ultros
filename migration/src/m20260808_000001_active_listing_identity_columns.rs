use sea_orm_migration::prelude::*;

/// Gives `active_listing` a real identity and the payload fields we previously
/// discarded.
///
/// `listing_id` is Universalis' stable id for a listing (their `listingID`, a
/// decimal string). Until now every write path deduplicated by an advisory
/// read-diff-insert on (retainer, price, quantity, hq) with **no uniqueness at
/// the database level**, so any two writers racing on the same (world, item) —
/// our own task-per-websocket-message ingest, the catch-up sweep's
/// `buffer_unordered(50)`, the manual refresh route, or Universalis' own
/// duplicate event emission — could and did insert the same board several
/// times over (item 44119 on Coeurl reached 306 rows for a 30-listing board).
/// The partial unique index makes concurrent duplicate writes collapse into
/// one row via `ON CONFLICT` instead of relying on every caller to have read a
/// current snapshot.
///
/// Partial (`WHERE listing_id IS NOT NULL`) because every pre-migration row —
/// and any future payload that genuinely lacks a `listingID` — has no identity
/// to be unique on; those continue through the legacy multiset-diff paths.
///
/// `materia`/`stain_id`/`creator_name`/`is_crafted`/`on_mannequin` were always
/// in the payload and never stored. Universalis' change diff compares only
/// (listingID, price, quantity), so mutations of these fields emit no
/// websocket events — they are populated/refreshed exclusively by REST board
/// fetches (catch-up, manual refresh, sweeps).
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// Out of the migration transaction so the unique index can be built
    /// `CONCURRENTLY` — `active_listing` is written continuously by ingest and
    /// a plain `CREATE UNIQUE INDEX` would block writes for the whole build.
    /// The column adds are metadata-only on Postgres 11+ and each statement is
    /// individually atomic; everything is `IF NOT EXISTS` so a partial failure
    /// reruns cleanly.
    fn use_transaction(&self) -> Option<bool> {
        Some(false)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared(
            r#"ALTER TABLE active_listing
               ADD COLUMN IF NOT EXISTS listing_id text,
               ADD COLUMN IF NOT EXISTS materia jsonb,
               ADD COLUMN IF NOT EXISTS stain_id integer,
               ADD COLUMN IF NOT EXISTS creator_name text,
               ADD COLUMN IF NOT EXISTS is_crafted boolean NOT NULL DEFAULT false,
               ADD COLUMN IF NOT EXISTS on_mannequin boolean NOT NULL DEFAULT false"#,
        )
        .await?;
        conn.execute_unprepared(
            r#"CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS idx_active_listing_identity
               ON active_listing (world_id, item_id, listing_id)
               WHERE listing_id IS NOT NULL"#,
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared(r#"DROP INDEX CONCURRENTLY IF EXISTS idx_active_listing_identity"#)
            .await?;
        conn.execute_unprepared(
            r#"ALTER TABLE active_listing
               DROP COLUMN IF EXISTS listing_id,
               DROP COLUMN IF EXISTS materia,
               DROP COLUMN IF EXISTS stain_id,
               DROP COLUMN IF EXISTS creator_name,
               DROP COLUMN IF EXISTS is_crafted,
               DROP COLUMN IF EXISTS on_mannequin"#,
        )
        .await?;
        Ok(())
    }
}
