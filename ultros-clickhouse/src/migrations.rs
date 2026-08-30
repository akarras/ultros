//! Tracked, run-once ClickHouse migrations.
//!
//! [`crate::schema`] covers everything expressible as idempotent DDL —
//! `CREATE TABLE IF NOT EXISTS`, `ALTER TABLE ... ADD INDEX IF NOT EXISTS`.
//! Re-running those on every boot costs nothing, so they need no bookkeeping.
//!
//! This module exists for the operations that are *not* free to repeat:
//! mutations, which rewrite every part of a table. There is no
//! `IF NOT EXISTS` for those, and the deployment has no release-command hook
//! to run them out of band (`fly.toml` starts the server and nothing else), so
//! a one-shot binary would simply never be run. The main binary therefore
//! applies them itself on startup, against a ledger, the way
//! `sea_orm_migration` does for Postgres.
//!
//! ## Why "applied" means "submitted"
//!
//! A ClickHouse mutation is asynchronous. `ALTER TABLE ... MATERIALIZE INDEX`
//! returns once the mutation is *registered*, not once it has finished; the
//! server then works through it in the background and resumes it across
//! restarts. Registration is the durable act, so that is what the ledger
//! records. Progress and failures live in `system.mutations`:
//!
//! ```sql
//! SELECT is_done, parts_to_do, latest_fail_reason
//! FROM system.mutations WHERE table = 'sales'
//! ```
//!
//! ## Failure is soft on purpose
//!
//! [`crate::ClickHouseClient::migrate`] runs these after the DDL and does not
//! let a failure here fail the whole call. A failed `migrate()` disables the
//! sale dual-write and the rollup scheduler for that boot (see `main.rs`) —
//! far too much to give up because an index materialization could not be
//! submitted. An unrecorded migration is simply retried on the next boot,
//! and in the meantime the only cost is that queries relying on it scan more
//! granules than they need to.

use std::collections::HashSet;

use clickhouse::Client;
use tracing::info;

use crate::ClickHouseError;

/// A statement that must run exactly once per deployment.
pub struct Migration {
    /// Stable identifier recorded in the ledger. Never reuse or rewrite one —
    /// the ledger is keyed on it, so a renamed migration runs a second time.
    pub id: &'static str,
    pub sql: &'static str,
    /// Set when `sql` submits a background mutation, naming the table and a
    /// substring that identifies the command in `system.mutations`. Used as a
    /// second guard against duplicate submission — see [`run`].
    pub mutation: Option<MutationMarker>,
}

/// Identifies a migration's mutation in `system.mutations`.
pub struct MutationMarker {
    pub table: &'static str,
    /// Substring matched against `system.mutations.command`.
    pub command_contains: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[Migration {
    // Materializes the `idx_sales_buyer` skip index over the parts of `sales`
    // that predate it. A skip index only covers parts written after it exists,
    // so without this the owned-character purchase history is granule-pruned
    // for recent data and a full scan for everything older.
    id: "m20260830_000001_materialize_idx_sales_buyer",
    sql: "ALTER TABLE sales MATERIALIZE INDEX idx_sales_buyer",
    mutation: Some(MutationMarker {
        table: "sales",
        command_contains: "idx_sales_buyer",
    }),
}];

/// The ledger. `ReplacingMergeTree` on `id` so a double insert collapses
/// rather than accumulating.
async fn ensure_ledger(client: &Client) -> Result<(), ClickHouseError> {
    client
        .query(
            r#"
            CREATE TABLE IF NOT EXISTS _schema_migrations (
                id          String,
                applied_at  DateTime DEFAULT now()
            )
            ENGINE = ReplacingMergeTree(applied_at)
            ORDER BY id
            "#,
        )
        .execute()
        .await?;
    Ok(())
}

async fn applied_ids(client: &Client) -> Result<HashSet<String>, ClickHouseError> {
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Applied {
        id: String,
    }
    let rows: Vec<Applied> = client
        .query("SELECT id FROM _schema_migrations FINAL")
        .fetch_all()
        .await?;
    Ok(rows.into_iter().map(|r| r.id).collect())
}

/// Whether ClickHouse already knows about this mutation.
///
/// The ledger alone leaves a race: two replicas booting together both read
/// "not applied" and both submit, and a duplicate `MATERIALIZE INDEX` is a
/// second full rewrite of every part rather than a cheap no-op. Rolling
/// deploys make that overlap the normal case, not the rare one.
///
/// `system.mutations` retains finished entries only up to
/// `finished_mutations_to_keep`, so this is a race guard and not a substitute
/// for the ledger — it is consulted only for migrations the ledger says are
/// outstanding.
async fn mutation_already_submitted(
    client: &Client,
    marker: &MutationMarker,
) -> Result<bool, ClickHouseError> {
    #[derive(clickhouse::Row, serde::Deserialize)]
    struct Found {
        n: u8,
    }
    let pattern = format!("%{}%", marker.command_contains);
    let found: Found = client
        .query(
            "SELECT count() > 0 AS n FROM system.mutations \
             WHERE database = currentDatabase() AND table = ? \
               AND command LIKE ? AND is_killed = 0",
        )
        .bind(marker.table)
        .bind(pattern)
        .fetch_one()
        .await?;
    Ok(found.n != 0)
}

async fn record(client: &Client, id: &str) -> Result<(), ClickHouseError> {
    #[derive(serde::Serialize, clickhouse::Row)]
    struct LedgerRow<'a> {
        id: &'a str,
        #[serde(with = "clickhouse::serde::chrono::datetime")]
        applied_at: chrono::DateTime<chrono::Utc>,
    }
    let mut insert = client.insert::<LedgerRow>("_schema_migrations").await?;
    insert
        .write(&LedgerRow {
            id,
            applied_at: chrono::Utc::now(),
        })
        .await?;
    insert.end().await?;
    Ok(())
}

/// Apply every migration the ledger has not recorded yet, in order.
///
/// Stops at the first failure and returns it, leaving the rest unrecorded so
/// they are retried on the next boot rather than applied out of order.
pub async fn run(client: &Client) -> Result<(), ClickHouseError> {
    ensure_ledger(client).await?;
    let applied = applied_ids(client).await?;

    for migration in MIGRATIONS {
        if applied.contains(migration.id) {
            continue;
        }
        // A mutation already registered with the server counts as applied:
        // another replica beat us to it, or a previous boot submitted it and
        // died before writing the ledger.
        if let Some(marker) = &migration.mutation
            && mutation_already_submitted(client, marker).await?
        {
            info!(
                migration = migration.id,
                "mutation already registered with ClickHouse; recording it as applied"
            );
            record(client, migration.id).await?;
            continue;
        }

        info!(migration = migration.id, "applying ClickHouse migration");
        client.query(migration.sql).execute().await?;
        record(client, migration.id).await?;
        if migration.mutation.is_some() {
            info!(
                migration = migration.id,
                "mutation submitted; it completes in the background \
                 (track it in system.mutations)"
            );
        }
    }
    Ok(())
}

/// Warn loudly about anything that looks structurally wrong with the migration
/// list. Cheap enough to assert in tests rather than discover in production.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_ids_are_unique() {
        let ids: HashSet<_> = MIGRATIONS.iter().map(|m| m.id).collect();
        assert_eq!(
            ids.len(),
            MIGRATIONS.len(),
            "duplicate migration id: the ledger is keyed on id, so a duplicate \
             would mark both as applied after running only one"
        );
    }

    /// The ledger is keyed on `id`, so ids have to be stable and ordered.
    /// Matching `sea_orm_migration`'s `mYYYYMMDD_NNNNNN_name` shape keeps them
    /// sortable and makes it obvious that renaming one re-runs it.
    #[test]
    fn migration_ids_follow_the_seaorm_naming_shape() {
        for m in MIGRATIONS {
            let (date, rest) =
                m.id.strip_prefix('m')
                    .and_then(|s| s.split_once('_'))
                    .unwrap_or_else(|| panic!("migration id must start with mYYYYMMDD_: {}", m.id));
            assert_eq!(date.len(), 8, "expected mYYYYMMDD in {}", m.id);
            assert!(
                date.chars().all(|c| c.is_ascii_digit()),
                "expected mYYYYMMDD in {}",
                m.id
            );
            let (seq, name) = rest
                .split_once('_')
                .unwrap_or_else(|| panic!("migration id needs a sequence and a name: {}", m.id));
            assert!(
                seq.chars().all(|c| c.is_ascii_digit()),
                "expected a numeric sequence in {}",
                m.id
            );
            assert!(!name.is_empty(), "migration id needs a name: {}", m.id);
        }
    }

    /// A mutation migration that forgot its marker loses the duplicate-submit
    /// guard, which is exactly the case where duplication is expensive.
    #[test]
    fn mutation_migrations_declare_a_marker() {
        for m in MIGRATIONS {
            let submits_mutation = m.sql.contains("MATERIALIZE") || m.sql.contains("DELETE WHERE");
            assert_eq!(
                submits_mutation,
                m.mutation.is_some(),
                "migration {} submits a mutation but declares no marker (or vice versa)",
                m.id
            );
        }
    }
}
