//! Buffered batch writer for sale rows.
//!
//! Owns a tokio task that pulls from an unbounded mpsc channel, batches rows
//! into ClickHouse `Insert` streams, and flushes either when the batch fills
//! (`DEFAULT_BATCH_SIZE`) or every `DEFAULT_FLUSH_INTERVAL`. On cancellation,
//! drains any remaining buffered rows so we don't lose what we've collected.
//!
//! ## Crash safety
//!
//! Postgres remains the source of truth for sale history. If this writer drops
//! rows for any reason (channel overflow, ClickHouse outage, panic), the gap
//! is recoverable by re-running [`crate::backfill::backfill_sales`] for the
//! affected `(world_id, year-month)` chunks. That's why `send()` is
//! non-blocking and never propagates errors upward — we never want a ClickHouse
//! hiccup to back-pressure the event bus or block the analyzer.

use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::{ClickHouseClient, ClickHouseError, rows::SaleRow};

/// How many rows to accumulate before forcing a flush.
const DEFAULT_BATCH_SIZE: usize = 1000;

/// How long to wait between flushes when the batch hasn't filled. 5s gives
/// dashboards near-real-time data without firing tiny inserts at idle rates.
const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_secs(5);

/// Cheap handle to the background writer task. Cloning shares the same
/// underlying channel; messages from any clone end up in the same batch.
#[derive(Clone)]
pub struct Writer {
    tx: mpsc::UnboundedSender<SaleRow>,
}

impl Writer {
    /// Spawn the flush task and return a handle for sending rows.
    ///
    /// The task exits when `token` is cancelled (after a final flush) or when
    /// every `Writer` clone has been dropped (closing the channel).
    pub fn spawn(client: ClickHouseClient, token: CancellationToken) -> Self {
        Self::spawn_with_config(client, token, DEFAULT_BATCH_SIZE, DEFAULT_FLUSH_INTERVAL)
    }

    /// Same as [`Self::spawn`] but with explicit batch sizing — used by tests
    /// that need to trigger flushes deterministically without waiting on the
    /// 5-second interval.
    pub fn spawn_with_config(
        client: ClickHouseClient,
        token: CancellationToken,
        batch_size: usize,
        flush_interval: Duration,
    ) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<SaleRow>();
        tokio::spawn(async move {
            let mut buf: Vec<SaleRow> = Vec::with_capacity(batch_size);
            let mut interval = tokio::time::interval(flush_interval);
            // If the system is slow and we miss a tick, just delay the next
            // one rather than firing back-to-back catch-up ticks.
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    // Cancellation has priority so shutdown drains promptly.
                    biased;
                    _ = token.cancelled() => {
                        // Drain any rows still sitting in the mpsc channel
                        // before the final flush. With `biased` select the
                        // cancellation arm fires before the recv arm gets a
                        // chance to pull pending messages, so without this
                        // a burst-of-sales right before shutdown would be
                        // silently dropped.
                        while let Ok(row) = rx.try_recv() {
                            buf.push(row);
                        }
                        if !buf.is_empty()
                            && let Err(e) = flush(&client, &mut buf).await
                        {
                            error!(error = ?e, "final ClickHouse flush failed");
                        }
                        break;
                    }
                    maybe_row = rx.recv() => {
                        match maybe_row {
                            Some(row) => {
                                buf.push(row);
                                if buf.len() >= batch_size
                                    && let Err(e) = flush(&client, &mut buf).await
                                {
                                    error!(error = ?e, "ClickHouse flush failed");
                                }
                            }
                            None => {
                                // All senders dropped; drain and exit.
                                if !buf.is_empty()
                                    && let Err(e) = flush(&client, &mut buf).await
                                {
                                    error!(error = ?e, "drain ClickHouse flush failed");
                                }
                                break;
                            }
                        }
                    }
                    _ = interval.tick() => {
                        if !buf.is_empty()
                            && let Err(e) = flush(&client, &mut buf).await
                        {
                            error!(error = ?e, "interval ClickHouse flush failed");
                        }
                    }
                }
            }
            info!("ClickHouse writer task exiting");
        });
        Self { tx }
    }

    /// Non-blocking send. Drops on closed channel — caller-side errors are
    /// logged at debug level only (Postgres is still the source of truth).
    pub fn send(&self, row: SaleRow) {
        if self.tx.send(row).is_err() {
            debug!("ClickHouse writer channel closed; dropping row");
        }
    }

    /// A Writer that swallows every row without spawning a flush task. Used in
    /// two cases:
    ///
    /// 1. Tests that construct an `AnalyzerService` directly without standing
    ///    up ClickHouse.
    /// 2. Production startup when `ClickHouseClient::migrate` fails — wiring a
    ///    real `spawn`ed writer would log a `ClickHouse flush failed` error
    ///    every 5 seconds (and trip the sentry-tracing layer into reporting
    ///    each one as a GlitchTip issue, see issue #5080).
    ///
    /// The channel is created and the receiver immediately dropped so every
    /// `send()` returns `Err` and silently no-ops.
    pub fn disabled() -> Self {
        let (tx, _rx) = mpsc::unbounded_channel::<SaleRow>();
        Self { tx }
    }
}

async fn flush(client: &ClickHouseClient, buf: &mut Vec<SaleRow>) -> Result<(), ClickHouseError> {
    let n = buf.len();
    let mut insert = client.client().insert::<SaleRow>("sales").await?;
    for row in buf.drain(..) {
        insert.write(&row).await?;
    }
    insert.end().await?;
    debug!(rows = n, "ClickHouse sales flush");
    Ok(())
}
