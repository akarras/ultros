use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use chrono::Utc;
use futures::{StreamExt, stream};
use sea_orm::prelude::DateTimeWithTimeZone;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, instrument, warn};
use ultros_api_types::websocket::{ListingEventData, SaleEventData};
use ultros_db::{
    SeaDbErr, UltrosDb,
    common::partial_diff_iterator::{DiffItem, PartialDiffIterator},
    entity::{listing_last_updated::Model, market_sweep, market_sweep_world, world},
    listings::ListingSummary,
    world_data::world_cache::WorldCache,
};
use universalis::{UniversalisClient, WorldId, WorldItemRecencyView};

use crate::event::{EventProducer, EventType};

/// Universalis' most-recently-updated endpoint returns at most this many entries.
const RECENTLY_UPDATED_WINDOW: u8 = 200;
/// Our `listing_last_updated` rows store *ingest* time rather than Universalis'
/// upload time, so an entry only counts as missed when their upload is newer
/// than our ingest by more than this slack.
const UPLOAD_TIME_SLACK_SECONDS: i64 = 120;
/// Minimum time between saturation-triggered full sweeps of a single world.
const FULL_SWEEP_COOLDOWN: Duration = Duration::from_secs(6 * 60 * 60);
/// Backoff schedule for transient Universalis failures inside a sweep chunk:
/// one initial attempt plus one retry per entry.
const CHUNK_RETRY_BACKOFF: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(45),
];

/// Runs `op`, retrying transient Universalis failures (429/5xx/timeouts — see
/// [`universalis::Error::is_transient`]) on the [`CHUNK_RETRY_BACKOFF`]
/// schedule. Non-transient errors and exhausted retries return the last error.
async fn retry_transient<T, F, Fut>(mut op: F) -> Result<T, universalis::Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, universalis::Error>>,
{
    let mut backoff = CHUNK_RETRY_BACKOFF.iter();
    loop {
        match op().await {
            Ok(value) => return Ok(value),
            Err(e) if e.is_transient() => match backoff.next() {
                Some(delay) => tokio::time::sleep(*delay).await,
                None => return Err(e),
            },
            Err(e) => return Err(e),
        }
    }
}

/// State of a world's full-sweep slot. `Running` reserves the slot while a
/// sweep is in flight; only a *completed* sweep stamps `CompletedAt`, so a
/// sweep that dies never costs the world its [`FULL_SWEEP_COOLDOWN`] (a
/// claim that leaks on panic degrades to the old stamp-upfront behavior).
#[derive(Clone, Copy, Debug)]
pub(crate) enum SweepSlot {
    Running,
    CompletedAt(Instant),
}

/// Claims the slot for `world_id` if it is free or its cooldown has elapsed.
/// Returns `false` (without touching the map) when a sweep is already
/// running or a completed sweep is still within [`FULL_SWEEP_COOLDOWN`].
fn claim_slot(slots: &mut HashMap<i32, SweepSlot>, world_id: i32, now: Instant) -> bool {
    match slots.get(&world_id) {
        Some(SweepSlot::Running) => false,
        Some(SweepSlot::CompletedAt(at)) if now.duration_since(*at) < FULL_SWEEP_COOLDOWN => false,
        _ => {
            slots.insert(world_id, SweepSlot::Running);
            true
        }
    }
}

/// Marks the slot as a completed sweep, starting its cooldown.
fn confirm_slot(slots: &mut HashMap<i32, SweepSlot>, world_id: i32, now: Instant) {
    slots.insert(world_id, SweepSlot::CompletedAt(now));
}

/// Frees a claimed-but-unfinished slot. A confirmed cooldown is left alone.
///
/// Called via `release_full_sweep_slot` when a full sweep makes no progress
/// on a world (every chunk skipped) — see `do_full_world_sweep`. Also called
/// from the "a full sweep is already running elsewhere" branch around the
/// claim in `check_for_missed_items_on_world`.
fn release_slot(slots: &mut HashMap<i32, SweepSlot>, world_id: i32) {
    if let Some(SweepSlot::Running) = slots.get(&world_id) {
        slots.remove(&world_id);
    }
}

/// Serializes full sweeps (manual and saturation-triggered): a full sweep
/// fetches every marketable item for a world, and two at once doubles the
/// load on Universalis for zero extra coverage.
///
/// `try_begin_full_sweep` claims it from the saturation branch's else-arm
/// in `check_for_missed_items_on_world` and from `/rescan_market`.
#[derive(Default)]
pub(crate) struct SweepLock(AtomicBool);

/// Stable Postgres advisory-lock id electing the one replica allowed to run a
/// full sweep. Distinct from `ROLLUP_SCHEDULER_LOCK_KEY` in `main.rs`; the two
/// leases are unrelated and a replica may hold either, both, or neither.
const FULL_SWEEP_LOCK_KEY: i64 = 0x53_57_45_45_50;

/// Held for the duration of a full sweep; frees the lock on drop (including
/// panics, so a crashed sweep never wedges the command).
pub(crate) struct SweepLockGuard {
    lock: Arc<SweepLock>,
    /// Connection holding [`FULL_SWEEP_LOCK_KEY`]. `None` for the in-process
    /// claim on its own, which is all the unit tests and a database-less
    /// caller need.
    lease: Option<sea_orm::sqlx::pool::PoolConnection<sea_orm::sqlx::Postgres>>,
}

impl SweepLock {
    pub(crate) fn try_claim(self: &Arc<Self>) -> Option<SweepLockGuard> {
        self.0
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| SweepLockGuard {
                lock: self.clone(),
                lease: None,
            })
    }
}

impl Drop for SweepLockGuard {
    fn drop(&mut self) {
        self.lock.0.store(false, Ordering::SeqCst);
        if let Some(mut connection) = self.lease.take() {
            // Never return a possibly locked session to the pool, including
            // cancellation while the asynchronous unlock is in flight.
            connection.close_on_drop();
            // A session advisory lock outlives the connection's return to the
            // pool, and `Drop` cannot await, so the unlock is handed to a
            // task. A process that dies before it runs is still fine:
            // Postgres releases the lock along with the session.
            tokio::spawn(async move {
                if let Err(error) =
                    sea_orm::sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1)")
                        .bind(FULL_SWEEP_LOCK_KEY)
                        .fetch_one(&mut *connection)
                        .await
                {
                    warn!(?error, "could not release the full sweep advisory lock");
                }
            });
        }
    }
}

impl SweepLockGuard {
    /// Stop polling the worker as soon as its session lease is lost. The
    /// worker uses other pool connections, so those recovering cannot be
    /// mistaken for continued ownership of this dedicated session.
    pub(crate) async fn run<F: std::future::Future>(&mut self, work: F) -> Option<F::Output> {
        let Some(connection) = self.lease.as_mut() else {
            return Some(work.await);
        };
        let lost = async {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                let alive = tokio::time::timeout(
                    Duration::from_secs(5),
                    sea_orm::sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&mut **connection),
                )
                .await;
                if !matches!(alive, Ok(Ok(_))) {
                    warn!("lost full market sweep lease; stopping the worker");
                    return;
                }
            }
        };
        run_until_lease_lost(work, lost).await
    }
}

async fn run_until_lease_lost<F: std::future::Future>(
    work: F,
    lost: impl std::future::Future<Output = ()>,
) -> Option<F::Output> {
    tokio::select! {
        biased;
        _ = lost => None,
        result = work => Some(result),
    }
}

/// Item update service attempts to keep ultros' data in sync with Universalis' data.
/// It does this primarily by comparing the recently updated items on Universalis with recently updated items on ultros
pub(crate) struct UpdateService {
    pub(crate) db: UltrosDb,
    pub(crate) world_cache: Arc<WorldCache>,
    pub(crate) universalis: UniversalisClient,
    pub(crate) listings: EventProducer<ListingEventData>,
    pub(crate) sales: EventProducer<SaleEventData>,
    /// ClickHouse `listing_events` mirror for every board this service writes.
    pub(crate) listing_events:
        ultros_clickhouse::writer::Writer<ultros_clickhouse::rows::ListingEventRow>,
    /// Per-world full-sweep slot: `Running` while a sweep is in flight,
    /// `CompletedAt` while the [`FULL_SWEEP_COOLDOWN`] from the last
    /// completed sweep is still active. See [`SweepSlot`].
    pub(crate) full_sweep_cooldowns: Mutex<HashMap<i32, SweepSlot>>,
    /// Worlds Universalis 404s on. Purely a log de-duplicator — see
    /// [`UpdateService::note_world_uncovered`].
    pub(crate) uncovered_worlds: Mutex<HashSet<i32>>,
    /// Serializes full sweeps — see [`SweepLock`].
    pub(crate) sweep_lock: Arc<SweepLock>,
    /// Fires on shutdown. A full sweep runs for hours and is routinely cut
    /// short by a deploy; watching this lets it stop on a chunk boundary with
    /// its cursor written, rather than being killed mid-write.
    pub(crate) shutdown: CancellationToken,
}

/// True when `error` is a Universalis `404`, i.e. it does not know the entity
/// we asked about. [`anyhow::Error`] erases the type, so match on the downcast.
fn universalis_not_found(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<universalis::Error>()
        .is_some_and(|e| e.is_not_found())
}

/// True when `error` is a Universalis failure that is expected to clear on its
/// own (429, 5xx, connect/timeout) — see [`universalis::Error::is_transient`].
fn universalis_transient(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<universalis::Error>()
        .is_some_and(|e| e.is_transient())
}

/// How one item's catch-up fetch turned out, used to label
/// `ultros_catchup_items_recovered`.
///
/// The recency diff flags an item whenever Universalis' upload time is newer
/// than our ingest marker, but an upload that changed nothing emits no
/// websocket events — so the marker never moves and the item is flagged even
/// though we missed nothing. Without the label, that structural noise is
/// indistinguishable from a genuinely lagging feed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CatchupOutcome {
    /// The fetch altered our data — a genuine missed update.
    Changed,
    /// Universalis' upload was newer than our marker but contained nothing we
    /// didn't already have. Upload churn, not backlog.
    Noop,
    /// A write failed; the marker was left untouched so the item is retried
    /// on the next cycle.
    Failed,
}

/// Classifies one item's catch-up result. `sales_changed` is `None` when the
/// sales write failed; a listing change still counts as `Changed` in that case
/// because real data was recovered regardless.
fn classify_catchup(listings_changed: bool, sales_changed: Option<bool>) -> CatchupOutcome {
    match (listings_changed, sales_changed) {
        (true, _) => CatchupOutcome::Changed,
        (false, Some(true)) => CatchupOutcome::Changed,
        (false, Some(false)) => CatchupOutcome::Noop,
        (false, None) => CatchupOutcome::Failed,
    }
}

/// Per-world tally of [`CatchupOutcome`]s for one sweep.
#[derive(Default, Debug, PartialEq, Eq)]
struct CatchupTally {
    changed: u64,
    noop: u64,
    failed: u64,
    /// Fetch chunks skipped after retries — see `ultros_sweep_chunks_failed`.
    chunks_failed: u64,
}

impl CatchupTally {
    fn add(&mut self, outcome: CatchupOutcome) {
        match outcome {
            CatchupOutcome::Changed => self.changed += 1,
            CatchupOutcome::Noop => self.noop += 1,
            CatchupOutcome::Failed => self.failed += 1,
        }
    }

    fn record(&self, world_name: &str) {
        for (outcome, count) in [
            ("changed", self.changed),
            ("noop", self.noop),
            ("failed", self.failed),
        ] {
            if count > 0 {
                metrics::counter!(
                    "ultros_catchup_items_recovered",
                    "world" => world_name.to_string(),
                    "outcome" => outcome
                )
                .increment(count);
            }
        }
    }

    /// True when a full sweep actually fetched and processed at least one
    /// item — `changed`, `noop`, and `failed` all count as progress, since
    /// each means an item was fetched and classified; only `chunks_failed`
    /// (every chunk skipped after retries) means nothing was recovered.
    /// Used by [`UpdateService::do_full_world_sweep`] to decide whether a
    /// world's full-sweep cooldown should be confirmed or released — a
    /// world with no progress must not burn its 6h cooldown for a sweep
    /// that recovered nothing.
    fn made_progress(&self) -> bool {
        self.changed + self.noop + self.failed > 0
    }
}

/// One world's outcome from a full sweep, folded into a [`SweepReport`].
///
/// `duration` feeds [`SweepReport::summary_text`]'s "slowest world" line —
/// see there.
pub(crate) struct WorldSweepSummary {
    world_name: String,
    tally: CatchupTally,
    duration: std::time::Duration,
}

impl WorldSweepSummary {
    /// Reads a world's totals back out of its persisted progress row. A sweep
    /// spanning a restart reports through this for every world, so the closing
    /// numbers cover the whole run rather than the process that finished it.
    fn from_progress(world_name: &str, row: &market_sweep_world::Model) -> Self {
        let count = |value: i64| u64::try_from(value).unwrap_or_default();
        Self {
            world_name: world_name.to_string(),
            tally: CatchupTally {
                changed: count(row.changed),
                noop: count(row.noop),
                failed: count(row.failed),
                chunks_failed: count(row.chunks_failed),
            },
            duration: std::time::Duration::from_millis(count(row.elapsed_ms)),
        }
    }
}

/// Fired after each world completes during [`UpdateService::do_full_world_sweep`]
/// so a long-running sweep can report interim status. `/rescan_market`
/// (`admin.rs`) forwards these through a throttled channel into a Discord
/// message via [`SweepProgress::summary_text`].
pub(crate) struct SweepProgress {
    worlds_done: usize,
    worlds_total: usize,
    items_changed: u64,
    chunks_failed: u64,
}

impl SweepProgress {
    pub(crate) fn summary_text(&self) -> String {
        format!(
            "Sweep progress: {}/{} worlds — {} items updated, {} chunks skipped.",
            self.worlds_done, self.worlds_total, self.items_changed, self.chunks_failed
        )
    }
}

/// Worlds listed by name before the count collapses to "+N more" — keeps the
/// summary safely under Discord's 2000-character message cap.
const REPORT_MAX_LISTED_WORLDS: usize = 10;

/// Result of a full sweep across every world, returned by
/// [`UpdateService::do_full_world_sweep`].
pub(crate) struct SweepReport {
    worlds: Vec<WorldSweepSummary>,
    worlds_total: usize,
    /// Wall-clock since the sweep was *started*, which for a resumed sweep
    /// spans every process that has worked on it.
    duration: std::time::Duration,
    /// True when shutdown stopped the sweep partway. Nothing was lost — the
    /// cursor is on disk and the next start resumes from it.
    interrupted: bool,
}

impl SweepReport {
    /// True when the sweep ran to the end and no longer needs resuming.
    pub(crate) fn is_complete(&self) -> bool {
        !self.interrupted
    }

    pub(crate) fn summary_text(&self) -> String {
        let changed: u64 = self.worlds.iter().map(|w| w.tally.changed).sum();
        let failed: u64 = self.worlds.iter().map(|w| w.tally.failed).sum();
        let chunks_failed: u64 = self.worlds.iter().map(|w| w.tally.chunks_failed).sum();
        let minutes = self.duration.as_secs() / 60;
        let mut text = if self.interrupted {
            format!(
                "Full market sweep paused at {}/{} worlds after {minutes} min — the server is restarting and will pick it up where it left off. {changed} items updated, {failed} item writes failed, {chunks_failed} chunks skipped so far.",
                self.worlds.len(),
                self.worlds_total
            )
        } else {
            format!(
                "Full market sweep finished: {} worlds in {minutes} min — {changed} items updated, {failed} item writes failed, {chunks_failed} chunks skipped.",
                self.worlds.len()
            )
        };
        // Flags the single slowest world so an operator can spot one world
        // dragging out the whole sweep (e.g. Universalis rate-limiting it
        // harder than the rest) without having to dig through server logs.
        if let Some(slowest) = self.worlds.iter().max_by_key(|w| w.duration) {
            let slowest_minutes = slowest.duration.as_secs() / 60;
            let slowest_seconds = slowest.duration.as_secs() % 60;
            text.push_str(&format!(
                "\nSlowest world: {} ({slowest_minutes}m{slowest_seconds:02}s).",
                slowest.world_name
            ));
        }
        let incomplete: Vec<&str> = self
            .worlds
            .iter()
            .filter(|w| w.tally.chunks_failed > 0)
            .map(|w| w.world_name.as_str())
            .collect();
        if !incomplete.is_empty() {
            let listed = incomplete[..incomplete.len().min(REPORT_MAX_LISTED_WORLDS)].join(", ");
            let overflow = incomplete.len().saturating_sub(REPORT_MAX_LISTED_WORLDS);
            text.push_str(&format!("\nIncomplete worlds: {listed}"));
            if overflow > 0 {
                text.push_str(&format!(" (+{overflow} more)"));
            }
            text.push_str(" — the 5-minute catch-up loop will recover the skipped items.");
        }
        text
    }
}

/// A full sweep's durable identity: the row it records progress against, plus
/// whatever an earlier process already recorded for it.
///
/// Obtained from [`UpdateService::begin_or_resume_sweep`] (the `/rescan_market`
/// path) or [`UpdateService::resume_sweep`] (startup), and handed to
/// [`UpdateService::do_full_world_sweep`].
pub(crate) struct SweepRun {
    sweep: market_sweep::Model,
    /// Per-world progress rows already on disk, keyed by world id.
    progress: HashMap<i32, market_sweep_world::Model>,
    /// True when this picked up a sweep an earlier process left unfinished
    /// rather than starting a new one.
    pub(crate) resumed: bool,
}

impl SweepRun {
    /// Channel the sweep should report to, from the `/rescan_market` that
    /// started it — which may have been in a process that no longer exists.
    pub(crate) fn discord_channel_id(&self) -> Option<i64> {
        self.sweep.discord_channel_id
    }

    /// Worlds already swept before this process picked the sweep up.
    pub(crate) fn worlds_done(&self) -> usize {
        self.progress
            .values()
            .filter(|world| world.completed_at.is_some())
            .count()
    }
}

/// A world's slot in the persisted sweep, written after every chunk so an
/// interrupted sweep loses at most one chunk of work.
///
/// `prior` is the row as this process found it; the tallies handed to
/// [`WorldCheckpoint::record`] are only what *this* process has recovered, and
/// are added on top. Keeping the two apart is what lets `CatchupTally::record`
/// report honest per-process metrics while the persisted row keeps growing
/// across restarts.
struct WorldCheckpoint<'a> {
    db: &'a UltrosDb,
    prior: market_sweep_world::Model,
    started: Instant,
}

impl WorldCheckpoint<'_> {
    /// Persists the resume point mid-world. A failed write costs resume
    /// precision and nothing else — the next process simply restarts this
    /// world further back — so it is logged rather than aborting a multi-hour
    /// sweep. The world's *completion* is stamped by `do_full_world_sweep`,
    /// which is the only place that knows the whole world was walked.
    async fn record(&self, next_item_id: i32, run: &CatchupTally) {
        let row = merged_progress(&self.prior, run, next_item_id, self.started.elapsed(), None);
        if let Err(error) = self.db.record_market_sweep_progress(&row).await {
            warn!(
                ?error,
                world_id = row.world_id,
                "could not record market sweep progress; resume will restart this world earlier"
            );
        }
    }
}

/// A world's progress row before any process has touched it.
fn new_world_progress(sweep_id: i32, world_id: i32) -> market_sweep_world::Model {
    market_sweep_world::Model {
        sweep_id,
        world_id,
        next_item_id: 0,
        completed_at: None,
        changed: 0,
        noop: 0,
        failed: 0,
        chunks_failed: 0,
        elapsed_ms: 0,
    }
}

/// Folds one process's share of a world (`run`, `elapsed`) into the row a
/// previous process left behind, producing the row to persist.
fn merged_progress(
    prior: &market_sweep_world::Model,
    run: &CatchupTally,
    next_item_id: i32,
    elapsed: std::time::Duration,
    completed_at: Option<DateTimeWithTimeZone>,
) -> market_sweep_world::Model {
    let add = |before: i64, during: u64| before.saturating_add(during as i64);
    market_sweep_world::Model {
        sweep_id: prior.sweep_id,
        world_id: prior.world_id,
        next_item_id,
        completed_at,
        changed: add(prior.changed, run.changed),
        noop: add(prior.noop, run.noop),
        failed: add(prior.failed, run.failed),
        chunks_failed: add(prior.chunks_failed, run.chunks_failed),
        elapsed_ms: add(prior.elapsed_ms, elapsed.as_millis() as u64),
    }
}

/// Wall-clock since a sweep was started, which for a resumed sweep spans every
/// process that has worked on it. Clocks that ran backwards read as zero.
fn elapsed_since(started_at: DateTimeWithTimeZone) -> std::time::Duration {
    (Utc::now().fixed_offset() - started_at)
        .to_std()
        .unwrap_or_default()
}

/// Where a resumed world picks back up: the index of the first item id at or
/// after `next_item_id` in the sweep's ascending item list.
///
/// The cursor is an item id rather than a chunk offset so that a game-data
/// bump between restarts — which adds items and shifts every offset after
/// them — still resumes at the right place.
fn resume_offset(items: &[i32], next_item_id: i32) -> usize {
    items.partition_point(|id| *id < next_item_id)
}

/// Cursor to record once `chunk` has been attempted: the first item id past
/// it, or `fallback` when there was nothing left to attempt.
///
/// Advances over a chunk Universalis refused as well as one it answered — the
/// skip is already tallied in `chunks_failed` and reported, and rewinding to
/// it on resume would mean re-fetching every chunk that succeeded after it.
fn cursor_past(chunk: &[i32], fallback: i32) -> i32 {
    chunk.last().map_or(fallback, |id| id.saturating_add(1))
}

struct CmpListing(Model);

impl PartialOrd<WorldItemRecencyView> for CmpListing {
    fn partial_cmp(&self, other: &WorldItemRecencyView) -> Option<std::cmp::Ordering> {
        self.0.item_id.partial_cmp(&other.item_id)
    }
}

impl PartialEq<WorldItemRecencyView> for CmpListing {
    fn eq(&self, other: &WorldItemRecencyView) -> bool {
        self.0.item_id.eq(&other.item_id)
    }
}

impl UpdateService {
    pub(crate) fn start_service(service: Arc<Self>, token: CancellationToken) {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        break;
                    }
                    _ = async {
                // check all worlds
                info!("Checking all worlds");
                // Create this 5 minute duration check now so that our refresh interval includes the time we spent checking
                let next_interval = Instant::now() + tokio::time::Duration::from_secs(60 * 5);
                for world in service.world_cache.get_all_worlds() {
                    info!("{world:?}");
                    let result = service.check_for_missed_items_on_world(world).await;
                    if let Err(e) = result {
                        // Same reasoning as the price-drift probe below: this
                        // sweep re-runs every five minutes, so Universalis
                        // shedding one request is a skipped cycle, not an
                        // application error worth reporting.
                        if universalis_transient(&e) {
                            warn!(error = ?e, world = %world.name, "catch-up sweep skipped: universalis unavailable");
                        } else {
                            error!(error = ?e, world = %world.name, "check_for_missed_items_on_world failed");
                        }
                    }
                }
                tokio::time::sleep_until(next_interval).await;
                    } => {}
                }
            }
        });
    }

    /// Every marketable item id, in ascending order.
    ///
    /// Sorted because `xiv_gen_db`'s item table is a `HashMap`, whose
    /// iteration order changes from process to process. A full sweep records
    /// its resume point as an item id in this order, so the order has to be
    /// the same in the process that resumes as in the one that stopped.
    pub(crate) fn all_marketable_items() -> Box<[i32]> {
        let mut items: Vec<i32> = xiv_gen_db::data()
            .items
            .values()
            .filter(|i| i.item_search_category != 0)
            .map(|i| i.key_id.0)
            .collect();
        items.sort_unstable();
        items.into_boxed_slice()
    }

    /// Claims the right to run a full sweep. `None` when one is already
    /// running — here or on another replica.
    ///
    /// Two locks, because a sweep has to be unique in two scopes.
    /// [`SweepLock`] keeps one process from starting two. The Postgres session
    /// advisory lock keeps a second replica from resuming the *same* sweep:
    /// the resume cursor lives in a shared database and every replica reads it
    /// on startup, so an in-process flag alone stopped being enough the moment
    /// sweeps became resumable. Session locks die with the connection, so a
    /// replica killed mid-sweep releases it with nothing to clean up.
    pub(crate) async fn try_begin_full_sweep(&self) -> Option<SweepLockGuard> {
        let mut guard = self.sweep_lock.try_claim()?;
        let pool = self.db.get_connection().get_postgres_connection_pool();
        let mut connection = match pool.acquire().await {
            Ok(connection) => connection,
            Err(error) => {
                warn!(
                    ?error,
                    "could not acquire a connection for the full sweep lease"
                );
                return None;
            }
        };
        // Cancellation can happen after Postgres grants the lock but before
        // the query result is delivered. Such a session must not be pooled.
        connection.close_on_drop();
        match sea_orm::sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock($1)")
            .bind(FULL_SWEEP_LOCK_KEY)
            .fetch_one(&mut *connection)
            .await
        {
            Ok(true) => {
                guard.lease = Some(connection);
                Some(guard)
            }
            // Another replica is sweeping. Dropping `guard` frees the
            // in-process flag we speculatively claimed above.
            Ok(false) => None,
            Err(error) => {
                warn!(?error, "could not claim the full sweep lease");
                None
            }
        }
    }

    /// Loads the sweep an earlier process left unfinished, if any, together
    /// with the per-world progress it recorded. `None` means there is nothing
    /// to resume.
    pub(crate) async fn resume_sweep(&self) -> Result<Option<SweepRun>, SeaDbErr> {
        let Some(sweep) = self.db.active_market_sweep().await? else {
            return Ok(None);
        };
        Ok(Some(self.load_progress(sweep, true).await?))
    }

    /// The unfinished sweep, or a fresh one when none is in flight.
    /// `channel_id` becomes where the sweep reports, including after a
    /// restart. Check [`SweepRun::resumed`] to tell the two cases apart.
    pub(crate) async fn begin_or_resume_sweep(
        &self,
        channel_id: Option<i64>,
    ) -> Result<SweepRun, SeaDbErr> {
        let (sweep, resumed) = self.db.begin_or_resume_market_sweep(channel_id).await?;
        self.load_progress(sweep, resumed).await
    }

    async fn load_progress(
        &self,
        sweep: market_sweep::Model,
        resumed: bool,
    ) -> Result<SweepRun, SeaDbErr> {
        let progress = self
            .db
            .market_sweep_progress(sweep.id)
            .await?
            .into_iter()
            .map(|row| (row.world_id, row))
            .collect();
        Ok(SweepRun {
            sweep,
            progress,
            resumed,
        })
    }

    /// Sweeps over every single marketable item in the game, ignoring the
    /// recency cache. Only should be used if data is known to be lost. Never
    /// aborts: failed chunks are skipped and reported via the returned
    /// [`SweepReport`]. `progress` fires after each world completes.
    ///
    /// Resumable: worlds `run` records as already swept are skipped, a world
    /// left partway through restarts at its cursor, and the cursor is written
    /// after every chunk. Shutdown stops the sweep at the next chunk boundary
    /// and leaves it unfinished for the next process to pick up — the report
    /// then says [`SweepReport::is_complete`] is false.
    ///
    /// Callers must hold a [`SweepLockGuard`] (see
    /// [`UpdateService::try_begin_full_sweep`]) so only one full sweep runs.
    pub(crate) async fn do_full_world_sweep(
        &self,
        run: SweepRun,
        mut progress: impl FnMut(SweepProgress),
    ) -> SweepReport {
        let SweepRun {
            sweep,
            progress: mut recorded,
            ..
        } = run;
        let all_marketable_items = Self::all_marketable_items();
        let worlds: Vec<&world::Model> = self.world_cache.get_all_worlds().copied().collect();
        let worlds_total = worlds.len();
        if worlds_total == 0 {
            // The world cache had not loaded. Nothing was swept, so the sweep
            // has to stay unfinished — stamping it done here would silently
            // drop a run the next start would otherwise resume.
            error!("a full market sweep found no worlds; leaving it to be resumed");
            return SweepReport {
                worlds: Vec::new(),
                worlds_total,
                duration: elapsed_since(sweep.started_at),
                interrupted: true,
            };
        }
        let mut summaries = Vec::with_capacity(worlds_total);
        let (mut items_changed, mut chunks_failed) = (0u64, 0u64);
        let mut interrupted = false;
        for world in worlds {
            let prior = recorded
                .remove(&world.id)
                .unwrap_or_else(|| new_world_progress(sweep.id, world.id));
            let row = if prior.completed_at.is_some() {
                // Swept before the restart. Its totals still belong in the
                // report, but nothing is re-fetched and no metric is
                // re-emitted: this process did not do that work.
                prior
            } else {
                if self.shutdown.is_cancelled() {
                    interrupted = true;
                    break;
                }
                let world_started = Instant::now();
                let offset = resume_offset(&all_marketable_items, prior.next_item_id);
                if offset > 0 {
                    info!(world = %world.name, resume_from = prior.next_item_id, "full sweep: resuming world");
                } else {
                    info!(world = %world.name, "full sweep: scanning world");
                }
                let checkpoint = WorldCheckpoint {
                    db: &self.db,
                    prior,
                    started: world_started,
                };
                let remaining = &all_marketable_items[offset..];
                let tally = self
                    .check_items_with(world, remaining, Some(&checkpoint))
                    .await;
                tally.record(&world.name);
                // A world that got nothing — every chunk skipped — has not
                // been refetched, so it must not burn its cooldown; releasing
                // lets the next saturated cycle retry. Partial coverage still
                // counts: re-sweeping a world we mostly refetched would hammer
                // Universalis for little gain, which is what the cooldown
                // exists to stop.
                if tally.made_progress() {
                    self.confirm_full_sweep(world.id);
                } else {
                    self.release_full_sweep_slot(world.id);
                }
                if self.shutdown.is_cancelled() {
                    // Shutdown cut the world short. The last chunk's
                    // checkpoint already recorded where to pick up, so leave
                    // the world unstamped rather than claiming it is done.
                    interrupted = true;
                    break;
                }
                let completed = merged_progress(
                    &checkpoint.prior,
                    &tally,
                    cursor_past(remaining, checkpoint.prior.next_item_id),
                    world_started.elapsed(),
                    Some(Utc::now().fixed_offset()),
                );
                if let Err(error) = self.db.record_market_sweep_progress(&completed).await {
                    // The world will simply be swept again after a restart.
                    warn!(?error, world = %world.name, "could not record the completed world");
                }
                completed
            };
            items_changed += u64::try_from(row.changed).unwrap_or_default();
            chunks_failed += u64::try_from(row.chunks_failed).unwrap_or_default();
            summaries.push(WorldSweepSummary::from_progress(&world.name, &row));
            progress(SweepProgress {
                worlds_done: summaries.len(),
                worlds_total,
                items_changed,
                chunks_failed,
            });
        }
        if !interrupted && let Err(error) = self.db.finish_market_sweep(sweep.id).await {
            // The sweep is done but still looks unfinished, so the next start
            // resumes it — finding every world stamped complete and closing it
            // out immediately. Wasteful, not wrong.
            error!(
                ?error,
                sweep_id = sweep.id,
                "could not mark the market sweep finished"
            );
        }
        SweepReport {
            worlds: summaries,
            worlds_total,
            duration: elapsed_since(sweep.started_at),
            interrupted,
        }
    }

    /// Records that Universalis does not cover `world_id`. Returns `true` the
    /// first time, so the caller logs once instead of on every sweep.
    fn note_world_uncovered(&self, world_id: i32) -> bool {
        self.uncovered_worlds
            .lock()
            .expect("uncovered_worlds poisoned")
            .insert(world_id)
    }

    /// Claims the full-sweep slot for a world if its cooldown has elapsed.
    fn claim_full_sweep_slot(&self, world_id: i32) -> bool {
        claim_slot(
            &mut self
                .full_sweep_cooldowns
                .lock()
                .expect("full_sweep_cooldowns poisoned"),
            world_id,
            Instant::now(),
        )
    }

    /// Marks a claimed slot as a completed sweep, starting its cooldown.
    fn confirm_full_sweep(&self, world_id: i32) {
        confirm_slot(
            &mut self
                .full_sweep_cooldowns
                .lock()
                .expect("full_sweep_cooldowns poisoned"),
            world_id,
            Instant::now(),
        )
    }

    /// Frees a claimed-but-unfinished full-sweep slot so the next saturated
    /// cycle can retry immediately instead of waiting out a cooldown that
    /// was never earned.
    ///
    /// Called by `do_full_world_sweep` when a world's sweep makes no
    /// progress at all, and from the "a full sweep is already running
    /// elsewhere" branch around the claim below. See `release_slot`.
    fn release_full_sweep_slot(&self, world_id: i32) {
        release_slot(
            &mut self
                .full_sweep_cooldowns
                .lock()
                .expect("full_sweep_cooldowns poisoned"),
            world_id,
        )
    }

    #[instrument(level = "trace", skip(self))]
    async fn check_for_missed_items_on_world(
        &self,
        world: &world::Model,
    ) -> Result<(), anyhow::Error> {
        let (window_ids, updates) = match self.get_missing_updates(world).await {
            Ok(updates) => updates,
            // Universalis 404s the recency endpoint for worlds it no longer
            // carries (our `world` table keeps rows for worlds it has since
            // dropped — `Innocence`, `Pixie`, `Titania`, `Tycoon`, `月牙湾`,
            // `雪松原`, `黄金谷` as of this writing). There is nothing to catch
            // up on and never will be, so log it once per process rather than
            // reporting an error every five minutes forever.
            Err(e) if universalis_not_found(&e) => {
                if self.note_world_uncovered(world.id) {
                    warn!(
                        world = %world.name,
                        "universalis has no data for this world; skipping catch-up sweeps"
                    );
                }
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        let item_ids: Box<[i32]> = updates.into_iter().map(|i| i.item_id).collect();
        let tally = self.check_items(world, &item_ids).await;
        tally.record(&world.name);
        if item_ids.len() >= usize::from(RECENTLY_UPDATED_WINDOW) {
            // Every entry in the recency window was one we missed, so more
            // updates have likely scrolled past where this endpoint can see.
            // The only way to recover those is a full sweep of the world.
            metrics::counter!("ultros_catchup_window_saturated", "world" => world.name.clone())
                .increment(1);
            if self.claim_full_sweep_slot(world.id) {
                // The world slot is ours; the global lock keeps us from
                // overlapping a manual /rescan_market sweep. If it's busy,
                // hand the world slot back unstamped so the next saturated
                // cycle retries.
                if let Some(mut guard) = self.try_begin_full_sweep().await {
                    warn!(world = %world.name, "recency window saturated, running full item sweep");
                    let items = Self::all_marketable_items();
                    let Some(tally) = guard.run(self.check_items(world, &items)).await else {
                        self.release_full_sweep_slot(world.id);
                        return Ok(());
                    };
                    tally.record(&world.name);
                    // Same rule as `do_full_world_sweep`: a world with no
                    // progress at all must not burn its cooldown for a sweep
                    // that recovered nothing.
                    if tally.made_progress() {
                        self.confirm_full_sweep(world.id);
                    } else {
                        self.release_full_sweep_slot(world.id);
                    }
                } else {
                    self.release_full_sweep_slot(world.id);
                    // Either another sweep holds the lock (here or on another
                    // replica) or the lease could not be taken; both are
                    // logged in detail by `try_begin_full_sweep`.
                    warn!(world = %world.name, "recency window saturated, but the full sweep lock is unavailable");
                }
            } else {
                warn!(world = %world.name, "recency window saturated, full sweep on cooldown");
            }
            // Either way every window item just got (or recently got) a full
            // refetch; probing them for drift now would only re-answer the
            // question the refetch already settled.
            return Ok(());
        }

        // Price-drift probe over the window items the marker diff called
        // in-sync. The marker cannot be trusted for them: Universalis delivers
        // each upload's events independently per channel, so a lost
        // `listings/remove` alongside a delivered `sales/add` (a purchase,
        // exactly the #1178 case) stamps the marker fresh while the board keeps
        // the sold listing forever. Comparing cheapest prices against their
        // aggregated cache catches that directly.
        let repaired: HashSet<i32> = item_ids.iter().copied().collect();
        let probe_ids: Vec<i32> = window_ids
            .into_iter()
            .filter(|id| !repaired.contains(id))
            .collect();
        if probe_ids.is_empty() {
            return Ok(());
        }
        match self.find_price_drift(world, &probe_ids).await {
            Ok(drifted) if !drifted.is_empty() => {
                warn!(
                    world = %world.name,
                    count = drifted.len(),
                    items = ?drifted,
                    "cheapest-price drift despite fresh ingest markers; refetching boards"
                );
                metrics::counter!("ultros_catchup_price_drift_items", "world" => world.name.clone())
                    .increment(drifted.len() as u64);
                let tally = self.check_items(world, &drifted).await;
                tally.record(&world.name);
            }
            Ok(_) => {}
            // The probe is an extra safety net on top of the normal sweep — a
            // failed probe cycle must not fail the world's catch-up pass.
            //
            // Nor is it worth *reporting* when Universalis simply shed the
            // request: the aggregated cache answers 429/5xx under congestion,
            // and the probe re-runs for every world every five minutes, so one
            // lost cycle changes nothing. Reporting those drowned the real
            // errors in this path (115 reports in five hours, every one an
            // upstream 504). Warnings still reach the log and ride along as
            // breadcrumbs; the metric makes the rate visible without paging.
            Err(e) => {
                let kind = if universalis_transient(&e) {
                    warn!(error = ?e, world = %world.name, "price-drift probe skipped: universalis unavailable");
                    "transient"
                } else {
                    error!(error = ?e, world = %world.name, "price-drift probe failed");
                    "error"
                };
                metrics::counter!(
                    "ultros_catchup_price_drift_probe_failed",
                    "world" => world.name.clone(),
                    "kind" => kind,
                )
                .increment(1);
            }
        }
        Ok(())
    }

    /// Items whose cheapest listed price in our DB disagrees with Universalis'
    /// aggregated cache for `world`, per quality. Runs on items whose ingest
    /// markers look fresh, so any mismatch means our board silently drifted —
    /// in practice a `listings/remove` that never reached us (Universalis'
    /// publisher drops/misroutes some removals) while adds and sales kept
    /// stamping the marker. Items Universalis itself cannot answer for
    /// (`failedItems`) are skipped rather than flagged.
    ///
    /// Both sides are read while the market keeps moving, so an item can be
    /// flagged by an ordinary in-flight update; the cost of a false positive is
    /// one redundant board refetch, bounded by the recency window size.
    async fn find_price_drift(
        &self,
        world: &world::Model,
        item_ids: &[i32],
    ) -> Result<Vec<i32>, anyhow::Error> {
        let mut theirs = HashMap::new();
        for chunk in item_ids.chunks(100) {
            let aggregated = self
                .universalis
                .aggregated_market_data(&world.name, chunk)
                .await?;
            for item in aggregated.results {
                theirs.insert(item.item_id, item.world_min_prices());
            }
        }
        let ours = self
            .db
            .cheapest_listings_for_items(&[world.id], item_ids)
            .await?;
        Ok(drifted_items(theirs, &ours))
    }

    /// Returns (every item id in Universalis' recency window, the subset our
    /// markers say we missed). The full window feeds the price-drift probe:
    /// marker freshness alone cannot prove a board is in sync (see
    /// [`UpdateService::find_price_drift`]), so the probe needs the items the
    /// marker diff considered fine, not just the ones it flagged.
    async fn get_missing_updates(
        &self,
        world: &world::Model,
    ) -> Result<(Vec<i32>, Vec<WorldItemRecencyView>), anyhow::Error> {
        let recently_updated = self
            .universalis
            .recently_updated_items(
                universalis::WorldOrDatacenter::World(&world.name),
                RECENTLY_UPDATED_WINDOW,
            )
            .await?;
        let our_recently_updated = self
            .db
            .get_recently_updated_listings_for_world(
                world.id,
                recently_updated.items.len() as u64 * 2,
            )
            .await?;
        let window_ids = recently_updated.items.iter().map(|i| i.item_id).collect();
        Ok((
            window_ids,
            missed_updates(our_recently_updated, recently_updated.items),
        ))
    }

    /// Refetches `item_ids` for `world`. Used both by the five-minute catch-up
    /// passes and, via [`UpdateService::check_items_with`], by the full sweep.
    async fn check_items(&self, world: &world::Model, item_ids: &[i32]) -> CatchupTally {
        self.check_items_with(world, item_ids, None).await
    }

    /// [`UpdateService::check_items`] with an optional per-chunk checkpoint.
    ///
    /// `checkpoint` is `Some` only for the full sweep, which runs for hours
    /// and has to survive a deploy; the catch-up passes cover a couple hundred
    /// items and are cheaper to redo than to record. When it is `Some`, the
    /// pass also stops at the next chunk boundary once shutdown is signalled,
    /// leaving the cursor where the next process should resume.
    async fn check_items_with(
        &self,
        world: &world::Model,
        item_ids: &[i32],
        checkpoint: Option<&WorldCheckpoint<'_>>,
    ) -> CatchupTally {
        let world_id = WorldId(world.id);
        let world_name = &world.name;
        let mut tally = CatchupTally::default();
        let total_chunks = item_ids.chunks(100).len();
        for (chunk_index, item_ids) in item_ids.chunks(100).enumerate() {
            if checkpoint.is_some() && self.shutdown.is_cancelled() {
                info!(world = %world_name, "full sweep: stopping for shutdown; progress is recorded");
                break;
            }
            let market_data = match retry_transient(|| {
                self.universalis
                    .marketboard_current_data(world_name, item_ids)
            })
            .await
            {
                Ok(data) => data,
                Err(e) if e.is_transient() => {
                    // Universalis kept shedding this chunk through the whole
                    // backoff schedule. The items' ingest markers are untouched,
                    // so the five-minute catch-up loop will re-flag them.
                    warn!(error = ?e, world = %world_name, items = item_ids.len(), "sweep chunk skipped after retries");
                    metrics::counter!(
                        "ultros_sweep_chunks_failed",
                        "world" => world_name.clone(),
                        "kind" => "transient",
                    )
                    .increment(1);
                    tally.chunks_failed += 1;
                    if let Some(checkpoint) = checkpoint {
                        checkpoint.record(cursor_past(item_ids, 0), &tally).await;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
                Err(e) => {
                    // A non-transient answer (404 world, malformed response) will
                    // repeat for every remaining chunk of this world — one warning
                    // and a bulk count beat ~150 identical ones.
                    let remaining = (total_chunks - chunk_index) as u64;
                    warn!(error = ?e, world = %world_name, remaining_chunks = remaining, "sweep aborted for world: universalis fetch failed");
                    metrics::counter!(
                        "ultros_sweep_chunks_failed",
                        "world" => world_name.clone(),
                        "kind" => "error",
                    )
                    .increment(remaining);
                    tally.chunks_failed += remaining;
                    break;
                }
            };
            info!("missing data {item_ids:?}");

            let outcomes = stream::iter(
                market_data
                    .items()
                    .map(|(item_id, listings, sales)| async move {
                        let listings_changed;
                        match self.db.update_listings(listings, item_id, world_id).await {
                            Ok(ultros_db::listings::ListingWrite {
                                added,
                                removed,
                                changes,
                            }) => {
                                listings_changed = !added.is_empty() || !removed.is_empty();
                                crate::record_listing_changes(
                                    &self.listing_events,
                                    &changes,
                                    ultros_clickhouse::rows::ListingEventSource::Catchup,
                                );
                                let _ =
                                    self.listings
                                        .send(EventType::Add(Arc::new(ListingEventData {
                                            item_id: item_id.0,
                                            world_id: world_id.0,
                                            listings: added,
                                        })));
                                let _ = self.listings.send(EventType::Remove(Arc::new(
                                    ListingEventData {
                                        item_id: item_id.0,
                                        world_id: world_id.0,
                                        listings: removed,
                                    },
                                )));
                            }
                            Err(e) => {
                                error!(error = ?e, item_id = item_id.0, world_id = world_id.0, "catch-up listing update failed");
                                // Storing the sales would bump `listing_last_updated`,
                                // the very marker this service diffs against Universalis
                                // to decide what still needs recovering. Bumping it after
                                // a failed listing write would make the item look freshly
                                // ingested and hide the gap from every later pass, so
                                // leave it untouched and retry on the next cycle.
                                return CatchupOutcome::Failed;
                            }
                        }
                        let sales_changed = match self.db.update_sales(sales, item_id, world_id).await {
                            Ok(added) => {
                                let sales_changed = !added.is_empty();
                                let _ = self
                                    .sales
                                    .send(EventType::added(SaleEventData { sales: added }));
                                Some(sales_changed)
                            }
                            Err(e) => {
                                error!(error = ?e, item_id = item_id.0, world_id = world_id.0, "catch-up sale update failed");
                                None
                            }
                        };
                        classify_catchup(listings_changed, sales_changed)
                    }),
            )
            .buffer_unordered(50)
            .collect::<Vec<_>>()
            .await;
            for outcome in outcomes {
                tally.add(outcome);
            }
            if let Some(checkpoint) = checkpoint {
                checkpoint.record(cursor_past(item_ids, 0), &tally).await;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        tally
    }
}

/// Items whose cheapest price per quality differs between Universalis'
/// aggregated view (`theirs`, keyed by item id, absent = Universalis could not
/// answer and the item is skipped) and our stored listings (`ours`, absent =
/// we hold no listings of that (item, quality)). Sorted for stable logs.
fn drifted_items(
    theirs: HashMap<i32, (Option<i64>, Option<i64>)>,
    ours: &[ListingSummary],
) -> Vec<i32> {
    let mut our_mins: HashMap<i32, (Option<i64>, Option<i64>)> = HashMap::new();
    for summary in ours {
        let entry = our_mins.entry(summary.item_id).or_default();
        let slot = if summary.hq {
            &mut entry.1
        } else {
            &mut entry.0
        };
        *slot = Some(summary.price_per_unit as i64);
    }
    let mut drifted: Vec<i32> = theirs
        .into_iter()
        .filter(|(item_id, their_mins)| {
            our_mins.get(item_id).copied().unwrap_or_default() != *their_mins
        })
        .map(|(item_id, _)| item_id)
        .collect();
    drifted.sort_unstable();
    drifted
}

/// Items Universalis has seen updates for that we appear to have missed:
/// either absent from our recent-update list entirely, or present but with a
/// Universalis upload newer than our last ingest (allowing
/// [`UPLOAD_TIME_SLACK_SECONDS`] of slack since we store ingest time, not
/// upload time).
fn missed_updates(
    ours: Vec<Model>,
    mut theirs: Vec<WorldItemRecencyView>,
) -> Vec<WorldItemRecencyView> {
    let mut ours: Vec<CmpListing> = ours.into_iter().map(CmpListing).collect();
    ours.sort_by_key(|i| i.0.item_id);
    theirs.sort_by_key(|i| i.item_id);
    PartialDiffIterator::new(ours.into_iter(), theirs.into_iter())
        .filter_map(|entry| match entry {
            DiffItem::Right(theirs) => Some(theirs),
            DiffItem::Same(ours, theirs) => (theirs.last_upload_time.timestamp()
                > ours.0.date_time.and_utc().timestamp() + UPLOAD_TIME_SLACK_SECONDS)
                .then_some(theirs),
            DiffItem::Left(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn losing_the_lease_drops_the_in_flight_sweep() {
        struct OnDrop(Arc<AtomicBool>);
        impl Drop for OnDrop {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let (started, running) = tokio::sync::oneshot::channel();
        let work = async {
            let _guard = OnDrop(dropped.clone());
            let _ = started.send(());
            std::future::pending::<()>().await;
        };
        let lost = async {
            running.await.unwrap();
        };
        assert_eq!(run_until_lease_lost(work, lost).await, None);
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_completed_sweep_keeps_its_result_while_the_lease_is_alive() {
        assert_eq!(
            run_until_lease_lost(async { 42 }, std::future::pending()).await,
            Some(42)
        );
    }

    #[tokio::test]
    async fn a_known_lost_lease_never_polls_the_worker() {
        let polled = AtomicBool::new(false);
        let work = async { polled.store(true, Ordering::SeqCst) };
        assert_eq!(run_until_lease_lost(work, async {}).await, None);
        assert!(!polled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    #[ignore = "requires MIGRATION_TEST_DATABASE_URL"]
    async fn sweep_lease_excludes_another_session_and_releases_when_closed() {
        use sea_orm::sqlx::{postgres::PgPoolOptions, query_scalar};
        let url = std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        let mut owner = pool.acquire().await.unwrap();
        let mut contender = pool.acquire().await.unwrap();
        // Never contend with a real sweep sharing this development database.
        let key = chrono::Utc::now().timestamp_micros() ^ i64::from(std::process::id());
        let acquired: bool = query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(key)
            .fetch_one(&mut *owner)
            .await
            .unwrap();
        assert!(acquired);
        let acquired: bool = query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(key)
            .fetch_one(&mut *contender)
            .await
            .unwrap();
        assert!(!acquired);
        owner.close().await.unwrap();
        let acquired: bool = query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(key)
            .fetch_one(&mut *contender)
            .await
            .unwrap();
        assert!(
            acquired,
            "a replacement session can take over after the owner exits"
        );
        contender.close().await.unwrap();
        pool.close().await;
    }

    #[tokio::test]
    #[ignore = "requires MIGRATION_TEST_DATABASE_URL and permission to terminate its own test session"]
    async fn sweep_worker_stops_when_its_lease_session_is_terminated() {
        use sea_orm::sqlx::{postgres::PgPoolOptions, query_scalar};
        let url = std::env::var("MIGRATION_TEST_DATABASE_URL").unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        let mut owner = pool.acquire().await.unwrap();
        owner.close_on_drop();
        let own_pid: i32 = query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *owner)
            .await
            .unwrap();
        let mut guard = Arc::new(SweepLock::default()).try_claim().unwrap();
        guard.lease = Some(owner);
        // Only this test's freshly acquired connection is terminated.
        let terminated: bool = query_scalar("SELECT pg_terminate_backend($1)")
            .bind(own_pid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(terminated);
        let stopped = tokio::time::timeout(
            Duration::from_secs(25),
            guard.run(std::future::pending::<()>()),
        )
        .await
        .expect("lease heartbeat must stop the worker");
        assert_eq!(stopped, None);
        drop(guard);
        pool.close().await;
    }
    use chrono::{DateTime, Local};

    const WORLD_ID: i32 = 34;

    fn universalis_status(status: u16) -> anyhow::Error {
        anyhow::Error::new(universalis::Error::Status {
            status,
            url: "https://universalis.app/api/v2/aggregated/Ravana/5".to_string(),
            body: String::new(),
        })
    }

    /// The probe's failure arm classifies through `anyhow`, which erases the
    /// concrete type — the downcast has to survive that or every upstream
    /// hiccup is reported as an application error again.
    #[test]
    fn transient_universalis_failures_are_classified_through_anyhow() {
        assert!(universalis_transient(&universalis_status(504)));
        assert!(universalis_transient(&universalis_status(429)));
        assert!(!universalis_transient(&universalis_status(404)));
        assert!(universalis_not_found(&universalis_status(404)));
        // A database failure in the same arm is a real error, not upstream noise.
        assert!(!universalis_transient(&anyhow::anyhow!(
            "connection pool closed"
        )));
    }

    fn bare_status(status: u16) -> universalis::Error {
        universalis::Error::Status {
            status,
            url: "https://universalis.app/api/v2/aggregated/Ravana/5".to_string(),
            body: String::new(),
        }
    }

    /// Paused tokio time: `sleep` auto-advances, so the 5s/15s/45s backoff runs
    /// instantly while still exercising the real await points.
    #[tokio::test(start_paused = true)]
    async fn retry_transient_retries_transient_errors_until_success() {
        let mut attempts = 0;
        let result = retry_transient(|| {
            attempts += 1;
            let out = if attempts < 3 {
                Err(bare_status(504))
            } else {
                Ok(42)
            };
            async move { out }
        })
        .await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts, 3);
    }

    #[tokio::test(start_paused = true)]
    async fn retry_transient_gives_up_after_backoff_is_exhausted() {
        let mut attempts = 0;
        let result: Result<i32, _> = retry_transient(|| {
            attempts += 1;
            async { Err(bare_status(429)) }
        })
        .await;
        assert!(result.unwrap_err().is_transient());
        // 1 initial attempt + one retry per backoff entry.
        assert_eq!(attempts, 1 + CHUNK_RETRY_BACKOFF.len());
    }

    #[tokio::test(start_paused = true)]
    async fn retry_transient_fails_non_transient_errors_immediately() {
        let mut attempts = 0;
        let result: Result<i32, _> = retry_transient(|| {
            attempts += 1;
            async { Err(bare_status(404)) }
        })
        .await;
        assert!(result.unwrap_err().is_not_found());
        assert_eq!(attempts, 1);
    }

    fn ours(item_id: i32, ingested_at: i64) -> Model {
        Model {
            item_id,
            world_id: WORLD_ID,
            date_time: DateTime::from_timestamp(ingested_at, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    fn theirs(item_id: i32, uploaded_at: i64) -> WorldItemRecencyView {
        WorldItemRecencyView {
            item_id,
            last_upload_time: DateTime::from_timestamp(uploaded_at, 0)
                .unwrap()
                .with_timezone(&Local),
            world_id: WORLD_ID,
            world_name: None,
        }
    }

    fn summary(item_id: i32, hq: bool, price: i32) -> ListingSummary {
        ListingSummary {
            item_id,
            hq,
            price_per_unit: price,
            world_id: WORLD_ID,
        }
    }

    #[test]
    fn claim_confirm_release_slot_lifecycle() {
        let mut slots = HashMap::new();
        let t0 = Instant::now();

        // Free slot claims; a claimed-but-unconfirmed slot refuses re-claims.
        assert!(claim_slot(&mut slots, WORLD_ID, t0));
        assert!(!claim_slot(&mut slots, WORLD_ID, t0));

        // Released without confirming (sweep died): immediately claimable again —
        // the failed sweep must not burn the 6h cooldown (spec §3).
        release_slot(&mut slots, WORLD_ID);
        assert!(claim_slot(&mut slots, WORLD_ID, t0));

        // Confirmed: cooldown holds until FULL_SWEEP_COOLDOWN has elapsed.
        confirm_slot(&mut slots, WORLD_ID, t0);
        assert!(!claim_slot(
            &mut slots,
            WORLD_ID,
            t0 + FULL_SWEEP_COOLDOWN - Duration::from_secs(1)
        ));
        assert!(claim_slot(&mut slots, WORLD_ID, t0 + FULL_SWEEP_COOLDOWN));
    }

    /// `do_full_world_sweep` decides confirm-vs-release per world via
    /// `CatchupTally::made_progress`; this pins that decision at the slot
    /// level (spec §3: "a failed saturation-triggered sweep no longer costs
    /// the world its 6-hour slot") without constructing an `UpdateService`.
    #[test]
    fn a_fully_failed_world_leaves_the_slot_claimable() {
        let mut slots = HashMap::new();
        let t0 = Instant::now();
        assert!(claim_slot(&mut slots, WORLD_ID, t0));

        // Every chunk skipped, nothing recovered — do_full_world_sweep's
        // else-arm releases rather than confirms.
        let all_chunks_skipped = CatchupTally {
            changed: 0,
            noop: 0,
            failed: 0,
            chunks_failed: 3,
        };
        assert!(!all_chunks_skipped.made_progress());
        release_slot(&mut slots, WORLD_ID);

        assert!(
            claim_slot(&mut slots, WORLD_ID, t0),
            "a fully-failed sweep must not burn the cooldown"
        );
    }

    /// The counterpart to the above: any progress at all — even a lone write
    /// failure with no changes or no-ops — confirms the cooldown so a mostly-
    /// successful sweep doesn't immediately re-hammer Universalis.
    #[test]
    fn partial_progress_still_confirms_the_cooldown() {
        let mut slots = HashMap::new();
        let t0 = Instant::now();
        assert!(claim_slot(&mut slots, WORLD_ID, t0));

        let one_failed_write = CatchupTally {
            changed: 0,
            noop: 0,
            failed: 1,
            chunks_failed: 2,
        };
        assert!(one_failed_write.made_progress());
        confirm_slot(&mut slots, WORLD_ID, t0);

        assert!(!claim_slot(
            &mut slots,
            WORLD_ID,
            t0 + FULL_SWEEP_COOLDOWN - Duration::from_secs(1)
        ));
    }

    #[test]
    fn release_does_not_clear_a_confirmed_cooldown() {
        let mut slots = HashMap::new();
        let t0 = Instant::now();
        assert!(claim_slot(&mut slots, WORLD_ID, t0));
        confirm_slot(&mut slots, WORLD_ID, t0);
        // A stray release (e.g. an error path running after completion) must not
        // reopen the world for immediate re-sweeping.
        release_slot(&mut slots, WORLD_ID);
        assert!(!claim_slot(
            &mut slots,
            WORLD_ID,
            t0 + Duration::from_secs(1)
        ));
    }

    #[test]
    fn slots_are_per_world() {
        let mut slots = HashMap::new();
        let t0 = Instant::now();
        assert!(claim_slot(&mut slots, 1, t0));
        assert!(claim_slot(&mut slots, 2, t0));
    }

    #[test]
    fn matching_mins_are_not_drifted() {
        let theirs = HashMap::from([(1, (Some(100), Some(250)))]);
        let ours = [summary(1, false, 100), summary(1, true, 250)];
        assert!(drifted_items(theirs, &ours).is_empty());
    }

    /// The #1178 signature: a sold listing's removal never reached us, so we
    /// still hold a cheaper row than the live board.
    #[test]
    fn phantom_cheap_listing_is_drifted() {
        let theirs = HashMap::from([(1, (Some(53000), None))]);
        let ours = [summary(1, false, 27000)];
        assert_eq!(drifted_items(theirs, &ours), [1]);
    }

    /// Drift in one quality flags the item even when the other quality agrees.
    #[test]
    fn single_quality_drift_is_enough() {
        let theirs = HashMap::from([(1, (Some(100), Some(250)))]);
        let ours = [summary(1, false, 100), summary(1, true, 200)];
        assert_eq!(drifted_items(theirs, &ours), [1]);
    }

    /// Their board is empty but we still hold rows (a fully-bought-out item
    /// whose removals were all lost), and the mirror case where they have
    /// listings we never stored — both directions must flag.
    #[test]
    fn presence_mismatch_in_either_direction_is_drifted() {
        let theirs = HashMap::from([(1, (None, None)), (2, (Some(500), None))]);
        let ours = [summary(1, false, 40)];
        assert_eq!(drifted_items(theirs, &ours), [1, 2]);
    }

    /// Both sides empty (item in the recency window because of a sale on an
    /// empty board) is in sync, and items Universalis could not answer for
    /// (absent from `theirs`) are never flagged, whatever we hold.
    #[test]
    fn empty_both_sides_and_unanswered_items_are_ignored() {
        let theirs = HashMap::from([(1, (None, None))]);
        let ours = [summary(9, false, 40)];
        assert!(drifted_items(theirs, &ours).is_empty());
    }

    #[test]
    fn item_unknown_to_us_is_missed() {
        let missed = missed_updates(vec![ours(1, 1_000)], vec![theirs(2, 1_000)]);
        assert_eq!(missed.iter().map(|i| i.item_id).collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn item_we_ingested_after_their_upload_is_in_sync() {
        let missed = missed_updates(vec![ours(1, 1_010)], vec![theirs(1, 1_000)]);
        assert!(missed.is_empty());
    }

    #[test]
    fn item_with_newer_upload_than_our_ingest_is_missed() {
        let missed = missed_updates(
            vec![ours(1, 1_000)],
            vec![theirs(1, 1_000 + UPLOAD_TIME_SLACK_SECONDS + 1)],
        );
        assert_eq!(missed.iter().map(|i| i.item_id).collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn upload_newer_within_slack_is_in_sync() {
        let missed = missed_updates(
            vec![ours(1, 1_000)],
            vec![theirs(1, 1_000 + UPLOAD_TIME_SLACK_SECONDS)],
        );
        assert!(missed.is_empty());
    }

    /// The `listing_last_updated` marker is the *only* signal that an item still
    /// needs recovering, and it is compared against Universalis' upload time. If a
    /// failed write stamps it anyway, the item reads as in-sync from then on and no
    /// later pass retries it. That is why `update_sales` waits until the sales have
    /// landed before writing the marker, and why the catch-up loop skips the sales
    /// update entirely when the listing update failed.
    #[test]
    fn marker_bumped_by_a_failed_write_permanently_hides_the_gap() {
        let their_upload = 1_000;
        // A catch-up attempt runs well after their upload, fails to write, but
        // still stamps `listing_last_updated` with "now".
        let failed_attempt_at = their_upload + UPLOAD_TIME_SLACK_SECONDS + 500;
        let missed = missed_updates(
            vec![ours(1, failed_attempt_at)],
            vec![theirs(1, their_upload)],
        );
        assert!(
            missed.is_empty(),
            "a prematurely bumped marker makes the still-missing item invisible to catch-up"
        );
    }

    /// The whole point of the outcome label: an upload that changed nothing —
    /// no listing delta, no new sales — is upload churn, not a missed update.
    /// Only when the fetch actually altered our data did the websocket miss
    /// something.
    #[test]
    fn unchanged_fetch_is_noop_any_change_is_changed() {
        assert_eq!(classify_catchup(false, Some(false)), CatchupOutcome::Noop);
        assert_eq!(classify_catchup(true, Some(false)), CatchupOutcome::Changed);
        assert_eq!(classify_catchup(false, Some(true)), CatchupOutcome::Changed);
        assert_eq!(classify_catchup(true, Some(true)), CatchupOutcome::Changed);
    }

    /// A failed sales write is only `Failed` when nothing else was recovered:
    /// if the listings changed, real data landed and the item counts as
    /// `Changed` even though the sales half will be retried next cycle.
    #[test]
    fn failed_sales_write_is_failed_unless_listings_changed() {
        assert_eq!(classify_catchup(false, None), CatchupOutcome::Failed);
        assert_eq!(classify_catchup(true, None), CatchupOutcome::Changed);
    }

    #[test]
    fn tally_counts_each_outcome_separately() {
        let mut tally = CatchupTally::default();
        for outcome in [
            CatchupOutcome::Changed,
            CatchupOutcome::Noop,
            CatchupOutcome::Noop,
            CatchupOutcome::Failed,
        ] {
            tally.add(outcome);
        }
        assert_eq!(
            tally,
            CatchupTally {
                changed: 1,
                noop: 2,
                failed: 1,
                chunks_failed: 0,
            }
        );
    }

    /// `chunks_failed` counts whole skipped fetch chunks (up to 100 items each),
    /// not items — it rides the tally for aggregation but is emitted through
    /// `ultros_sweep_chunks_failed`, never `ultros_catchup_items_recovered`.
    #[test]
    fn tally_default_has_no_failed_chunks() {
        let tally = CatchupTally::default();
        assert_eq!(tally.chunks_failed, 0);
    }

    #[test]
    fn item_only_on_our_side_is_ignored() {
        let missed = missed_updates(vec![ours(1, 1_000)], vec![]);
        assert!(missed.is_empty());
    }

    #[test]
    fn unsorted_inputs_still_diff_correctly() {
        let missed = missed_updates(
            vec![ours(5, 1_000), ours(1, 1_000), ours(3, 1_000)],
            vec![theirs(3, 500), theirs(7, 1_000), theirs(1, 9_999)],
        );
        assert_eq!(missed.iter().map(|i| i.item_id).collect::<Vec<_>>(), [1, 7]);
    }

    #[test]
    fn sweep_lock_is_exclusive_and_releases_on_drop() {
        let lock = Arc::new(SweepLock::default());
        let guard = lock.try_claim().expect("free lock claims");
        assert!(
            lock.try_claim().is_none(),
            "held lock refuses a second sweep"
        );
        drop(guard);
        assert!(lock.try_claim().is_some(), "dropped guard frees the lock");
    }

    fn world_summary(name: &str, changed: u64, chunks_failed: u64) -> WorldSweepSummary {
        world_summary_with_duration(name, changed, chunks_failed, Duration::from_secs(60))
    }

    fn world_summary_with_duration(
        name: &str,
        changed: u64,
        chunks_failed: u64,
        duration: Duration,
    ) -> WorldSweepSummary {
        WorldSweepSummary {
            world_name: name.to_string(),
            tally: CatchupTally {
                changed,
                noop: 0,
                failed: 0,
                chunks_failed,
            },
            duration,
        }
    }

    /// A sweep that reached the end of every world it was given.
    fn finished_report(worlds: Vec<WorldSweepSummary>, duration: Duration) -> SweepReport {
        SweepReport {
            worlds_total: worlds.len(),
            worlds,
            duration,
            interrupted: false,
        }
    }

    #[test]
    fn sweep_report_summary_flags_the_slowest_world() {
        let report = finished_report(
            vec![
                world_summary_with_duration("Sargatanas", 1, 0, Duration::from_secs(30)),
                world_summary_with_duration("Ravana", 1, 0, Duration::from_secs(150)),
                world_summary_with_duration("Cerberus", 1, 0, Duration::from_secs(45)),
            ],
            Duration::from_secs(225),
        );
        let text = report.summary_text();
        assert!(text.contains("Slowest world: Ravana (2m30s)"), "{text}");
    }

    #[test]
    fn sweep_report_summary_totals_and_flags_incomplete_worlds() {
        let report = finished_report(
            vec![
                world_summary("Sargatanas", 10, 0),
                world_summary("Ravana", 5, 2),
                world_summary("Cerberus", 0, 1),
            ],
            Duration::from_secs(2 * 3600 + 90),
        );
        let text = report.summary_text();
        assert!(text.contains("3 worlds"));
        assert!(text.contains("15"), "total changed items: {text}");
        assert!(text.contains("3 chunks skipped"), "{text}");
        assert!(
            text.contains("Ravana") && text.contains("Cerberus"),
            "{text}"
        );
        assert!(
            !text.contains("Sargatanas"),
            "clean worlds are not listed: {text}"
        );
        assert!(text.len() <= 2000, "must fit one Discord message: {text}");
    }

    #[test]
    fn sweep_report_summary_caps_the_incomplete_world_list() {
        let worlds: Vec<_> = (0..40)
            .map(|i| world_summary(&format!("World{i}"), 1, 1))
            .collect();
        let report = finished_report(worlds, Duration::from_secs(3600));
        let text = report.summary_text();
        assert!(text.contains("+30 more"), "{text}");
        assert!(text.len() <= 2000, "must fit one Discord message: {text}");
    }

    /// A sweep stopped by a deploy has to say so rather than reporting the
    /// worlds it got through as a finished run — the numbers are a fraction of
    /// the sweep, and the operator's next question is whether they have to
    /// start it again (they do not).
    #[test]
    fn an_interrupted_sweep_reports_how_far_it_got_and_that_it_resumes() {
        let report = SweepReport {
            worlds: vec![world_summary("Sargatanas", 10, 0)],
            worlds_total: 90,
            duration: Duration::from_secs(45 * 60),
            interrupted: true,
        };
        let text = report.summary_text();
        assert!(!report.is_complete());
        assert!(text.contains("paused at 1/90 worlds"), "{text}");
        assert!(text.contains("where it left off"), "{text}");
        assert!(
            !text.contains("finished"),
            "an interrupted sweep is not a finished one: {text}"
        );
    }

    #[test]
    fn a_completed_sweep_is_reported_as_finished() {
        let report = finished_report(
            vec![world_summary("Sargatanas", 10, 0)],
            Duration::from_secs(60),
        );
        assert!(report.is_complete());
        assert!(report.summary_text().contains("finished"));
    }

    /// Every marketable item, ascending. The resume cursor is an index into
    /// this order, and `xiv_gen_db`'s item table is a `HashMap` whose
    /// iteration order differs per process — so without the sort a sweep would
    /// resume at an item id that meant something else in the process that
    /// recorded it.
    #[test]
    fn the_sweeps_item_order_is_stable_across_processes() {
        let items = UpdateService::all_marketable_items();
        assert!(!items.is_empty(), "the game has marketable items");
        assert!(
            items.windows(2).all(|pair| pair[0] < pair[1]),
            "item ids must be sorted and unique"
        );
    }

    #[test]
    fn a_world_resumes_at_the_first_item_it_has_not_swept() {
        let items = [10, 20, 30, 40];
        assert_eq!(resume_offset(&items, 0), 0, "a fresh world starts at 0");
        assert_eq!(resume_offset(&items, 30), 2);
        assert_eq!(
            resume_offset(&items, 41),
            items.len(),
            "a finished world has nothing left"
        );
    }

    /// The cursor is an item id, not an offset, so items added by a game-data
    /// bump between restarts do not shift the resume point onto other items.
    #[test]
    fn items_added_between_restarts_do_not_move_the_resume_point() {
        let before = [10, 20, 30, 40];
        let cursor = cursor_past(&before[..2], 0);
        let after = [5, 10, 15, 20, 30, 40];
        assert_eq!(
            &after[resume_offset(&after, cursor)..],
            &[30, 40],
            "everything swept before the restart stays swept"
        );
    }

    #[test]
    fn the_cursor_advances_past_an_attempted_chunk() {
        assert_eq!(cursor_past(&[10, 20, 30], 0), 31);
        assert_eq!(
            cursor_past(&[], 77),
            77,
            "nothing left to attempt leaves the cursor alone"
        );
        assert_eq!(
            cursor_past(&[i32::MAX], 0),
            i32::MAX,
            "the last item in the game does not overflow the cursor"
        );
    }

    /// A sweep spanning two processes has to report the sum of both, not the
    /// share of whichever one happened to finish it.
    #[test]
    fn progress_accumulates_across_restarts() {
        let prior = market_sweep_world::Model {
            changed: 7,
            noop: 3,
            failed: 1,
            chunks_failed: 2,
            elapsed_ms: 5_000,
            ..new_world_progress(1, 21)
        };
        let run = CatchupTally {
            changed: 4,
            noop: 1,
            failed: 0,
            chunks_failed: 1,
        };
        let merged = merged_progress(&prior, &run, 900, Duration::from_secs(2), None);
        assert_eq!(merged.sweep_id, 1);
        assert_eq!(merged.world_id, 21);
        assert_eq!(merged.next_item_id, 900);
        assert_eq!((merged.changed, merged.noop), (11, 4));
        assert_eq!((merged.failed, merged.chunks_failed), (1, 3));
        assert_eq!(merged.elapsed_ms, 7_000);
        assert!(
            merged.completed_at.is_none(),
            "a mid-world checkpoint does not finish the world"
        );
    }

    /// Worlds swept before a restart are read back out of the database, so
    /// they still count toward the closing report.
    #[test]
    fn a_restored_world_reports_the_totals_it_was_stored_with() {
        let row = market_sweep_world::Model {
            changed: 12,
            chunks_failed: 2,
            elapsed_ms: 90_000,
            ..new_world_progress(1, 21)
        };
        let summary = WorldSweepSummary::from_progress("Sargatanas", &row);
        assert_eq!(summary.tally.changed, 12);
        assert_eq!(summary.tally.chunks_failed, 2);
        assert_eq!(summary.duration, Duration::from_secs(90));
    }

    #[test]
    fn sweep_progress_summary_mentions_counts() {
        let text = SweepProgress {
            worlds_done: 42,
            worlds_total: 90,
            items_changed: 1234,
            chunks_failed: 3,
        }
        .summary_text();
        assert!(text.contains("42/90"), "{text}");
        assert!(text.contains("1234"), "{text}");
        // Plain `contains("3")` would pass on the "3" inside "1234" above
        // regardless of `chunks_failed` — assert the distinctive phrase
        // `summary_text` actually emits instead.
        assert!(text.contains("3 chunks skipped"), "{text}");
    }
}
