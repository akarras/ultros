//! Bounded, retrying row writer. Postgres remains the source of truth.
//!
//! One `Writer<R>` per table: `sales` (analyzer dual-write), `listing_events`
//! (every listing ingest path) and `floor_changes` (analyzer floor moves).
//! They share this queue/retry/drain implementation and differ only in the
//! row type, whose [`TableRow::TABLE`] names the destination.
//!
//! A failed insert retains its complete batch until an acknowledged retry.
//! Retrying an ambiguous response is safe for `sales` because its
//! ReplacingMergeTree uses the same Postgres id; readers must use FINAL until
//! merges complete. The append-only tables can double a batch on an ambiguous
//! retry; their readers tolerate duplicates.
//! The queue is deliberately bounded: overflow, process crashes, and event-bus
//! lag still require a Postgres backfill (for `sales`) or are simply lost (for
//! the listing tables, which Postgres cannot reconstruct). This is not a
//! durable replication log.

use std::{sync::Arc, time::Duration};

use futures::future::BoxFuture;
use tokio::{
    sync::{Mutex, mpsc, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::{ClickHouseClient, ClickHouseError, rows::TableRow};

const DEFAULT_BATCH_SIZE: usize = 1000;
const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_secs(5);
const QUEUE_CAPACITY: usize = 10_000;
const INSERT_TIMEOUT: Duration = Duration::from_secs(10);
const MIGRATION_RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// Cheap handle to the bounded writer. Clones share the task and shutdown.
pub struct Writer<R: TableRow> {
    tx: mpsc::Sender<R>,
    token: CancellationToken,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    ready: watch::Receiver<bool>,
}

// Manual impl: a derive would demand `R: Clone`, which rows need not be.
impl<R: TableRow> Clone for Writer<R> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            token: self.token.clone(),
            task: self.task.clone(),
            ready: self.ready.clone(),
        }
    }
}

impl<R: TableRow> Writer<R> {
    /// Spawn after the caller has applied the schema (primarily useful in tests).
    pub fn spawn(client: ClickHouseClient, token: CancellationToken) -> Self {
        Self::spawn_with_config(client, token, DEFAULT_BATCH_SIZE, DEFAULT_FLUSH_INTERVAL)
    }

    /// Production startup: retry schema initialization after a temporary outage.
    /// Rows accumulate only up to QUEUE_CAPACITY while ClickHouse is unavailable.
    pub fn spawn_recovering(client: ClickHouseClient, token: CancellationToken) -> Self {
        Self::spawn_inner(
            client,
            token,
            DEFAULT_BATCH_SIZE,
            DEFAULT_FLUSH_INTERVAL,
            true,
        )
    }

    pub fn spawn_with_config(
        client: ClickHouseClient,
        token: CancellationToken,
        batch_size: usize,
        flush_interval: Duration,
    ) -> Self {
        Self::spawn_inner(client, token, batch_size, flush_interval, false)
    }

    fn spawn_inner(
        client: ClickHouseClient,
        token: CancellationToken,
        batch_size: usize,
        flush_interval: Duration,
        migrate: bool,
    ) -> Self {
        assert!(batch_size > 0);
        assert!(!flush_interval.is_zero());
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        let (ready_tx, ready) = watch::channel(false);
        let token = token.child_token();
        let worker_token = token.clone();
        let task = tokio::spawn(async move {
            if migrate && !initialize(&client, &worker_token).await {
                record_unflushed(rx.len(), R::TABLE);
                return;
            }
            ready_tx.send_replace(true);
            run_writer(rx, worker_token, batch_size, flush_interval, move |rows| {
                let client = client.clone();
                Box::pin(async move { flush::<R>(&client, rows).await })
            })
            .await;
        });
        Self {
            tx,
            token,
            task: Arc::new(Mutex::new(Some(task))),
            ready,
        }
    }

    /// Wait until initialization succeeds; false means the worker exited first.
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

    /// Never back-pressure the producer. Overflow is observable and requires
    /// reconciliation from Postgres, just like sales lost by the broadcast bus.
    pub fn send(&self, row: R) {
        if let Err(error) = self.tx.try_send(row) {
            let reason = match error {
                mpsc::error::TrySendError::Full(_) => "queue_full",
                mpsc::error::TrySendError::Closed(_) => "queue_closed",
            };
            metrics::counter!(
                "ultros_clickhouse_writer_dropped_rows_total",
                "reason" => reason,
                "table" => R::TABLE
            )
            .increment(1);
        }
    }

    /// Stop accepting rows, flush queued batches, and await task completion.
    /// Call only after producers have stopped. Each insert has a timeout; a
    /// final failed insert is counted and logged for operator reconciliation.
    pub async fn shutdown(&self) {
        self.token.cancel();
        // Keep the lock while joining so concurrent callers also wait.
        let mut task = self.task.lock().await;
        if let Some(task) = task.take()
            && let Err(error) = task.await
        {
            warn!(
                ?error,
                table = R::TABLE,
                "ClickHouse writer task failed during shutdown"
            );
        }
    }

    /// Test fixture with no task or ClickHouse dependency.
    pub fn disabled() -> Self {
        let (tx, _rx) = mpsc::channel(1);
        let (_ready_tx, ready) = watch::channel(false);
        Self {
            tx,
            token: CancellationToken::new(),
            task: Arc::new(Mutex::new(None)),
            ready,
        }
    }
}

async fn initialize(client: &ClickHouseClient, token: &CancellationToken) -> bool {
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
        tokio::select! {
            _ = token.cancelled() => return false,
            _ = tokio::time::sleep(MIGRATION_RETRY_INTERVAL) => {}
        }
    }
}

/// Separate transport from the queue loop so outage behavior can be tested
/// without a running database. The callback borrows rows; failures cannot
/// consume or partially remove the retry batch.
async fn run_writer<R, F>(
    mut rx: mpsc::Receiver<R>,
    token: CancellationToken,
    batch_size: usize,
    flush_interval: Duration,
    mut insert: F,
) where
    R: TableRow,
    F: for<'a> FnMut(&'a [R]) -> BoxFuture<'a, Result<(), ClickHouseError>>,
{
    let mut buf = Vec::with_capacity(batch_size);
    let mut interval = tokio::time::interval(flush_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // First interval flush follows a full interval, including in tests.
    interval.tick().await;
    let mut retrying = false;
    loop {
        let stopping = tokio::select! {
            biased;
            _ = token.cancelled() => true,
            row = rx.recv(), if buf.len() < batch_size => {
                match row {
                    Some(row) => {
                        buf.push(row);
                        if buf.len() < batch_size || retrying {
                            continue;
                        }
                        false
                    }
                    None => true,
                }
            }
            _ = interval.tick() => false,
        };
        metrics::gauge!("ultros_clickhouse_writer_queued_rows", "table" => R::TABLE)
            .set(rx.len() as f64);
        if stopping {
            // Closing first prevents concurrent producers extending the drain.
            rx.close();
            loop {
                while buf.len() < batch_size {
                    match rx.try_recv() {
                        Ok(row) => buf.push(row),
                        Err(_) => break,
                    }
                }
                if buf.is_empty() {
                    break;
                }
                if !try_flush(&mut buf, &mut insert).await {
                    record_unflushed(buf.len() + rx.len(), R::TABLE);
                    break;
                }
            }
            break;
        }
        if !buf.is_empty() {
            retrying = !try_flush(&mut buf, &mut insert).await;
        }
    }
    metrics::gauge!("ultros_clickhouse_writer_queued_rows", "table" => R::TABLE).set(0.0);
    info!(table = R::TABLE, "ClickHouse writer task exiting");
}

async fn try_flush<R, F>(buf: &mut Vec<R>, insert: &mut F) -> bool
where
    R: TableRow,
    F: for<'a> FnMut(&'a [R]) -> BoxFuture<'a, Result<(), ClickHouseError>>,
{
    match tokio::time::timeout(INSERT_TIMEOUT, insert(buf)).await {
        Ok(Ok(())) => {
            metrics::counter!("ultros_clickhouse_writer_written_rows_total", "table" => R::TABLE)
                .increment(buf.len() as u64);
            buf.clear();
            true
        }
        result => {
            metrics::counter!("ultros_clickhouse_writer_flush_failures_total", "table" => R::TABLE)
                .increment(1);
            warn!(
                ?result,
                rows = buf.len(),
                table = R::TABLE,
                "ClickHouse insert failed; retaining complete batch for retry"
            );
            false
        }
    }
}

fn record_unflushed(rows: usize, table: &'static str) {
    if rows != 0 {
        metrics::counter!(
            "ultros_clickhouse_writer_dropped_rows_total",
            "reason" => "shutdown_unflushed",
            "table" => table
        )
        .increment(rows as u64);
        warn!(
            rows,
            table, "ClickHouse shutdown left unflushed rows; Postgres backfill required"
        );
    }
}

async fn flush<R: TableRow>(client: &ClickHouseClient, rows: &[R]) -> Result<(), ClickHouseError> {
    let mut insert = client.client().insert::<R>(R::TABLE).await?;
    for row in rows {
        insert.write(row).await?;
    }
    insert.end().await?;
    debug!(rows = rows.len(), table = R::TABLE, "ClickHouse flush");
    Ok(())
}

/// One-shot bulk insert for producers that bypass the queue (the seed, the
/// analyzer's resync diff). Each chunk is one INSERT, so a failure loses at
/// most `chunk` rows and the caller decides whether to retry.
pub async fn insert_all<R: TableRow>(
    client: &ClickHouseClient,
    rows: &[R],
    chunk: usize,
) -> Result<u64, ClickHouseError> {
    let mut written = 0u64;
    for part in rows.chunks(chunk.max(1)) {
        flush(client, part).await?;
        written += part.len() as u64;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rows::SaleRow;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::{Notify, Semaphore};

    fn row(pg_id: i32) -> SaleRow {
        SaleRow {
            pg_id,
            sold_date: chrono::DateTime::UNIX_EPOCH,
            item_id: 1,
            hq: 0,
            world_id: 1,
            price_per_item: 100,
            quantity: 1,
            buying_character_id: 1,
            buyer_name: String::new(),
        }
    }

    #[tokio::test]
    async fn outage_retains_complete_batch_and_bounds_pending_rows() {
        let (tx, rx) = mpsc::channel(2);
        let (attempt_tx, mut attempts) = mpsc::unbounded_channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let acknowledge_retry = Arc::new(Semaphore::new(0));
        let retry_gate = acknowledge_retry.clone();
        let token = CancellationToken::new();
        let task = tokio::spawn(run_writer(
            rx,
            token.clone(),
            2,
            Duration::from_millis(50),
            move |rows: &[SaleRow]| {
                let ids = rows.iter().map(|r| r.pg_id).collect::<Vec<_>>();
                attempt_tx.send(ids).unwrap();
                let attempt = calls.fetch_add(1, Ordering::SeqCst);
                let retry_gate = retry_gate.clone();
                Box::pin(async move {
                    if attempt == 0 {
                        Err(ClickHouseError::Backfill("simulated failed insert".into()))
                    } else {
                        if attempt == 1 {
                            retry_gate.acquire().await.unwrap().forget();
                        }
                        Ok(())
                    }
                })
            },
        ));
        tx.send(row(1)).await.unwrap();
        tx.send(row(2)).await.unwrap();
        assert_eq!(attempts.recv().await.unwrap(), vec![1, 2]);
        // Failed batch occupies the entire buffer: the worker must stop
        // consuming, leaving only the bounded channel for new events.
        tx.try_send(row(3)).unwrap();
        tx.try_send(row(4)).unwrap();
        assert!(matches!(
            tx.try_send(row(5)),
            Err(mpsc::error::TrySendError::Full(_))
        ));
        acknowledge_retry.add_permits(1);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), attempts.recv())
                .await
                .unwrap()
                .unwrap(),
            vec![1, 2],
            "retry must include even rows written before the failed acknowledgement",
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), attempts.recv())
                .await
                .unwrap()
                .unwrap(),
            vec![3, 4],
        );
        token.cancel();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_drains_queue_in_bounded_batches() {
        let (tx, rx) = mpsc::channel(7);
        for id in 1..=7 {
            tx.try_send(row(id)).unwrap();
        }
        let (batch_tx, mut batches) = mpsc::unbounded_channel();
        let token = CancellationToken::new();
        token.cancel();
        run_writer(
            rx,
            token,
            3,
            Duration::from_secs(60),
            move |rows: &[SaleRow]| {
                batch_tx
                    .send(rows.iter().map(|r| r.pg_id).collect::<Vec<_>>())
                    .unwrap();
                Box::pin(async { Ok(()) })
            },
        )
        .await;
        assert_eq!(batches.recv().await.unwrap(), vec![1, 2, 3]);
        assert_eq!(batches.recv().await.unwrap(), vec![4, 5, 6]);
        assert_eq!(batches.recv().await.unwrap(), vec![7]);
        assert!(tx.is_closed());
    }

    #[tokio::test]
    async fn shutdown_waits_for_in_flight_acknowledgement() {
        let (tx, rx) = mpsc::channel(2);
        let token = CancellationToken::new();
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Semaphore::new(0));
        let started = entered.clone();
        let proceed = release.clone();
        let task = tokio::spawn(run_writer(
            rx,
            token.clone(),
            1,
            Duration::from_secs(60),
            move |_: &[SaleRow]| {
                let started = started.clone();
                let proceed = proceed.clone();
                Box::pin(async move {
                    started.notify_one();
                    proceed.acquire().await.unwrap().forget();
                    Ok(())
                })
            },
        ));
        let (_ready_tx, ready) = watch::channel(true);
        let writer = Writer {
            tx,
            token,
            task: Arc::new(Mutex::new(Some(task))),
            ready,
        };
        writer.send(row(1));
        tokio::time::timeout(Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        let shutdown = tokio::spawn(async move { writer.shutdown().await });
        tokio::task::yield_now().await;
        assert!(
            !shutdown.is_finished(),
            "shutdown returned before the insert completed"
        );
        release.add_permits(1);
        tokio::time::timeout(Duration::from_secs(2), shutdown)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn readiness_wait_returns_false_when_initialization_exits() {
        assert!(!Writer::<SaleRow>::disabled().wait_ready().await);
    }

    #[tokio::test]
    async fn run_writer_is_generic_over_the_row_type() {
        use crate::rows::{FloorChangeReason, FloorChangeRow};
        let (tx, rx) = mpsc::channel(4);
        let (batch_tx, mut batches) = mpsc::unbounded_channel();
        let token = CancellationToken::new();
        for i in 0..3 {
            tx.try_send(FloorChangeRow::new(
                chrono::Utc::now(),
                i,
                false,
                40,
                100 + i,
                FloorChangeReason::Listing,
            ))
            .unwrap();
        }
        token.cancel();
        run_writer(
            rx,
            token,
            2,
            Duration::from_secs(60),
            move |rows: &[FloorChangeRow]| {
                batch_tx
                    .send(rows.iter().map(|r| r.item_id).collect::<Vec<_>>())
                    .unwrap();
                Box::pin(async { Ok(()) })
            },
        )
        .await;
        assert_eq!(batches.recv().await.unwrap(), vec![0, 1]);
        assert_eq!(batches.recv().await.unwrap(), vec![2]);
    }
}
