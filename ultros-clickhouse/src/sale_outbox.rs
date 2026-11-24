//! Durable Postgres-outbox drainer for ClickHouse `sales`.
//!
//! Replaces the lossy in-memory broadcast-bus `Writer<SaleRow>` path: every
//! committed `sale_history` insert (`UltrosDb::update_sales`, covering both
//! Universalis WebSocket ingest and `UpdateService` catch-up) atomically
//! writes matching rows to Postgres `clickhouse_sale_outbox`.
//!
//! A single leader elected via Postgres session advisory lock drains committed
//! non-quarantined batches in `id ASC` order, validates ClickHouse `DateTime`
//! bounds (durably quarantining corrupt rows so they cannot stall later valid
//! sales), flushes valid rows into ClickHouse `sales`
//! (`ReplacingMergeTree(inserted_at)` keyed on
//! `(item_id, hq, world_id, sold_date, pg_id)`), and deletes acknowledged
//! outbox IDs only after ClickHouse confirms the insert.
//!
//! Because rows are deleted by exact ID rather than skipped past a high-water
//! mark, out-of-order transactions and wrapped negative `sale_history.id`
//! values can never be skipped. Because `sales` is a `ReplacingMergeTree`
//! read with `FINAL`, crashing between ClickHouse insert and Postgres `DELETE`
//! replays the batch without double-counting.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::Utc;
use futures::future::BoxFuture;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use ultros_db::{
    UltrosDb,
    clickhouse_outbox::{SaleOutboxMetrics, SaleOutboxRow},
};

use crate::{ClickHouseClient, ClickHouseError, rows::SaleRow, writer};

/// Stable Postgres advisory-lock key electing the one replica draining
/// `clickhouse_sale_outbox` into ClickHouse (`"SALEOUT"` in ASCII).
/// Distinct from `ROLLUP_SCHEDULER_LOCK_KEY` and `FULL_SWEEP_LOCK_KEY`.
const SALE_OUTBOX_LOCK_KEY: i64 = 0x0053_414c_454f_5554;

/// Steady-state batch cap (~100 rows every 5s at normal ingest rate).
const STEADY_BATCH_SIZE: usize = 1_000;
/// Catch-up batch cap once a full batch indicates an outage backlog, keeping
/// ClickHouse part creation low (~75 parts per 1.5M-row day of outage instead
/// of 1,500 parts).
const BACKLOG_BATCH_SIZE: usize = 20_000;
const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_secs(5);
const INSERT_TIMEOUT: Duration = Duration::from_secs(30);
const PG_OP_TIMEOUT: Duration = Duration::from_secs(5);
const DRAIN_BUDGET: Duration = Duration::from_secs(5);
/// Hard ceiling on [`SaleOutboxDrainer::shutdown`] so a wedged Postgres or
/// ClickHouse connection can never eat the global 30s SIGTERM budget. Because
/// unacknowledged rows stay persisted in Postgres, aborting loses zero sales.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);
const INITIAL_BACKOFF: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const MIGRATION_RETRY_INTERVAL: Duration = Duration::from_secs(60);
const LEASE_RETRY_INTERVAL: Duration = Duration::from_secs(15);
const LEASE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const LEASE_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(5);
const LOCK_OP_TIMEOUT: Duration = Duration::from_secs(5);
const UNLOCK_TIMEOUT: Duration = Duration::from_secs(2);
const METRICS_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// Handle to the background `clickhouse_sale_outbox` drainer.
#[derive(Clone)]
pub struct SaleOutboxDrainer {
    token: CancellationToken,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    ready: watch::Receiver<bool>,
}

impl SaleOutboxDrainer {
    /// Production startup: retry ClickHouse schema initialization on startup
    /// outages (while still exporting Postgres outbox depth/age metrics),
    /// elect a single drainer via Postgres advisory lock, and drain
    /// `clickhouse_sale_outbox` until [`Self::shutdown`].
    pub fn spawn_recovering(
        client: ClickHouseClient,
        db: UltrosDb,
        token: CancellationToken,
    ) -> Self {
        let (ready_tx, ready) = watch::channel(false);
        let token = token.child_token();
        let worker_token = token.clone();
        let task = tokio::spawn(async move {
            if !initialize_with_metrics(&client, &db, &worker_token).await {
                return;
            }
            ready_tx.send_replace(true);
            run_with_advisory_lock(client, db, worker_token).await;
        });
        Self {
            token,
            task: Arc::new(Mutex::new(Some(task))),
            ready,
        }
    }

    /// Wait until ClickHouse schema initialization succeeds; `false` means the
    /// drainer exited before ClickHouse became reachable.
    pub async fn wait_ready(&self) -> bool {
        let mut ready = self.ready.clone();
        loop {
            if *ready.borrow_and_update() {
                return true;
            }
            if ready.changed().await.is_err() {
                return false;
            }
        }
    }

    /// Cancel the drainer, perform a bounded final drain, release the advisory
    /// lock, and await task exit up to [`SHUTDOWN_TIMEOUT`]. Any rows not
    /// drained remain safely persisted in Postgres `clickhouse_sale_outbox`.
    pub async fn shutdown(&self) {
        self.token.cancel();
        let mut task = self.task.lock().await;
        if let Some(mut handle) = task.take() {
            match tokio::time::timeout(SHUTDOWN_TIMEOUT, &mut handle).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    warn!(
                        ?error,
                        "ClickHouse sale outbox drainer failed during shutdown"
                    );
                }
                Err(_) => {
                    warn!(
                        timeout_secs = SHUTDOWN_TIMEOUT.as_secs(),
                        "ClickHouse sale outbox shutdown timed out; aborting task (rows remain durable in Postgres)"
                    );
                    handle.abort();
                }
            }
        }
    }
}

/// Retry ClickHouse DDL while continuing to publish Postgres outbox backlog
/// gauges so operators see queue growth during a startup ClickHouse outage.
async fn initialize_with_metrics(
    client: &ClickHouseClient,
    db: &UltrosDb,
    token: &CancellationToken,
) -> bool {
    loop {
        let result = tokio::select! {
            _ = token.cancelled() => return false,
            result = tokio::time::timeout(Duration::from_secs(30), client.migrate()) => result,
        };
        match result {
            Ok(Ok(())) => return true,
            result => {
                metrics::counter!("ultros_clickhouse_writer_migration_failures_total").increment(1);
                warn!(
                    ?result,
                    "ClickHouse schema initialization failed; retrying in 60 seconds"
                );
            }
        }
        publish_pg_metrics(db).await;
        tokio::select! {
            _ = token.cancelled() => return false,
            _ = tokio::time::sleep(MIGRATION_RETRY_INTERVAL) => {}
        }
    }
}

async fn publish_pg_metrics(db: &UltrosDb) {
    if let Ok(Ok(stats)) = tokio::time::timeout(PG_OP_TIMEOUT, db.sale_outbox_metrics()).await {
        record_metrics(stats);
    }
}

fn record_metrics(stats: SaleOutboxMetrics) {
    metrics::gauge!("ultros_clickhouse_sale_outbox_pending_rows")
        .set(stats.approx_pending_rows.max(0) as f64);
    metrics::gauge!("ultros_clickhouse_sale_outbox_oldest_age_seconds")
        .set(stats.oldest_age_seconds.max(0.0));
}

fn reset_leader_metrics() {
    metrics::gauge!("ultros_clickhouse_sale_outbox_leader").set(0.0);
    metrics::gauge!("ultros_clickhouse_sale_outbox_pending_rows").set(0.0);
    metrics::gauge!("ultros_clickhouse_sale_outbox_oldest_age_seconds").set(0.0);
}

async fn run_with_advisory_lock(client: ClickHouseClient, db: UltrosDb, token: CancellationToken) {
    loop {
        if token.is_cancelled() {
            return;
        }
        let pool = db.get_connection().get_postgres_connection_pool();
        let mut connection = tokio::select! {
            _ = token.cancelled() => return,
            result = tokio::time::timeout(LOCK_OP_TIMEOUT, pool.acquire()) => match result {
                Ok(Ok(connection)) => connection,
                Ok(Err(error)) => {
                    warn!(?error, "could not acquire connection for ClickHouse sale outbox lease");
                    tokio::select! {
                        _ = token.cancelled() => return,
                        _ = tokio::time::sleep(LEASE_RETRY_INTERVAL) => {}
                    }
                    continue;
                }
                Err(_) => {
                    warn!("timed out acquiring connection for ClickHouse sale outbox lease");
                    tokio::select! {
                        _ = token.cancelled() => return,
                        _ = tokio::time::sleep(LEASE_RETRY_INTERVAL) => {}
                    }
                    continue;
                }
            }
        };

        // Mark `close_on_drop` BEFORE requesting the session advisory lock,
        // matching `SweepLockGuard` in `item_update_service`: an ambiguous
        // error, timeout, cancellation, or unwind can never return a still-
        // locked session back into the connection pool.
        connection.close_on_drop();

        let acquired = tokio::select! {
            _ = token.cancelled() => return,
            res = tokio::time::timeout(
                LOCK_OP_TIMEOUT,
                sea_orm::sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock($1)")
                    .bind(SALE_OUTBOX_LOCK_KEY)
                    .fetch_one(&mut *connection),
            ) => res,
        };

        match acquired {
            Ok(Ok(true)) => {
                info!("acquired ClickHouse sale outbox drainer lease");
                metrics::gauge!("ultros_clickhouse_sale_outbox_leader").set(1.0);
                let shutdown_token = token.child_token();
                let lease_token = CancellationToken::new();
                let drainer = run_pg_drainer(
                    client.clone(),
                    db.clone(),
                    shutdown_token.clone(),
                    lease_token.clone(),
                );
                tokio::pin!(drainer);
                let mut heartbeat = tokio::time::interval(LEASE_HEARTBEAT_INTERVAL);
                heartbeat.tick().await;
                let mut shutting_down = false;

                let (retry, lease_lost) = loop {
                    tokio::select! {
                        _ = token.cancelled(), if !shutting_down => {
                            // Signal graceful shutdown while keeping the
                            // heartbeat arm active during the bounded final
                            // drain so lease loss still aborts immediately.
                            shutting_down = true;
                            shutdown_token.cancel();
                        }
                        _ = &mut drainer => {
                            break (!shutting_down, false);
                        }
                        _ = heartbeat.tick() => {
                            let alive = tokio::time::timeout(
                                LEASE_HEARTBEAT_TIMEOUT,
                                sea_orm::sqlx::query_scalar::<_, i32>("SELECT 1")
                                    .fetch_one(&mut *connection),
                            )
                            .await;
                            match alive {
                                Ok(Ok(_)) => continue,
                                Ok(Err(error)) => warn!(
                                    ?error,
                                    "lost ClickHouse sale outbox lease connection"
                                ),
                                Err(_) => warn!(
                                    "timed out checking ClickHouse sale outbox lease"
                                ),
                            }
                            lease_token.cancel();
                            drainer.await;
                            break (!shutting_down, true);
                        }
                    }
                };

                reset_leader_metrics();
                if !lease_lost {
                    let unlocked = tokio::time::timeout(
                        UNLOCK_TIMEOUT,
                        sea_orm::sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1)")
                            .bind(SALE_OUTBOX_LOCK_KEY)
                            .fetch_one(&mut *connection),
                    )
                    .await;
                    if !matches!(unlocked, Ok(Ok(true))) {
                        warn!(
                            ?unlocked,
                            "could not cleanly release ClickHouse sale outbox advisory lock; session will close on drop"
                        );
                    }
                }
                if !retry {
                    return;
                }
            }
            Ok(Ok(false)) => {
                metrics::gauge!("ultros_clickhouse_sale_outbox_leader").set(0.0);
            }
            Ok(Err(error)) => {
                metrics::gauge!("ultros_clickhouse_sale_outbox_leader").set(0.0);
                warn!(
                    ?error,
                    "failed to check ClickHouse sale outbox advisory lock"
                );
            }
            Err(_) => {
                metrics::gauge!("ultros_clickhouse_sale_outbox_leader").set(0.0);
                warn!("timed out checking ClickHouse sale outbox advisory lock");
            }
        }
        drop(connection);
        tokio::select! {
            _ = token.cancelled() => return,
            _ = tokio::time::sleep(LEASE_RETRY_INTERVAL) => {}
        }
    }
}

async fn run_pg_drainer(
    client: ClickHouseClient,
    db: UltrosDb,
    shutdown_token: CancellationToken,
    lease_token: CancellationToken,
) {
    let fetch_db = db.clone();
    let ack_db = db.clone();
    let quarantine_db = db.clone();
    let metrics_db = db;
    run_outbox_loop(
        OutboxConfig::default(),
        shutdown_token,
        lease_token,
        move |limit| {
            let db = fetch_db.clone();
            Box::pin(async move { db.fetch_sale_outbox_batch(limit as i64).await })
        },
        move |rows| {
            let client = client.clone();
            Box::pin(async move { writer::flush::<SaleRow>(&client, rows).await })
        },
        move |ids| {
            let db = ack_db.clone();
            Box::pin(async move { db.ack_sale_outbox_ids(ids).await })
        },
        move |rows| {
            let db = quarantine_db.clone();
            let owned: Vec<(i64, String)> = rows.to_vec();
            Box::pin(async move { db.quarantine_sale_outbox_rows(&owned).await })
        },
        move || {
            let db = metrics_db.clone();
            Box::pin(async move { db.sale_outbox_metrics().await })
        },
    )
    .await;
}

#[derive(Clone, Copy, Debug)]
struct OutboxConfig {
    steady_batch_size: usize,
    backlog_batch_size: usize,
    flush_interval: Duration,
    insert_timeout: Duration,
    pg_op_timeout: Duration,
    drain_budget: Duration,
    initial_backoff: Duration,
    max_backoff: Duration,
    metrics_refresh_interval: Duration,
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            steady_batch_size: STEADY_BATCH_SIZE,
            backlog_batch_size: BACKLOG_BATCH_SIZE,
            flush_interval: DEFAULT_FLUSH_INTERVAL,
            insert_timeout: INSERT_TIMEOUT,
            pg_op_timeout: PG_OP_TIMEOUT,
            drain_budget: DRAIN_BUDGET,
            initial_backoff: INITIAL_BACKOFF,
            max_backoff: MAX_BACKOFF,
            metrics_refresh_interval: METRICS_REFRESH_INTERVAL,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepOutcome {
    Empty,
    PartialFlushed,
    FullBatchFlushed,
    Failed,
    LeaseLost,
}

fn next_backoff(current: Option<Duration>, config: OutboxConfig) -> Duration {
    match current {
        None => config.initial_backoff,
        Some(prev) => prev.saturating_mul(2).min(config.max_backoff),
    }
}

async fn run_cancellable<T, Fut>(
    lease_token: &CancellationToken,
    timeout: Duration,
    fut: Fut,
) -> Option<Result<T, tokio::time::error::Elapsed>>
where
    Fut: std::future::Future<Output = T>,
{
    tokio::select! {
        biased;
        _ = lease_token.cancelled() => None,
        res = tokio::time::timeout(timeout, fut) => Some(res),
    }
}

async fn drain_one_batch<Fetch, Insert, Ack, Quarantine>(
    config: OutboxConfig,
    batch_size: usize,
    lease_token: &CancellationToken,
    fetch: &mut Fetch,
    insert: &mut Insert,
    ack: &mut Ack,
    quarantine: &mut Quarantine,
) -> StepOutcome
where
    Fetch: FnMut(usize) -> BoxFuture<'static, anyhow::Result<Vec<SaleOutboxRow>>>,
    Insert: for<'a> FnMut(&'a [SaleRow]) -> BoxFuture<'a, Result<(), ClickHouseError>>,
    Ack: for<'a> FnMut(&'a [i64]) -> BoxFuture<'a, anyhow::Result<u64>>,
    Quarantine: for<'a> FnMut(&'a [(i64, String)]) -> BoxFuture<'a, anyhow::Result<u64>>,
{
    let outbox_rows = match run_cancellable(lease_token, config.pg_op_timeout, fetch(batch_size))
        .await
    {
        None => return StepOutcome::LeaseLost,
        Some(Ok(Ok(rows))) => rows,
        Some(result) => {
            metrics::counter!("ultros_clickhouse_sale_outbox_fetch_failures_total").increment(1);
            warn!(?result, "failed to read clickhouse_sale_outbox batch");
            return StepOutcome::Failed;
        }
    };

    if outbox_rows.is_empty() {
        metrics::gauge!("ultros_clickhouse_sale_outbox_pending_rows").set(0.0);
        metrics::gauge!("ultros_clickhouse_sale_outbox_oldest_age_seconds").set(0.0);
        return StepOutcome::Empty;
    }

    let oldest_age_secs = (Utc::now() - outbox_rows[0].created_at)
        .num_milliseconds()
        .max(0) as f64
        / 1000.0;
    metrics::gauge!("ultros_clickhouse_sale_outbox_oldest_age_seconds").set(oldest_age_secs);

    let fetched_count = outbox_rows.len();
    let mut valid_sales = Vec::with_capacity(fetched_count);
    let mut valid_ids = Vec::with_capacity(fetched_count);
    let mut invalid_rows = Vec::new();
    for row in &outbox_rows {
        match SaleRow::try_from_outbox_row(row) {
            Ok(sale) => {
                valid_sales.push(sale);
                valid_ids.push(row.id);
            }
            Err(reason) => {
                error!(
                    outbox_id = row.id,
                    pg_id = row.pg_id,
                    item_id = row.item_id,
                    world_id = row.world_id,
                    sold_date = %row.sold_date,
                    %reason,
                    "quarantining clickhouse_sale_outbox row that cannot be serialized to ClickHouse"
                );
                invalid_rows.push((row.id, reason));
            }
        }
    }

    if !invalid_rows.is_empty() {
        match run_cancellable(lease_token, config.pg_op_timeout, quarantine(&invalid_rows)).await {
            None => return StepOutcome::LeaseLost,
            Some(Ok(Ok(_))) => {
                metrics::counter!("ultros_clickhouse_sale_outbox_quarantined_rows_total")
                    .increment(invalid_rows.len() as u64);
            }
            Some(result) => {
                metrics::counter!("ultros_clickhouse_sale_outbox_quarantine_failures_total")
                    .increment(1);
                warn!(
                    ?result,
                    rows = invalid_rows.len(),
                    "failed to quarantine invalid clickhouse_sale_outbox rows"
                );
                return StepOutcome::Failed;
            }
        }
    }

    if !valid_sales.is_empty() {
        match run_cancellable(lease_token, config.insert_timeout, insert(&valid_sales)).await {
            None => return StepOutcome::LeaseLost,
            Some(Ok(Ok(()))) => {}
            Some(result) => {
                metrics::counter!("ultros_clickhouse_sale_outbox_flush_failures_total")
                    .increment(1);
                warn!(
                    ?result,
                    rows = valid_sales.len(),
                    "ClickHouse sale outbox insert failed; retaining outbox rows for retry"
                );
                return StepOutcome::Failed;
            }
        }

        match run_cancellable(lease_token, config.pg_op_timeout, ack(&valid_ids)).await {
            None => return StepOutcome::LeaseLost,
            Some(Ok(Ok(_))) => {
                metrics::counter!("ultros_clickhouse_sale_outbox_flushed_rows_total")
                    .increment(valid_sales.len() as u64);
                debug!(
                    rows = valid_sales.len(),
                    "flushed clickhouse_sale_outbox batch"
                );
            }
            Some(result) => {
                metrics::counter!("ultros_clickhouse_sale_outbox_ack_failures_total").increment(1);
                warn!(
                    ?result,
                    rows = valid_sales.len(),
                    "failed to acknowledge clickhouse_sale_outbox rows after ClickHouse insert; batch will replay idempotently"
                );
                return StepOutcome::Failed;
            }
        }
    }

    if fetched_count < batch_size {
        metrics::gauge!("ultros_clickhouse_sale_outbox_pending_rows").set(0.0);
        metrics::gauge!("ultros_clickhouse_sale_outbox_oldest_age_seconds").set(0.0);
        StepOutcome::PartialFlushed
    } else {
        StepOutcome::FullBatchFlushed
    }
}

/// Core outbox drain loop with injected storage/transport hooks for
/// deterministic outage, quarantine, backlog, replay, lease-loss, and shutdown
/// testing.
#[allow(clippy::too_many_arguments)]
async fn run_outbox_loop<Fetch, Insert, Ack, Quarantine, MetricsFn>(
    config: OutboxConfig,
    shutdown_token: CancellationToken,
    lease_token: CancellationToken,
    mut fetch: Fetch,
    mut insert: Insert,
    mut ack: Ack,
    mut quarantine: Quarantine,
    mut fetch_metrics: MetricsFn,
) where
    Fetch: FnMut(usize) -> BoxFuture<'static, anyhow::Result<Vec<SaleOutboxRow>>>,
    Insert: for<'a> FnMut(&'a [SaleRow]) -> BoxFuture<'a, Result<(), ClickHouseError>>,
    Ack: for<'a> FnMut(&'a [i64]) -> BoxFuture<'a, anyhow::Result<u64>>,
    Quarantine: for<'a> FnMut(&'a [(i64, String)]) -> BoxFuture<'a, anyhow::Result<u64>>,
    MetricsFn: FnMut() -> BoxFuture<'static, anyhow::Result<SaleOutboxMetrics>>,
{
    assert!(config.steady_batch_size > 0);
    assert!(config.backlog_batch_size >= config.steady_batch_size);

    let mut backoff: Option<Duration> = None;
    let mut last_metrics_at: Option<Instant> = None;
    let mut current_batch_size = config.steady_batch_size;

    loop {
        if lease_token.is_cancelled() || shutdown_token.is_cancelled() {
            break;
        }
        let outcome = drain_one_batch(
            config,
            current_batch_size,
            &lease_token,
            &mut fetch,
            &mut insert,
            &mut ack,
            &mut quarantine,
        )
        .await;
        match outcome {
            StepOutcome::LeaseLost => break,
            StepOutcome::FullBatchFlushed => {
                backoff = None;
                // Step up batch size while draining a backlog so ClickHouse
                // creates fewer parts during outage recovery.
                current_batch_size = config.backlog_batch_size;
                maybe_refresh_metrics(
                    config,
                    &lease_token,
                    &mut last_metrics_at,
                    &mut fetch_metrics,
                )
                .await;
                tokio::task::yield_now().await;
            }
            StepOutcome::Empty | StepOutcome::PartialFlushed => {
                backoff = None;
                current_batch_size = config.steady_batch_size;
                tokio::select! {
                    biased;
                    _ = lease_token.cancelled() => break,
                    _ = shutdown_token.cancelled() => break,
                    _ = tokio::time::sleep(config.flush_interval) => {}
                }
            }
            StepOutcome::Failed => {
                maybe_refresh_metrics(
                    config,
                    &lease_token,
                    &mut last_metrics_at,
                    &mut fetch_metrics,
                )
                .await;
                let delay = next_backoff(backoff, config);
                backoff = Some(delay);
                tokio::select! {
                    biased;
                    _ = lease_token.cancelled() => break,
                    _ = shutdown_token.cancelled() => break,
                    _ = tokio::time::sleep(delay) => {}
                }
            }
        }
    }

    // Only perform a bounded final drain on graceful shutdown while still
    // holding the advisory lock. Use steady_batch_size so shutdown stays
    // responsive within `drain_budget`.
    if !lease_token.is_cancelled() {
        let _ = tokio::time::timeout(config.drain_budget, async {
            loop {
                if lease_token.is_cancelled() {
                    break;
                }
                match drain_one_batch(
                    config,
                    config.steady_batch_size,
                    &lease_token,
                    &mut fetch,
                    &mut insert,
                    &mut ack,
                    &mut quarantine,
                )
                .await
                {
                    StepOutcome::FullBatchFlushed => continue,
                    StepOutcome::Empty
                    | StepOutcome::PartialFlushed
                    | StepOutcome::Failed
                    | StepOutcome::LeaseLost => break,
                }
            }
        })
        .await;
    }
    info!("ClickHouse sale outbox drainer exiting");
}

async fn maybe_refresh_metrics<MetricsFn>(
    config: OutboxConfig,
    lease_token: &CancellationToken,
    last_metrics_at: &mut Option<Instant>,
    fetch_metrics: &mut MetricsFn,
) where
    MetricsFn: FnMut() -> BoxFuture<'static, anyhow::Result<SaleOutboxMetrics>>,
{
    let now = Instant::now();
    if last_metrics_at
        .is_some_and(|prev| now.duration_since(prev) < config.metrics_refresh_interval)
    {
        return;
    }
    *last_metrics_at = Some(now);
    if let Some(Ok(Ok(stats))) =
        run_cancellable(lease_token, config.pg_op_timeout, fetch_metrics()).await
    {
        record_metrics(stats);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;
    use std::{collections::BTreeMap, sync::Mutex as StdMutex};

    fn outbox_row(id: i64, pg_id: i32) -> SaleOutboxRow {
        SaleOutboxRow {
            id,
            pg_id,
            sold_date: DateTime::from_timestamp(1_700_000_000 + id, 0)
                .unwrap()
                .naive_utc(),
            item_id: 42,
            hq: false,
            world_id: 54,
            price_per_item: 5000,
            quantity: 2,
            buying_character_id: 99,
            buyer_name: format!("Buyer-{id}"),
            created_at: Utc::now(),
        }
    }

    fn test_config() -> OutboxConfig {
        OutboxConfig {
            steady_batch_size: 2,
            backlog_batch_size: 4,
            flush_interval: Duration::from_secs(5),
            insert_timeout: Duration::from_secs(2),
            pg_op_timeout: Duration::from_secs(2),
            drain_budget: Duration::from_secs(3),
            initial_backoff: Duration::from_secs(5),
            max_backoff: Duration::from_secs(20),
            metrics_refresh_interval: Duration::from_secs(30),
        }
    }

    fn ok_metrics(
        store: Arc<StdMutex<BTreeMap<i64, SaleOutboxRow>>>,
    ) -> impl FnMut() -> BoxFuture<'static, anyhow::Result<SaleOutboxMetrics>> {
        move || {
            let s = store.clone();
            Box::pin(async move {
                let len = s.lock().unwrap().len() as i64;
                Ok(SaleOutboxMetrics {
                    approx_pending_rows: len,
                    oldest_age_seconds: 0.0,
                })
            })
        }
    }

    fn noop_quarantine()
    -> impl for<'a> FnMut(&'a [(i64, String)]) -> BoxFuture<'a, anyhow::Result<u64>> {
        move |_| Box::pin(async move { Ok(0) })
    }

    #[tokio::test(start_paused = true)]
    async fn drains_full_batches_back_to_back_then_idles() {
        let store = Arc::new(StdMutex::new(BTreeMap::from([
            (1, outbox_row(1, 101)),
            (2, outbox_row(2, 102)),
            (3, outbox_row(3, 103)),
        ])));
        let inserted = Arc::new(StdMutex::new(Vec::<Vec<i32>>::new()));
        let token = CancellationToken::new();

        let s_fetch = store.clone();
        let s_ack = store.clone();
        let ins = inserted.clone();
        let task = tokio::spawn(run_outbox_loop(
            test_config(),
            token.clone(),
            CancellationToken::new(),
            move |limit| {
                let s = s_fetch.clone();
                Box::pin(
                    async move { Ok(s.lock().unwrap().values().take(limit).cloned().collect()) },
                )
            },
            move |rows| {
                let ins = ins.clone();
                let ids: Vec<i32> = rows.iter().map(|r| r.pg_id).collect();
                Box::pin(async move {
                    ins.lock().unwrap().push(ids);
                    Ok(())
                })
            },
            move |ids| {
                let s = s_ack.clone();
                let ids = ids.to_vec();
                Box::pin(async move {
                    let mut map = s.lock().unwrap();
                    let mut removed = 0;
                    for id in ids {
                        if map.remove(&id).is_some() {
                            removed += 1;
                        }
                    }
                    Ok(removed)
                })
            },
            noop_quarantine(),
            ok_metrics(store.clone()),
        ));

        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(store.lock().unwrap().is_empty());
        assert_eq!(*inserted.lock().unwrap(), vec![vec![101, 102], vec![103]]);

        token.cancel();
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn backlog_steps_up_to_larger_batch_size_then_returns_to_steady_size() {
        let store = Arc::new(StdMutex::new(
            (1..=7)
                .map(|i| (i, outbox_row(i, 100 + i as i32)))
                .collect(),
        ));
        let requested_limits = Arc::new(StdMutex::new(Vec::<usize>::new()));
        let token = CancellationToken::new();

        let s_fetch = store.clone();
        let s_ack = store.clone();
        let limits = requested_limits.clone();
        let task = tokio::spawn(run_outbox_loop(
            test_config(),
            token.clone(),
            CancellationToken::new(),
            move |limit| {
                let s = s_fetch.clone();
                let limits = limits.clone();
                Box::pin(async move {
                    limits.lock().unwrap().push(limit);
                    Ok(s.lock().unwrap().values().take(limit).cloned().collect())
                })
            },
            move |_| Box::pin(async move { Ok(()) }),
            move |ids| {
                let s = s_ack.clone();
                let ids = ids.to_vec();
                Box::pin(async move {
                    let mut map = s.lock().unwrap();
                    for id in &ids {
                        map.remove(id);
                    }
                    Ok(ids.len() as u64)
                })
            },
            noop_quarantine(),
            ok_metrics(store.clone()),
        ));

        for _ in 0..12 {
            tokio::task::yield_now().await;
        }
        assert!(store.lock().unwrap().is_empty());
        // 2 (steady, full) -> 4 (backlog, full) -> 4 (backlog, partial of 1 -> back to steady)
        assert_eq!(*requested_limits.lock().unwrap(), vec![2, 4, 4]);

        // Next idle poll must be back to steady_batch_size (2).
        tokio::time::advance(Duration::from_secs(5)).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(*requested_limits.lock().unwrap(), vec![2, 4, 4, 2]);

        token.cancel();
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn poison_timestamp_row_is_quarantined_without_blocking_valid_sales() {
        let mut poison = outbox_row(1, 101);
        poison.sold_date = DateTime::from_timestamp(-86_400, 0).unwrap().naive_utc();
        let valid = outbox_row(2, 102);

        let store = Arc::new(StdMutex::new(BTreeMap::from([(1, poison), (2, valid)])));
        let quarantined = Arc::new(StdMutex::new(Vec::<(i64, String)>::new()));
        let delivered = Arc::new(StdMutex::new(Vec::<i32>::new()));
        let token = CancellationToken::new();

        let s_fetch = store.clone();
        let s_ack = store.clone();
        let s_quar = store.clone();
        let q_log = quarantined.clone();
        let del = delivered.clone();
        let task = tokio::spawn(run_outbox_loop(
            test_config(),
            token.clone(),
            CancellationToken::new(),
            move |limit| {
                let s = s_fetch.clone();
                Box::pin(
                    async move { Ok(s.lock().unwrap().values().take(limit).cloned().collect()) },
                )
            },
            move |rows| {
                let del = del.clone();
                let ids: Vec<i32> = rows.iter().map(|r| r.pg_id).collect();
                Box::pin(async move {
                    del.lock().unwrap().extend(ids);
                    Ok(())
                })
            },
            move |ids| {
                let s = s_ack.clone();
                let ids = ids.to_vec();
                Box::pin(async move {
                    let mut map = s.lock().unwrap();
                    for id in &ids {
                        map.remove(id);
                    }
                    Ok(ids.len() as u64)
                })
            },
            move |rows| {
                let s = s_quar.clone();
                let q_log = q_log.clone();
                let rows = rows.to_vec();
                Box::pin(async move {
                    let mut map = s.lock().unwrap();
                    for (id, reason) in &rows {
                        map.remove(id);
                        q_log.lock().unwrap().push((*id, reason.clone()));
                    }
                    Ok(rows.len() as u64)
                })
            },
            ok_metrics(store.clone()),
        ));

        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(store.lock().unwrap().is_empty());
        assert_eq!(*delivered.lock().unwrap(), vec![102]);
        {
            let q = quarantined.lock().unwrap();
            assert_eq!(q.len(), 1);
            assert_eq!(q[0].0, 1);
            assert!(q[0].1.contains("DateTime"));
        }

        token.cancel();
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn clickhouse_insert_failure_retains_outbox_rows_and_retries_with_backoff() {
        let store = Arc::new(StdMutex::new(BTreeMap::from([(
            1,
            outbox_row(1, i32::MIN),
        )])));
        let attempts = Arc::new(StdMutex::new(0usize));
        let delivered = Arc::new(StdMutex::new(Vec::<i32>::new()));
        let token = CancellationToken::new();

        let s_fetch = store.clone();
        let s_ack = store.clone();
        let att = attempts.clone();
        let del = delivered.clone();
        let task = tokio::spawn(run_outbox_loop(
            test_config(),
            token.clone(),
            CancellationToken::new(),
            move |limit| {
                let s = s_fetch.clone();
                Box::pin(
                    async move { Ok(s.lock().unwrap().values().take(limit).cloned().collect()) },
                )
            },
            move |rows| {
                let att = att.clone();
                let del = del.clone();
                let pg_ids: Vec<i32> = rows.iter().map(|r| r.pg_id).collect();
                Box::pin(async move {
                    let mut n = att.lock().unwrap();
                    *n += 1;
                    if *n <= 2 {
                        return Err(ClickHouseError::Backfill("simulated CH outage".into()));
                    }
                    del.lock().unwrap().extend(pg_ids);
                    Ok(())
                })
            },
            move |ids| {
                let s = s_ack.clone();
                let ids = ids.to_vec();
                Box::pin(async move {
                    let mut map = s.lock().unwrap();
                    for id in &ids {
                        map.remove(id);
                    }
                    Ok(ids.len() as u64)
                })
            },
            noop_quarantine(),
            ok_metrics(store.clone()),
        ));

        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(*attempts.lock().unwrap(), 1);
        assert_eq!(
            store.lock().unwrap().len(),
            1,
            "failed insert must not delete outbox row"
        );

        tokio::time::advance(Duration::from_secs(5)).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(*attempts.lock().unwrap(), 2);
        assert_eq!(store.lock().unwrap().len(), 1);

        tokio::time::advance(Duration::from_secs(5)).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(*attempts.lock().unwrap(), 2);

        tokio::time::advance(Duration::from_secs(5)).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(*attempts.lock().unwrap(), 3);
        assert!(store.lock().unwrap().is_empty());
        assert_eq!(*delivered.lock().unwrap(), vec![i32::MIN]);

        token.cancel();
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn ack_failure_after_insert_replays_batch_and_deletes_on_recovery() {
        let store = Arc::new(StdMutex::new(BTreeMap::from([(10, outbox_row(10, 500))])));
        let inserts = Arc::new(StdMutex::new(Vec::<Vec<i32>>::new()));
        let ack_calls = Arc::new(StdMutex::new(0usize));
        let token = CancellationToken::new();

        let s_fetch = store.clone();
        let s_ack = store.clone();
        let ins = inserts.clone();
        let ac = ack_calls.clone();
        let task = tokio::spawn(run_outbox_loop(
            test_config(),
            token.clone(),
            CancellationToken::new(),
            move |limit| {
                let s = s_fetch.clone();
                Box::pin(
                    async move { Ok(s.lock().unwrap().values().take(limit).cloned().collect()) },
                )
            },
            move |rows| {
                let ins = ins.clone();
                let ids: Vec<i32> = rows.iter().map(|r| r.pg_id).collect();
                Box::pin(async move {
                    ins.lock().unwrap().push(ids);
                    Ok(())
                })
            },
            move |ids| {
                let s = s_ack.clone();
                let ac = ac.clone();
                let ids = ids.to_vec();
                Box::pin(async move {
                    let mut n = ac.lock().unwrap();
                    *n += 1;
                    if *n == 1 {
                        anyhow::bail!("simulated PG ack failure");
                    }
                    let mut map = s.lock().unwrap();
                    for id in &ids {
                        map.remove(id);
                    }
                    Ok(ids.len() as u64)
                })
            },
            noop_quarantine(),
            ok_metrics(store.clone()),
        ));

        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(store.lock().unwrap().len(), 1);
        assert_eq!(*inserts.lock().unwrap(), vec![vec![500]]);

        tokio::time::advance(Duration::from_secs(5)).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(*inserts.lock().unwrap(), vec![vec![500], vec![500]]);
        assert!(store.lock().unwrap().is_empty());

        token.cancel();
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn out_of_order_lower_id_commit_is_never_skipped() {
        let store = Arc::new(StdMutex::new(BTreeMap::from([(2, outbox_row(2, 200))])));
        let delivered = Arc::new(StdMutex::new(Vec::<i32>::new()));
        let token = CancellationToken::new();

        let s_fetch = store.clone();
        let s_ack = store.clone();
        let del = delivered.clone();
        let task = tokio::spawn(run_outbox_loop(
            test_config(),
            token.clone(),
            CancellationToken::new(),
            move |limit| {
                let s = s_fetch.clone();
                Box::pin(
                    async move { Ok(s.lock().unwrap().values().take(limit).cloned().collect()) },
                )
            },
            move |rows| {
                let del = del.clone();
                let ids: Vec<i32> = rows.iter().map(|r| r.pg_id).collect();
                Box::pin(async move {
                    del.lock().unwrap().extend(ids);
                    Ok(())
                })
            },
            move |ids| {
                let s = s_ack.clone();
                let ids = ids.to_vec();
                Box::pin(async move {
                    let mut map = s.lock().unwrap();
                    for id in &ids {
                        map.remove(id);
                    }
                    Ok(ids.len() as u64)
                })
            },
            noop_quarantine(),
            ok_metrics(store.clone()),
        ));

        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(*delivered.lock().unwrap(), vec![200]);

        store.lock().unwrap().insert(1, outbox_row(1, 100));
        tokio::time::advance(Duration::from_secs(5)).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(*delivered.lock().unwrap(), vec![200, 100]);
        assert!(store.lock().unwrap().is_empty());

        token.cancel();
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_flushes_pending_rows_and_preserves_unflushed_rows_on_outage() {
        let store = Arc::new(StdMutex::new(BTreeMap::<i64, SaleOutboxRow>::new()));
        let delivered = Arc::new(StdMutex::new(Vec::<i32>::new()));
        let token = CancellationToken::new();

        let s_fetch = store.clone();
        let s_ack = store.clone();
        let del = delivered.clone();
        let task = tokio::spawn(run_outbox_loop(
            test_config(),
            token.clone(),
            CancellationToken::new(),
            move |limit| {
                let s = s_fetch.clone();
                Box::pin(
                    async move { Ok(s.lock().unwrap().values().take(limit).cloned().collect()) },
                )
            },
            move |rows| {
                let del = del.clone();
                let ids: Vec<i32> = rows.iter().map(|r| r.pg_id).collect();
                Box::pin(async move {
                    del.lock().unwrap().extend(ids);
                    Ok(())
                })
            },
            move |ids| {
                let s = s_ack.clone();
                let ids = ids.to_vec();
                Box::pin(async move {
                    let mut map = s.lock().unwrap();
                    for id in &ids {
                        map.remove(id);
                    }
                    Ok(ids.len() as u64)
                })
            },
            noop_quarantine(),
            ok_metrics(store.clone()),
        ));

        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        store.lock().unwrap().insert(7, outbox_row(7, 777));
        token.cancel();
        task.await.unwrap();
        assert_eq!(*delivered.lock().unwrap(), vec![777]);
        assert!(store.lock().unwrap().is_empty());

        let outage_store = Arc::new(StdMutex::new(BTreeMap::from([(8, outbox_row(8, 888))])));
        let outage_token = CancellationToken::new();
        outage_token.cancel();
        let s_fetch = outage_store.clone();
        let s_ack = outage_store.clone();
        run_outbox_loop(
            test_config(),
            outage_token,
            CancellationToken::new(),
            move |limit| {
                let s = s_fetch.clone();
                Box::pin(
                    async move { Ok(s.lock().unwrap().values().take(limit).cloned().collect()) },
                )
            },
            move |_| {
                Box::pin(
                    async move { Err(ClickHouseError::Backfill("CH down at shutdown".into())) },
                )
            },
            move |ids| {
                let s = s_ack.clone();
                let ids = ids.to_vec();
                Box::pin(async move {
                    let mut map = s.lock().unwrap();
                    for id in &ids {
                        map.remove(id);
                    }
                    Ok(ids.len() as u64)
                })
            },
            noop_quarantine(),
            ok_metrics(outage_store.clone()),
        )
        .await;
        assert_eq!(
            outage_store.lock().unwrap().len(),
            1,
            "unflushed rows must remain in Postgres outbox for next startup"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_respects_drain_budget_when_insert_hangs() {
        let store = Arc::new(StdMutex::new(BTreeMap::from([(1, outbox_row(1, 101))])));
        let mut config = test_config();
        config.insert_timeout = Duration::from_secs(30);
        config.drain_budget = Duration::from_secs(3);

        let shutdown_token = CancellationToken::new();
        shutdown_token.cancel();
        let s_fetch = store.clone();
        let s_ack = store.clone();
        let task = tokio::spawn(run_outbox_loop(
            config,
            shutdown_token,
            CancellationToken::new(),
            move |limit| {
                let s = s_fetch.clone();
                Box::pin(
                    async move { Ok(s.lock().unwrap().values().take(limit).cloned().collect()) },
                )
            },
            move |_| Box::pin(std::future::pending()),
            move |ids| {
                let s = s_ack.clone();
                let ids = ids.to_vec();
                Box::pin(async move {
                    let mut map = s.lock().unwrap();
                    for id in &ids {
                        map.remove(id);
                    }
                    Ok(ids.len() as u64)
                })
            },
            noop_quarantine(),
            ok_metrics(store.clone()),
        ));

        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert!(!task.is_finished());

        tokio::time::advance(Duration::from_secs(3)).await;
        task.await.unwrap();
        assert_eq!(
            store.lock().unwrap().len(),
            1,
            "timed-out final drain must leave unacknowledged rows in Postgres"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn lease_loss_exits_without_running_final_drain() {
        let store = Arc::new(StdMutex::new(BTreeMap::<i64, SaleOutboxRow>::new()));
        let delivered = Arc::new(StdMutex::new(Vec::<i32>::new()));
        let shutdown_token = CancellationToken::new();
        let lease_token = CancellationToken::new();

        let s_fetch = store.clone();
        let s_ack = store.clone();
        let del = delivered.clone();
        let task = tokio::spawn(run_outbox_loop(
            test_config(),
            shutdown_token,
            lease_token.clone(),
            move |limit| {
                let s = s_fetch.clone();
                Box::pin(
                    async move { Ok(s.lock().unwrap().values().take(limit).cloned().collect()) },
                )
            },
            move |rows| {
                let del = del.clone();
                let ids: Vec<i32> = rows.iter().map(|r| r.pg_id).collect();
                Box::pin(async move {
                    del.lock().unwrap().extend(ids);
                    Ok(())
                })
            },
            move |ids| {
                let s = s_ack.clone();
                let ids = ids.to_vec();
                Box::pin(async move {
                    let mut map = s.lock().unwrap();
                    for id in &ids {
                        map.remove(id);
                    }
                    Ok(ids.len() as u64)
                })
            },
            noop_quarantine(),
            ok_metrics(store.clone()),
        ));

        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        store.lock().unwrap().insert(9, outbox_row(9, 999));
        lease_token.cancel();
        task.await.unwrap();

        assert!(
            delivered.lock().unwrap().is_empty(),
            "losing the advisory lock must not trigger a final drain"
        );
        assert_eq!(store.lock().unwrap().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn lease_loss_cancels_in_flight_fetch_and_never_starts_insert() {
        let insert_called = Arc::new(StdMutex::new(false));
        let fetch_started = Arc::new(tokio::sync::Notify::new());
        let shutdown_token = CancellationToken::new();
        let lease_token = CancellationToken::new();

        let fs = fetch_started.clone();
        let ic = insert_called.clone();
        let task = tokio::spawn(run_outbox_loop(
            test_config(),
            shutdown_token,
            lease_token.clone(),
            move |_| {
                let fs = fs.clone();
                Box::pin(async move {
                    fs.notify_one();
                    std::future::pending::<anyhow::Result<Vec<SaleOutboxRow>>>().await
                })
            },
            move |_| {
                let ic = ic.clone();
                Box::pin(async move {
                    *ic.lock().unwrap() = true;
                    Ok(())
                })
            },
            move |_| Box::pin(async move { Ok(0) }),
            noop_quarantine(),
            move || Box::pin(async move { Ok(SaleOutboxMetrics::default()) }),
        ));

        fetch_started.notified().await;
        lease_token.cancel();
        task.await.unwrap();
        assert!(
            !*insert_called.lock().unwrap(),
            "lease loss during in-flight fetch must cancel immediately and never call ClickHouse insert"
        );
    }
}
