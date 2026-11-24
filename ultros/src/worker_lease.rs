//! Whole-worker exclusive Postgres session advisory lease for stateful
//! `ultros-market` workers (`ULTROS_ROLES=ingest,market,bot`).
//!
//! Prevents overlapping `ultros-market` replacements from running two
//! Universalis WebSocket ingest pipelines, two `AlertManager`s, or two Discord
//! gateway connections concurrently against the same deployment:
//! - Acquires `pg_try_advisory_lock(WORKER_LEASE_LOCK_KEY)` on a dedicated
//!   standalone `PgConnection` (outside the SeaORM pool, polling rather than
//!   blocking so standby workers never hold an active `virtualxid` that could
//!   deadlock `CREATE INDEX CONCURRENTLY` migrations).
//! - Runs a 15s `SELECT 1` heartbeat on that exact connection; if the lease
//!   connection drops or times out, cancels `shutdown_token` immediately so a
//!   partitioned worker exits cleanly rather than split-braining.
//! - Holds the lock until [`WorkerLease::release`] is called **after** all
//!   ingest, analyzer, ClickHouse, and bot shutdown stages have drained.

use std::time::Duration;

use anyhow::Result;
use sea_orm::sqlx::{Connection, PgConnection};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Stable Postgres session advisory-lock key fencing the active stateful
/// worker (`ingest` / `bot`). Distinct from `MIGRATION_LOCK_KEY` and
/// `ROLLUP_SCHEDULER_LOCK_KEY`.
pub(crate) const WORKER_LEASE_LOCK_KEY: i64 = 0x55_4c_54_52_4f_53_00_02;

/// Optional env override (`ULTROS_EXCLUSIVE_WORKER_LEASE=true`) to force the
/// exclusive worker lease even when `web` is also enabled.
pub(crate) const EXCLUSIVE_WORKER_LEASE_ENV: &str = "ULTROS_EXCLUSIVE_WORKER_LEASE";

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const RETRY_CONNECT_INTERVAL: Duration = Duration::from_secs(2);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(5);

/// Active whole-worker Postgres advisory lease. Call [`Self::release`] as the
/// final step of graceful shutdown so a standby worker cannot take over until
/// the outgoing worker has drained its writes.
pub(crate) struct WorkerLease {
    release_token: CancellationToken,
    heartbeat_task: Option<JoinHandle<()>>,
}

impl WorkerLease {
    /// Acquire the exclusive worker lease, waiting in standby until any
    /// current owner exits or `cancel_wait` fires (e.g. `SIGTERM` while in
    /// standby). Returns `Ok(None)` if `cancel_wait` fired before acquisition.
    ///
    /// Once acquired, a background heartbeat monitors the lock-owning session
    /// and cancels `on_lease_lost` if connectivity to Postgres is lost.
    pub(crate) async fn acquire(
        database_url: &str,
        cancel_wait: CancellationToken,
        on_lease_lost: CancellationToken,
    ) -> Result<Option<Self>> {
        Self::acquire_with_key(
            database_url,
            WORKER_LEASE_LOCK_KEY,
            cancel_wait,
            on_lease_lost,
        )
        .await
    }

    async fn acquire_with_key(
        database_url: &str,
        lock_key: i64,
        cancel_wait: CancellationToken,
        on_lease_lost: CancellationToken,
    ) -> Result<Option<Self>> {
        let mut logged_standby = false;
        let mut conn: Option<PgConnection> = None;

        loop {
            if cancel_wait.is_cancelled() {
                return Ok(None);
            }
            if conn.is_none() {
                let connect_res = tokio::select! {
                    _ = cancel_wait.cancelled() => return Ok(None),
                    res = PgConnection::connect(database_url) => res,
                };
                match connect_res {
                    Ok(c) => conn = Some(c),
                    Err(error) => {
                        warn!(
                            ?error,
                            "could not connect to Postgres for exclusive worker lease; retrying"
                        );
                        tokio::select! {
                            _ = cancel_wait.cancelled() => return Ok(None),
                            _ = tokio::time::sleep(RETRY_CONNECT_INTERVAL) => continue,
                        }
                    }
                }
            }

            let active_conn = conn.as_mut().expect("connection established");
            let try_lock = tokio::select! {
                _ = cancel_wait.cancelled() => return Ok(None),
                res = sea_orm::sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock($1)")
                    .bind(lock_key)
                    .fetch_one(&mut *active_conn) => res,
            };

            match try_lock {
                Ok(true) => {
                    info!(lock_key, "acquired exclusive worker lease");
                    metrics::gauge!("ultros_worker_lease_active").set(1.0);
                    let owned_conn = conn.take().expect("connection established");
                    let release_token = CancellationToken::new();
                    let task_release = release_token.clone();
                    let heartbeat_task = tokio::spawn(run_lease_heartbeat(
                        owned_conn,
                        lock_key,
                        task_release,
                        on_lease_lost,
                    ));
                    return Ok(Some(Self {
                        release_token,
                        heartbeat_task: Some(heartbeat_task),
                    }));
                }
                Ok(false) => {
                    metrics::gauge!("ultros_worker_lease_active").set(0.0);
                    if !logged_standby {
                        info!(
                            lock_key,
                            "another active worker holds the exclusive lease; waiting in standby"
                        );
                        logged_standby = true;
                    } else {
                        debug!(
                            lock_key,
                            "exclusive worker lease still held; staying in standby"
                        );
                    }
                    tokio::select! {
                        _ = cancel_wait.cancelled() => return Ok(None),
                        _ = tokio::time::sleep(POLL_INTERVAL) => {}
                    }
                }
                Err(error) => {
                    warn!(
                        ?error,
                        "failed querying exclusive worker lease; reconnecting"
                    );
                    conn = None;
                    tokio::select! {
                        _ = cancel_wait.cancelled() => return Ok(None),
                        _ = tokio::time::sleep(RETRY_CONNECT_INTERVAL) => {}
                    }
                }
            }
        }
    }

    /// Release the advisory lock and close the lease session cleanly.
    pub(crate) async fn release(mut self) {
        self.release_token.cancel();
        if let Some(task) = self.heartbeat_task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for WorkerLease {
    fn drop(&mut self) {
        // Ensure early error returns during startup still cancel the heartbeat
        // task and drop the advisory-lock connection promptly.
        self.release_token.cancel();
    }
}

async fn run_lease_heartbeat(
    mut conn: PgConnection,
    lock_key: i64,
    release_token: CancellationToken,
    on_lease_lost: CancellationToken,
) {
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    // Lock acquisition just proved the session alive.
    heartbeat.tick().await;
    loop {
        tokio::select! {
            _ = release_token.cancelled() => {
                let _ = tokio::time::timeout(
                    HEARTBEAT_TIMEOUT,
                    sea_orm::sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1)")
                        .bind(lock_key)
                        .fetch_one(&mut conn),
                )
                .await;
                metrics::gauge!("ultros_worker_lease_active").set(0.0);
                info!(lock_key, "released exclusive worker lease");
                return;
            }
            _ = heartbeat.tick() => {
                let alive = tokio::time::timeout(
                    HEARTBEAT_TIMEOUT,
                    sea_orm::sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&mut conn),
                )
                .await;
                match alive {
                    Ok(Ok(_)) => continue,
                    Ok(Err(error)) => error!(
                        ?error,
                        lock_key,
                        "lost exclusive worker lease connection; triggering graceful shutdown"
                    ),
                    Err(_) => error!(
                        lock_key,
                        "timed out checking exclusive worker lease; triggering graceful shutdown"
                    ),
                }
                metrics::gauge!("ultros_worker_lease_active").set(0.0);
                on_lease_lost.cancel();
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires live DB"]
    async fn exclusive_worker_lease_fences_standby_until_release() {
        let database_url = std::env::var("DATABASE_URL")
            .or_else(|_| std::env::var("PG_URL"))
            .expect("DATABASE_URL or PG_URL must be set for live DB test");
        let test_key: i64 = WORKER_LEASE_LOCK_KEY ^ 0x1357_2468;

        let cancel_a = CancellationToken::new();
        let lost_a = CancellationToken::new();
        let lease_a = WorkerLease::acquire_with_key(&database_url, test_key, cancel_a, lost_a)
            .await
            .expect("first worker acquires lease")
            .expect("lease_a acquired");

        // Second worker must stay in standby while lease_a is held.
        let url_b = database_url.clone();
        let cancel_b = CancellationToken::new();
        let lost_b = CancellationToken::new();
        let mut standby = tokio::spawn(async move {
            WorkerLease::acquire_with_key(&url_b, test_key, cancel_b, lost_b).await
        });

        tokio::time::sleep(Duration::from_millis(800)).await;
        assert!(
            !standby.is_finished(),
            "standby worker must not acquire the lease while active worker holds it"
        );

        // Releasing lease_a allows standby to take over cleanly.
        lease_a.release().await;
        let lease_b = tokio::time::timeout(Duration::from_secs(5), &mut standby)
            .await
            .expect("standby should acquire lease promptly after release")
            .expect("standby task join")
            .expect("standby acquire ok")
            .expect("lease_b acquired");
        lease_b.release().await;
    }
}
