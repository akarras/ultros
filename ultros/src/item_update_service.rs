use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures::{StreamExt, stream};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, instrument, warn};
use ultros_api_types::websocket::{ListingEventData, SaleEventData};
use ultros_db::{
    UltrosDb,
    common::partial_diff_iterator::{DiffItem, PartialDiffIterator},
    entity::{listing_last_updated::Model, world},
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
/// No production call site exists yet: the only legitimate caller is the
/// "a full sweep is already running elsewhere" branch that the saturation
/// branch introduces around the claim in `check_for_missed_items_on_world`.
/// Exercised directly by `mod tests` until then.
// TODO(task-5): remove this allow — `release_slot` gets its caller (via
// `release_full_sweep_slot`) when the saturation branch's else-arm lands.
#[allow(dead_code)]
fn release_slot(slots: &mut HashMap<i32, SweepSlot>, world_id: i32) {
    if let Some(SweepSlot::Running) = slots.get(&world_id) {
        slots.remove(&world_id);
    }
}

/// Serializes full sweeps (manual and saturation-triggered): a full sweep
/// fetches every marketable item for a world, and two at once doubles the
/// load on Universalis for zero extra coverage.
///
/// No production caller yet: only `try_begin_full_sweep` claims it, and
/// nothing calls that until Task 5 wires it into the saturation branch's
/// "already running elsewhere" check and Task 6 wires it into
/// `/rescan_market`. Exercised directly by `mod tests` until then.
// TODO(task-5): remove this allow — `try_begin_full_sweep` gets its first
// caller when the saturation branch's else-arm lands.
#[allow(dead_code)]
#[derive(Default)]
pub(crate) struct SweepLock(AtomicBool);

/// Held for the duration of a full sweep; frees the lock on drop (including
/// panics, so a crashed sweep never wedges the command).
// TODO(task-5): remove this allow — constructed once `try_begin_full_sweep`
// has a caller (see `SweepLock`).
#[allow(dead_code)]
pub(crate) struct SweepLockGuard(Arc<SweepLock>);

impl SweepLock {
    // TODO(task-5): remove this allow — called from `try_begin_full_sweep`
    // once the saturation branch wires that in.
    #[allow(dead_code)]
    pub(crate) fn try_claim(self: &Arc<Self>) -> Option<SweepLockGuard> {
        self.0
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| SweepLockGuard(self.clone()))
    }
}

impl Drop for SweepLockGuard {
    fn drop(&mut self) {
        self.0.0.store(false, Ordering::SeqCst);
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
    /// Per-world full-sweep slot: `Running` while a sweep is in flight,
    /// `CompletedAt` while the [`FULL_SWEEP_COOLDOWN`] from the last
    /// completed sweep is still active. See [`SweepSlot`].
    pub(crate) full_sweep_cooldowns: Mutex<HashMap<i32, SweepSlot>>,
    /// Worlds Universalis 404s on. Purely a log de-duplicator — see
    /// [`UpdateService::note_world_uncovered`].
    pub(crate) uncovered_worlds: Mutex<HashSet<i32>>,
    /// Serializes full sweeps — see [`SweepLock`].
    // TODO(task-5): remove this allow — read once `try_begin_full_sweep` has
    // a caller (the saturation branch's else-arm).
    #[allow(dead_code)]
    pub(crate) sweep_lock: Arc<SweepLock>,
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
}

/// One world's outcome from a full sweep, folded into a [`SweepReport`].
///
/// `duration` isn't read by [`SweepReport::summary_text`] today — it's kept
/// per-world for Task 6's richer Discord report (e.g. flagging an
/// unusually slow world). No production reader exists until then.
// TODO(task-6): remove this allow — `duration` gets a reader when
// `/rescan_market`'s rewrite reads per-world timing.
#[allow(dead_code)]
pub(crate) struct WorldSweepSummary {
    world_name: String,
    tally: CatchupTally,
    duration: std::time::Duration,
}

/// Fired after each world completes during [`UpdateService::do_full_world_sweep`]
/// so a long-running sweep can report interim status (e.g. edit a Discord
/// reply). This task's `admin.rs` caller passes a no-op `|_| {}` (Task 6
/// replaces that command wholesale), so nothing reads these fields or calls
/// `summary_text` in production yet — exercised directly by `mod tests`
/// until Task 6 wires a real progress callback.
// TODO(task-6): remove this allow — these fields get a reader when
// `/rescan_market`'s rewrite passes a real progress callback instead of `|_| {}`.
#[allow(dead_code)]
pub(crate) struct SweepProgress {
    worlds_done: usize,
    worlds_total: usize,
    items_changed: u64,
    chunks_failed: u64,
}

impl SweepProgress {
    /// See the struct doc-comment: no production caller until Task 6 wires a
    /// real progress callback.
    // TODO(task-6): remove this allow — called once `/rescan_market`'s
    // rewrite has a real progress callback that formats interim status.
    #[allow(dead_code)]
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

/// Result of a completed full sweep across every world, returned by
/// [`UpdateService::do_full_world_sweep`].
pub(crate) struct SweepReport {
    worlds: Vec<WorldSweepSummary>,
    duration: std::time::Duration,
}

impl SweepReport {
    pub(crate) fn summary_text(&self) -> String {
        let changed: u64 = self.worlds.iter().map(|w| w.tally.changed).sum();
        let failed: u64 = self.worlds.iter().map(|w| w.tally.failed).sum();
        let chunks_failed: u64 = self.worlds.iter().map(|w| w.tally.chunks_failed).sum();
        let minutes = self.duration.as_secs() / 60;
        let mut text = format!(
            "Full market sweep finished: {} worlds in {minutes} min — {changed} items updated, {failed} item writes failed, {chunks_failed} chunks skipped.",
            self.worlds.len()
        );
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

    pub(crate) fn all_marketable_items() -> Box<[i32]> {
        xiv_gen_db::data()
            .items
            .values()
            .filter(|i| i.item_search_category != 0)
            .map(|i| i.key_id.0)
            .collect()
    }

    /// Claims the global full-sweep lock. `None` when a sweep (manual or
    /// saturation-triggered) is already running. See [`SweepLock`].
    // TODO(task-5): remove this allow — called from the saturation branch's
    // else-arm in `check_for_missed_items_on_world` once Task 5 wires it in
    // (Task 6 adds a second caller from `/rescan_market`).
    #[allow(dead_code)]
    pub(crate) fn try_begin_full_sweep(&self) -> Option<SweepLockGuard> {
        self.sweep_lock.try_claim()
    }

    /// Sweeps over every single marketable item in the game, ignoring the
    /// recency cache. Only should be used if data is known to be lost. Never
    /// aborts: failed chunks are skipped and reported via the returned
    /// [`SweepReport`]. `progress` fires after each world completes.
    ///
    /// Callers must hold a [`SweepLockGuard`] (see
    /// [`UpdateService::try_begin_full_sweep`]) so only one full sweep runs.
    pub(crate) async fn do_full_world_sweep(
        &self,
        mut progress: impl FnMut(SweepProgress),
    ) -> SweepReport {
        let all_marketable_items = Self::all_marketable_items();
        let worlds: Vec<&world::Model> = self.world_cache.get_all_worlds().copied().collect();
        let worlds_total = worlds.len();
        let started = Instant::now();
        let mut summaries = Vec::with_capacity(worlds_total);
        let (mut items_changed, mut chunks_failed) = (0u64, 0u64);
        for world in worlds {
            let world_started = Instant::now();
            info!(world = %world.name, "full sweep: scanning world");
            let tally = self.check_items(world, &all_marketable_items).await;
            tally.record(&world.name);
            // This world just got a full refetch — a saturation-triggered sweep
            // inside the cooldown window would be pure duplication.
            self.confirm_full_sweep(world.id);
            items_changed += tally.changed;
            chunks_failed += tally.chunks_failed;
            summaries.push(WorldSweepSummary {
                world_name: world.name.clone(),
                tally,
                duration: world_started.elapsed(),
            });
            progress(SweepProgress {
                worlds_done: summaries.len(),
                worlds_total,
                items_changed,
                chunks_failed,
            });
        }
        SweepReport {
            worlds: summaries,
            duration: started.elapsed(),
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
    /// Not yet called: its call site is the "a full sweep is already running
    /// elsewhere" branch that the saturation branch adds around the claim
    /// below. See `release_slot`.
    // TODO(task-5): remove this allow — this gets its caller when the
    // saturation branch's else-arm lands.
    #[allow(dead_code)]
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
                // TODO(task-5): wrap this claim with the global concurrency
                // guard (`try_begin_full_sweep`) and call `release_full_sweep_slot`
                // in the "already running elsewhere" branch below; until then
                // the claim/confirm pair alone still fixes the
                // cooldown-on-failure bug Task 3 targeted.
                warn!(world = %world.name, "recency window saturated, running full item sweep");
                let tally = self.check_items(world, &Self::all_marketable_items()).await;
                tally.record(&world.name);
                self.confirm_full_sweep(world.id);
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

    async fn check_items(&self, world: &world::Model, item_ids: &[i32]) -> CatchupTally {
        let world_id = WorldId(world.id);
        let world_name = &world.name;
        let mut tally = CatchupTally::default();
        let total_chunks = item_ids.chunks(100).len();
        for (chunk_index, item_ids) in item_ids.chunks(100).enumerate() {
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
                            Ok((added, removed)) => {
                                listings_changed = !added.is_empty() || !removed.is_empty();
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
        WorldSweepSummary {
            world_name: name.to_string(),
            tally: CatchupTally {
                changed,
                noop: 0,
                failed: 0,
                chunks_failed,
            },
            duration: Duration::from_secs(60),
        }
    }

    #[test]
    fn sweep_report_summary_totals_and_flags_incomplete_worlds() {
        let report = SweepReport {
            worlds: vec![
                world_summary("Sargatanas", 10, 0),
                world_summary("Ravana", 5, 2),
                world_summary("Cerberus", 0, 1),
            ],
            duration: Duration::from_secs(2 * 3600 + 90),
        };
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
        let report = SweepReport {
            worlds,
            duration: Duration::from_secs(3600),
        };
        let text = report.summary_text();
        assert!(text.contains("+30 more"), "{text}");
        assert!(text.len() <= 2000, "must fit one Discord message: {text}");
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
        assert!(text.contains("3"), "{text}");
    }
}
