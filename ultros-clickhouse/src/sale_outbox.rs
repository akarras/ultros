//! Durable Postgres-outbox drainer for ClickHouse `sales`.
//!
//! Replaces the lossy in-memory broadcast-bus `Writer<SaleRow>` path: every
//! committed `sale_history` insert (`UltrosDb::update_sales`, covering both
//! Universalis WebSocket ingest and `UpdateService` catch-up) atomically
//! writes matching rows to Postgres `clickhouse_sale_outbox`.
//!
//! A single leader elected via Postgres session advisory lock drains committed
//! batches in `id ASC` order, flushes them into ClickHouse `sales`
//! (`ReplacingMergeTree(inserted_at)` keyed on
//! `(item_id, hq, world_id, sold_date, pg_id)`), and deletes the acknowledged
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
use tracing::{debug, info, warn};
use ultros_db::{UltrosDb, clickhouse_outbox::SaleOutboxRow};

use crate::{ClickHouseClient, ClickHouseError, rows::SaleRow, writer};

/// Stable Postgres advisory-lock key electing the one replica draining
/// `clickhouse_sale_outbox` into ClickHouse (`"SALEOUT"` in ASCII).
/// Distinct from `ROLLUP_SCHEDULER_LOCK_KEY` and `FULL_SWEEP_LOCK_KEY`.
const SALE_OUTBOX_LOCK_KEY: i64 = 0x0053_414c_454f_5554;

const DEFAULT_BATCH_SIZE: usize = 1000;
const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_secs(5);
const INSERT_TIMEOUT: Duration = Duration::from_secs(10);
const DRAIN_BUDGET: Duration = Duration::from_secs(5);
const INITIAL_BACKOFF: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const LEASE_RETRY_INTERVAL: Duration = Duration::from_secs(15);
const LEASE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const LEASE_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(5);
const PENDING_COUNT_INTERVAL: Duration = Duration::from_secs(30);

/// Handle to the background `clickhouse_sale_outbox` drainer.
#[derive(Clone)]
pub struct SaleOutboxDrainer {
    token: CancellationToken,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    ready: watch::Receiver<bool>,
}

impl SaleOutboxDrainer {
    /// Production startup: retry ClickHouse schema initialization on startup
    /// outages, elect a single drainer via Postgres advisory lock, and drain
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
            if !writer::initialize(&client, &worker_token).await {
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
    /// lock, and await task exit. Any rows not drained within [`DRAIN_BUDGET`]
    /// remain safely persisted in Postgres `clickhouse_sale_outbox`.
    pub async fn shutdown(&self) {
        self.token.cancel();
        let mut task = self.task.lock().await;
        if let Some(task) = task.take()
            && let Err(error) = task.await
        {
            warn!(
                ?error,
                "ClickHouse sale outbox drainer failed during shutdown"
            );
        }
    }
}

async fn run_with_advisory_lock(client: ClickHouseClient, db: UltrosDb, token: CancellationToken) {
    loop {
        if token.is_cancelled() {
            return;
        }
        let pool = db.get_connection().get_postgres_connection_pool();
        let mut connection = tokio::select! {
            _ = token.cancelled() => return,
            result = pool.acquire() => match result {
                Ok(connection) => connection,
                Err(error) => {
                    warn!(?error, "could not acquire connection for ClickHouse sale outbox lease");
                    tokio::select! {
                        _ = token.cancelled() => return,
                        _ = tokio::time::sleep(LEASE_RETRY_INTERVAL) => {}
                    }
                    continue;
                }
            }
        };

        let acquired = sea_orm::sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock($1)")
            .bind(SALE_OUTBOX_LOCK_KEY)
            .fetch_one(&mut *connection)
            .await;

        match acquired {
            Ok(true) => {
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

                let (retry, lease_lost) = loop {
                    tokio::select! {
                        _ = token.cancelled() => {
                            shutdown_token.cancel();
                            drainer.await;
                            break (false, false);
                        }
                        _ = &mut drainer => {
                            break (true, false);
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
                            // Signal lease loss (not graceful shutdown) so the
                            // loop stops immediately without starting a final
                            // drain that could race a new leader.
                            lease_token.cancel();
                            drainer.await;
                            break (true, true);
                        }
                    }
                };

                metrics::gauge!("ultros_clickhouse_sale_outbox_leader").set(0.0);
                if lease_lost {
                    // Never return a broken or potentially still-locked session
                    // to the connection pool.
                    connection.close_on_drop();
                } else {
                    let unlocked =
                        sea_orm::sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1)")
                            .bind(SALE_OUTBOX_LOCK_KEY)
                            .fetch_one(&mut *connection)
                            .await;
                    if !matches!(unlocked, Ok(true)) {
                        warn!(
                            ?unlocked,
                            "could not cleanly release ClickHouse sale outbox advisory lock; closing session"
                        );
                        connection.close_on_drop();
                    }
                }
                if !retry {
                    return;
                }
            }
            Ok(false) => {
                metrics::gauge!("ultros_clickhouse_sale_outbox_leader").set(0.0);
            }
            Err(error) => {
                metrics::gauge!("ultros_clickhouse_sale_outbox_leader").set(0.0);
                warn!(
                    ?error,
                    "failed to check ClickHouse sale outbox advisory lock"
                );
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
    let count_db = db;
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
        move || {
            let db = count_db.clone();
            Box::pin(async move { db.count_sale_outbox_pending().await })
        },
    )
    .await;
}

#[derive(Clone, Copy, Debug)]
struct OutboxConfig {
    batch_size: usize,
    flush_interval: Duration,
    insert_timeout: Duration,
    drain_budget: Duration,
    initial_backoff: Duration,
    max_backoff: Duration,
    pending_count_interval: Duration,
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            batch_size: DEFAULT_BATCH_SIZE,
            flush_interval: DEFAULT_FLUSH_INTERVAL,
            insert_timeout: INSERT_TIMEOUT,
            drain_budget: DRAIN_BUDGET,
            initial_backoff: INITIAL_BACKOFF,
            max_backoff: MAX_BACKOFF,
            pending_count_interval: PENDING_COUNT_INTERVAL,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepOutcome {
    Empty,
    PartialFlushed,
    FullBatchFlushed,
    Failed,
}

fn next_backoff(current: Option<Duration>, config: OutboxConfig) -> Duration {
    match current {
        None => config.initial_backoff,
        Some(prev) => prev.saturating_mul(2).min(config.max_backoff),
    }
}

async fn drain_one_batch<Fetch, Insert, Ack>(
    config: OutboxConfig,
    fetch: &mut Fetch,
    insert: &mut Insert,
    ack: &mut Ack,
) -> StepOutcome
where
    Fetch: FnMut(usize) -> BoxFuture<'static, anyhow::Result<Vec<SaleOutboxRow>>>,
    Insert: for<'a> FnMut(&'a [SaleRow]) -> BoxFuture<'a, Result<(), ClickHouseError>>,
    Ack: for<'a> FnMut(&'a [i64]) -> BoxFuture<'a, anyhow::Result<u64>>,
{
    let outbox_rows = match fetch(config.batch_size).await {
        Ok(rows) => rows,
        Err(error) => {
            metrics::counter!("ultros_clickhouse_sale_outbox_fetch_failures_total").increment(1);
            warn!(?error, "failed to read clickhouse_sale_outbox batch");
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

    let count = outbox_rows.len();
    let sale_rows: Vec<SaleRow> = outbox_rows.iter().map(SaleRow::from_outbox_row).collect();
    let ids: Vec<i64> = outbox_rows.iter().map(|r| r.id).collect();

    match tokio::time::timeout(config.insert_timeout, insert(&sale_rows)).await {
        Ok(Ok(())) => {}
        result => {
            metrics::counter!("ultros_clickhouse_sale_outbox_flush_failures_total").increment(1);
            warn!(
                ?result,
                rows = count,
                "ClickHouse sale outbox insert failed; retaining outbox rows for retry"
            );
            return StepOutcome::Failed;
        }
    }

    match ack(&ids).await {
        Ok(_) => {
            metrics::counter!("ultros_clickhouse_sale_outbox_flushed_rows_total")
                .increment(count as u64);
            debug!(rows = count, "flushed clickhouse_sale_outbox batch");
            if count < config.batch_size {
                metrics::gauge!("ultros_clickhouse_sale_outbox_pending_rows").set(0.0);
                metrics::gauge!("ultros_clickhouse_sale_outbox_oldest_age_seconds").set(0.0);
                StepOutcome::PartialFlushed
            } else {
                StepOutcome::FullBatchFlushed
            }
        }
        Err(error) => {
            metrics::counter!("ultros_clickhouse_sale_outbox_ack_failures_total").increment(1);
            warn!(
                ?error,
                rows = count,
                "failed to acknowledge clickhouse_sale_outbox rows after ClickHouse insert; batch will replay idempotently"
            );
            StepOutcome::Failed
        }
    }
}

/// Core outbox drain loop with injected storage/transport hooks for
/// deterministic outage, replay, lease-loss, and shutdown testing.
async fn run_outbox_loop<Fetch, Insert, Ack, Count>(
    config: OutboxConfig,
    shutdown_token: CancellationToken,
    lease_token: CancellationToken,
    mut fetch: Fetch,
    mut insert: Insert,
    mut ack: Ack,
    mut count_pending: Count,
) where
    Fetch: FnMut(usize) -> BoxFuture<'static, anyhow::Result<Vec<SaleOutboxRow>>>,
    Insert: for<'a> FnMut(&'a [SaleRow]) -> BoxFuture<'a, Result<(), ClickHouseError>>,
    Ack: for<'a> FnMut(&'a [i64]) -> BoxFuture<'a, anyhow::Result<u64>>,
    Count: FnMut() -> BoxFuture<'static, anyhow::Result<u64>>,
{
    assert!(config.batch_size > 0);
    let mut backoff: Option<Duration> = None;
    let mut last_count_at: Option<Instant> = None;

    loop {
        if lease_token.is_cancelled() || shutdown_token.is_cancelled() {
            break;
        }
        let outcome = drain_one_batch(config, &mut fetch, &mut insert, &mut ack).await;
        match outcome {
            StepOutcome::FullBatchFlushed => {
                backoff = None;
                maybe_refresh_pending_count(config, &mut last_count_at, &mut count_pending).await;
                // Drain backlog immediately in full batches without sleeping
                // between parts, yielding so cancellation stays responsive.
                tokio::task::yield_now().await;
            }
            StepOutcome::Empty | StepOutcome::PartialFlushed => {
                backoff = None;
                tokio::select! {
                    biased;
                    _ = lease_token.cancelled() => break,
                    _ = shutdown_token.cancelled() => break,
                    _ = tokio::time::sleep(config.flush_interval) => {}
                }
            }
            StepOutcome::Failed => {
                maybe_refresh_pending_count(config, &mut last_count_at, &mut count_pending).await;
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

    // Only perform a final drain on graceful shutdown while still holding the
    // advisory lock. If the lease was lost, stop immediately so we do not race
    // another replica taking over leadership.
    if !lease_token.is_cancelled() {
        let _ = tokio::time::timeout(config.drain_budget, async {
            loop {
                if lease_token.is_cancelled() {
                    break;
                }
                match drain_one_batch(config, &mut fetch, &mut insert, &mut ack).await {
                    StepOutcome::FullBatchFlushed => continue,
                    StepOutcome::Empty | StepOutcome::PartialFlushed | StepOutcome::Failed => break,
                }
            }
        })
        .await;
    }
    info!("ClickHouse sale outbox drainer exiting");
}

async fn maybe_refresh_pending_count<Count>(
    config: OutboxConfig,
    last_count_at: &mut Option<Instant>,
    count_pending: &mut Count,
) where
    Count: FnMut() -> BoxFuture<'static, anyhow::Result<u64>>,
{
    let now = Instant::now();
    if last_count_at.is_some_and(|prev| now.duration_since(prev) < config.pending_count_interval) {
        return;
    }
    *last_count_at = Some(now);
    if let Ok(pending) = count_pending().await {
        metrics::gauge!("ultros_clickhouse_sale_outbox_pending_rows").set(pending as f64);
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
            batch_size: 2,
            flush_interval: Duration::from_secs(5),
            insert_timeout: Duration::from_secs(2),
            drain_budget: Duration::from_secs(3),
            initial_backoff: Duration::from_secs(5),
            max_backoff: Duration::from_secs(20),
            pending_count_interval: Duration::from_secs(30),
        }
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
        let s_count = store.clone();
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
            move || {
                let s = s_count.clone();
                Box::pin(async move { Ok(s.lock().unwrap().len() as u64) })
            },
        ));

        // Yield without advancing the clock: full batch [101, 102] and partial
        // [103] must both drain immediately without waiting for flush_interval.
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(store.lock().unwrap().is_empty());
        assert_eq!(*inserted.lock().unwrap(), vec![vec![101, 102], vec![103]]);

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
        let s_count = store.clone();
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
            move || {
                let s = s_count.clone();
                Box::pin(async move { Ok(s.lock().unwrap().len() as u64) })
            },
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

        // Initial backoff is 5s -> second failure engages 10s backoff.
        tokio::time::advance(Duration::from_secs(5)).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(*attempts.lock().unwrap(), 2);
        assert_eq!(store.lock().unwrap().len(), 1);

        // At +5s (only 5s of 10s backoff elapsed), must not have retried yet.
        tokio::time::advance(Duration::from_secs(5)).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(*attempts.lock().unwrap(), 2);

        // After remaining 5s (total 10s backoff), third attempt succeeds and deletes.
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
        let s_count = store.clone();
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
            move || {
                let s = s_count.clone();
                Box::pin(async move { Ok(s.lock().unwrap().len() as u64) })
            },
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
        // Replayed cleanly and then acknowledged.
        assert_eq!(*inserts.lock().unwrap(), vec![vec![500], vec![500]]);
        assert!(store.lock().unwrap().is_empty());

        token.cancel();
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn out_of_order_lower_id_commit_is_never_skipped() {
        // Simulates tx B (id=2) committing before tx A (id=1): tx B is drained
        // and deleted first, then tx A becomes visible and is picked up on the
        // next poll because acknowledgement deletes exact IDs rather than
        // advancing a high-water mark.
        let store = Arc::new(StdMutex::new(BTreeMap::from([(2, outbox_row(2, 200))])));
        let delivered = Arc::new(StdMutex::new(Vec::<i32>::new()));
        let token = CancellationToken::new();

        let s_fetch = store.clone();
        let s_ack = store.clone();
        let s_count = store.clone();
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
            move || {
                let s = s_count.clone();
                Box::pin(async move { Ok(s.lock().unwrap().len() as u64) })
            },
        ));

        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(*delivered.lock().unwrap(), vec![200]);

        // Late-committing lower outbox ID becomes visible.
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
        // Part 1: healthy shutdown flushes rows that arrived during idle sleep.
        let store = Arc::new(StdMutex::new(BTreeMap::<i64, SaleOutboxRow>::new()));
        let delivered = Arc::new(StdMutex::new(Vec::<i32>::new()));
        let token = CancellationToken::new();

        let s_fetch = store.clone();
        let s_ack = store.clone();
        let s_count = store.clone();
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
            move || {
                let s = s_count.clone();
                Box::pin(async move { Ok(s.lock().unwrap().len() as u64) })
            },
        ));

        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        // Row commits while drainer is idle; then shutdown fires immediately.
        store.lock().unwrap().insert(7, outbox_row(7, 777));
        token.cancel();
        task.await.unwrap();
        assert_eq!(*delivered.lock().unwrap(), vec![777]);
        assert!(store.lock().unwrap().is_empty());

        // Part 2: ClickHouse outage during shutdown leaves rows safely in PG.
        let outage_store = Arc::new(StdMutex::new(BTreeMap::from([(8, outbox_row(8, 888))])));
        let outage_token = CancellationToken::new();
        outage_token.cancel();
        let s_fetch = outage_store.clone();
        let s_ack = outage_store.clone();
        let s_count = outage_store.clone();
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
            move || {
                let s = s_count.clone();
                Box::pin(async move { Ok(s.lock().unwrap().len() as u64) })
            },
        )
        .await;
        assert_eq!(
            outage_store.lock().unwrap().len(),
            1,
            "unflushed rows must remain in Postgres outbox for next startup"
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
        let s_count = store.clone();
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
            move || {
                let s = s_count.clone();
                Box::pin(async move { Ok(s.lock().unwrap().len() as u64) })
            },
        ));

        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        // Row commits while idle; then leadership is lost instead of graceful shutdown.
        store.lock().unwrap().insert(9, outbox_row(9, 999));
        lease_token.cancel();
        task.await.unwrap();

        assert!(
            delivered.lock().unwrap().is_empty(),
            "losing the advisory lock must not trigger a final drain"
        );
        assert_eq!(store.lock().unwrap().len(), 1);
    }
}
