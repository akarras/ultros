//! Periodic reconciliation: the source of truth for Discord-backed group
//! membership.
//!
//! Gateway events (see [`super::events`]) keep membership fresh between runs,
//! but they can be missed — a dropped shard, a restart, an event Discord never
//! sent. This walks the guild from scratch and makes the database match, so
//! any drift is bounded by the cycle interval rather than lasting forever.
//!
//! ## What racing a gateway event actually guarantees
//!
//! Both paths apply idempotent plans through the same DB primitive, but that
//! alone does *not* make the order irrelevant: reconciliation's plan is
//! computed from a member snapshot taken minutes earlier and applied later, so
//! a gateway event landing in that window is newer information than the plan
//! that is about to overwrite it.
//!
//! What holds:
//!
//! - **Removes cannot undo a newer add.** Reconciliation applies through
//!   `apply_role_sync_from_snapshot`, which skips removing any role membership
//!   created after the snapshot was taken. That is the direction that loses
//!   access, and it is closed in the database, so it holds across processes.
//! - **Two reconciles of one guild do not overlap** in this process; see
//!   [`begin_reconcile`].
//!
//! What does not hold: a reconcile whose snapshot predates a role *removal*
//! can still re-add the member the gateway just removed. That grants access
//! rather than losing it, and the next cycle — which snapshots after the
//! event — takes it back, so the divergence is bounded by
//! [`RECONCILE_INTERVAL`] and never silent about membership somebody had.

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use poise::serenity_prelude::{self as serenity, GuildId};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use ultros_db::UltrosDb;
use ultros_db::group_roles::{RoleSyncPlan, SyncSummary};

use super::diff::{self, GuildMember};

/// Discord's maximum page size for `GET /guilds/{id}/members`.
const MEMBER_PAGE_SIZE: u64 = 1000;

/// Discord's own ceiling on guild size, so a guild this bot is in cannot
/// exceed it. Reaching the cap means the cursor is not advancing the way we
/// think it is, and the pages we did get are not the guild.
const MEMBER_HARD_CAP: usize = 250_000;

/// How often every guild with a Discord-backed role is walked.
const RECONCILE_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// How long to wait for the gateway before giving up on a reconcile cycle.
///
/// The first tick can beat the bot's connection — a slow start, a Discord
/// outage at boot — and without this the next attempt is a whole
/// [`RECONCILE_INTERVAL`] away, which is six hours of a freshly deployed
/// instance never syncing anything.
const CTX_RETRY_DELAY: Duration = Duration::from_secs(30);
const CTX_RETRIES: usize = 20;

/// The gateway needs time to connect before the first cycle, and a cold start
/// already has plenty else to do. Reconciliation is a background correctness
/// pass, not a startup dependency.
const RECONCILE_STARTUP_DELAY: Duration = Duration::from_secs(5 * 60);

/// Breathing room between guilds so one cycle does not empty the Discord rate
/// limit budget that user-facing requests share.
const BETWEEN_GUILDS: Duration = Duration::from_secs(2);

/// A 403 on the members endpoint means the Server Members intent is off. That
/// is a deploy-wide misconfiguration, identical on every guild and every
/// cycle, so it is worth one loud line and nothing after it.
static MEMBERS_FORBIDDEN_REPORTED: AtomicBool = AtomicBool::new(false);

/// What one guild's reconcile did, for the summary log.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GuildSyncReport {
    pub roles: usize,
    pub orphaned: usize,
    /// Roles that were orphaned and are syncing again.
    pub restored: usize,
    pub members_seen: usize,
    pub added: usize,
    pub removed: usize,
    pub left_group: usize,
    pub handed_over: usize,
    pub names_refreshed: usize,
}

impl GuildSyncReport {
    fn absorb(&mut self, summary: SyncSummary) {
        self.added += summary.added;
        self.removed += summary.removed;
        self.left_group += summary.left_group;
        self.handed_over += summary.handed_over;
    }
}

/// True when a Discord failure is expected to clear on its own, following the
/// repo's `is_transient` convention (see `item_update_service`): rate limits,
/// server-side errors, and anything that never got an HTTP response at all.
///
/// A 4xx that is not 429 is a real answer from Discord — missing permissions,
/// a guild we are no longer in — and retrying it changes nothing.
pub(crate) fn is_transient(error: &serenity::Error) -> bool {
    match error {
        serenity::Error::Http(http) => {
            transient_status(http.status_code().map(|status| status.as_u16()))
        }
        serenity::Error::Io(_) => true,
        // Decode, model and format errors are bugs or schema drift, not
        // weather. Retrying them just repeats the same failure.
        _ => false,
    }
}

/// Split out from [`is_transient`] so the classification is testable:
/// serenity's `ErrorResponse` is `#[non_exhaustive]`, so an HTTP status cannot
/// be constructed from outside that crate.
///
/// `None` means the request never got a response at all — a connect failure,
/// a TLS error, a timeout — which is the transient case by definition.
fn transient_status(status: Option<u16>) -> bool {
    match status {
        Some(status) => status == 429 || (500..600).contains(&status),
        None => true,
    }
}

/// Whether Discord answered "no", as opposed to "not right now".
fn is_forbidden(error: &serenity::Error) -> bool {
    matches!(
        error,
        serenity::Error::Http(http)
            if http.status_code().is_some_and(|status| status.as_u16() == 403)
    )
}

/// Classify and log a guild's reconcile failure. Membership is left exactly as
/// it was either way: a guild we could not read is not a guild that emptied.
fn report_failure(guild_id: i64, error: &anyhow::Error) {
    match error.downcast_ref::<serenity::Error>() {
        // The one loud line from `note_members_failure` already says what a
        // 403 means. Repeating it per guild per cycle would only bury it.
        Some(discord) if is_forbidden(discord) => {
            warn!(guild_id, "skipping guild sync, Discord refused the request")
        }
        Some(discord) if is_transient(discord) => {
            warn!(
                guild_id,
                "skipping guild sync, Discord is unhappy: {discord}"
            )
        }
        _ => error!(guild_id, "guild sync failed: {error:?}"),
    }
}

/// Note a members-endpoint failure that names a misconfigured intent, once.
fn note_members_failure(guild_id: i64, error: &serenity::Error) {
    if is_forbidden(error) && !MEMBERS_FORBIDDEN_REPORTED.swap(true, Ordering::Relaxed) {
        error!(
            guild_id,
            "Discord refused the guild member list (403). Group membership sync needs the \
             Server Members intent enabled for this bot in the Discord developer portal; \
             until it is, no group will receive members from Discord."
        );
    }
}

/// Whether pagination should ask for another page after this one.
///
/// Split out from [`fetch_guild_members`] so the two conditions that must
/// *fail* rather than truncate are testable without a gateway connection. Both
/// of them mean the same thing — the pages collected so far are not the guild
/// — and a partial member list is exactly the input that turns
/// reconciliation into a mass removal, so neither may return the members it
/// has.
fn wants_another_page(
    fetched: usize,
    page_len: usize,
    highest: u64,
    after: Option<u64>,
) -> Result<bool> {
    // Discord pages members by ascending user id and returns a short page at
    // the end. This is the only way the walk finishes with the whole guild.
    if page_len < MEMBER_PAGE_SIZE as usize {
        return Ok(false);
    }
    if Some(highest) <= after {
        anyhow::bail!("Discord member pagination did not advance");
    }
    if fetched >= MEMBER_HARD_CAP {
        // Not a stopping point: a guild cannot be this large, so the cursor is
        // misbehaving and what we hold is a truncated prefix. Returning it
        // would put every member past the cap into `plan.removes` and drop
        // them from the role — and from the group. Fail instead, so the guild
        // is skipped and existing membership is left untouched.
        anyhow::bail!(
            "Discord member pagination passed the {MEMBER_HARD_CAP} hard cap at {fetched} \
             members, which no guild reaches; refusing to treat a truncated list as the guild"
        );
    }
    Ok(true)
}

/// Every member of the guild, paginated at Discord's maximum page size.
///
/// Either the full membership or an error. There is no third answer: a partial
/// list is indistinguishable from a shrunken guild once it reaches [`diff`].
async fn fetch_guild_members(
    ctx: &serenity::Context,
    guild_id: GuildId,
) -> Result<Vec<GuildMember>> {
    let mut members = Vec::new();
    let mut after: Option<u64> = None;
    loop {
        let page = ctx
            .http
            .get_guild_members(guild_id, Some(MEMBER_PAGE_SIZE), after)
            .await
            .inspect_err(|error| note_members_failure(guild_id.get() as i64, error))?;
        let page_len = page.len();
        let mut highest = after.unwrap_or(0);
        for member in &page {
            let id = member.user.id.get();
            highest = highest.max(id);
            members.push(GuildMember {
                user_id: id as i64,
                display_name: member.display_name().to_string(),
                role_ids: member.roles.iter().map(|role| role.get() as i64).collect(),
            });
        }
        if !wants_another_page(members.len(), page_len, highest, after)? {
            break;
        }
        after = Some(highest);
    }
    Ok(members)
}

/// Reconcile one guild against Discord. `Ok(None)` means the guild has nothing
/// Discord-backed and was skipped without touching Discord at all.
pub(crate) async fn reconcile_guild(
    db: &UltrosDb,
    ctx: &serenity::Context,
    guild_id: i64,
) -> Result<Option<GuildSyncReport>> {
    // Orphaned roles are included so a role that stopped syncing can start
    // again; see `restore_roles`.
    let backed = db.discord_roles_for_guild(guild_id).await?;
    if backed.is_empty() {
        return Ok(None);
    }
    let mut report = GuildSyncReport::default();

    let discord_guild = GuildId::new(u64::try_from(guild_id)?);
    let live: HashSet<i64> = discord_guild
        .roles(&ctx.http)
        .await?
        .into_keys()
        .map(|role| role.get() as i64)
        .collect();
    if live.is_empty() {
        // Every guild has `@everyone`, so an empty listing is a bad response,
        // not a guild without roles — and believing it would orphan every
        // synced role of the guild in one pass.
        anyhow::bail!("Discord listed no roles for this guild, which cannot be true");
    }

    // A role Discord no longer has stops syncing but keeps its members, so the
    // list share pointing at it does not silently lose everyone.
    let mut roles = Vec::with_capacity(backed.len());
    let mut restore = Vec::new();
    for entry in backed {
        // `@everyone` is returned by the roles endpoint, but orphaning it by
        // accident would silently switch off whole-server sync, so it is never
        // a candidate for orphaning on the strength of a role listing.
        if diff::is_everyone_role(guild_id, entry.role.discord_role_id)
            || live.contains(&entry.role.discord_role_id)
        {
            if entry.orphaned {
                restore.push(entry.role.role_id);
            }
            roles.push(entry.role);
        } else if !entry.orphaned {
            db.mark_role_orphaned(guild_id, entry.role.discord_role_id)
                .await?;
            report.orphaned += 1;
        }
    }
    report.restored = db.restore_roles(restore).await?;
    if report.restored > 0 {
        info!(
            guild_id,
            restored = report.restored,
            "Discord listed these roles again; they are syncing once more"
        );
    }
    report.roles = roles.len();
    if roles.is_empty() {
        return Ok(Some(report));
    }

    // Taken before the fetch, not after: anything that changes while we are
    // reading Discord is newer than what we are reading, and the apply step
    // uses this to refuse to undo it.
    let snapshot_at = chrono::Utc::now();
    let members = fetch_guild_members(ctx, discord_guild).await?;
    report.members_seen = members.len();
    // Nothing below runs against a member list we do not believe.
    diff::check_member_list(&members)?;

    // Every user we already had in one of these roles: their `discord_user`
    // row certainly exists, which is what makes the bulk name refresh below
    // safe to run over them.
    //
    // Note that every plan is computed, and every plan is checked, before any
    // of them is applied: a snapshot that produced one implausible plan is not
    // a snapshot to trust for the guild's other roles either, so tripping the
    // breaker leaves the whole guild untouched rather than half-applied.
    let mut known: HashSet<i64> = HashSet::new();
    let mut by_group: BTreeMap<i32, Vec<RoleSyncPlan>> = BTreeMap::new();
    for role in &roles {
        let desired = diff::desired_members(guild_id, role.discord_role_id, &members);
        let current = db.role_member_ids(role.role_id).await?;
        let plan = diff::plan(role.role_id, &desired, &current);
        diff::check_removals(role.role_id, plan.removes.len(), current.len())?;
        known.extend(current.iter().copied());
        by_group.entry(role.group_id).or_default().push(plan);
    }

    for (group_id, plans) in by_group {
        report.absorb(
            db.apply_role_sync_from_snapshot(group_id, plans, snapshot_at)
                .await?,
        );
    }

    // Names of members who were already in place, which the apply step has no
    // reason to touch. Restricted to ids we already hold so this can never
    // create a row for a guild member who is in no synced role.
    let refresh: Vec<(i64, String)> = members
        .into_iter()
        .filter(|member| known.contains(&member.user_id))
        .map(|member| (member.user_id, member.display_name))
        .collect();
    report.names_refreshed = refresh.len();
    db.refresh_member_display_names(&refresh).await?;

    Ok(Some(report))
}

/// Guilds with a reconcile in flight in this process.
///
/// Nothing else serialized these: `spawn_reconcile` is fire-and-forget and the
/// role-import endpoint calls it per role, so importing three roles used to
/// start three whole-guild walks at once, each computing a plan from its own
/// snapshot and applying it over the others'.
///
/// An overlapping run is rejected rather than queued. A second walk of the
/// same guild started seconds after the first has nothing new to find, and
/// queueing would only spend Discord's rate limit budget saying so.
///
/// This is process-local, which is the honest scope of the guarantee: it stops
/// this instance racing itself, not two instances racing each other. The
/// correctness guard that does hold across processes is the snapshot timestamp
/// (`apply_role_sync_from_snapshot`), which lives in the database.
static IN_FLIGHT: LazyLock<Mutex<HashSet<i64>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Held for the whole of one guild's reconcile — fetch and apply both.
struct ReconcileGuard {
    guild_id: i64,
}

impl Drop for ReconcileGuard {
    fn drop(&mut self) {
        IN_FLIGHT
            .lock()
            .expect("reconcile guard set poisoned")
            .remove(&self.guild_id);
    }
}

/// Claim a guild for a reconcile, or `None` if one is already running.
fn begin_reconcile(guild_id: i64) -> Option<ReconcileGuard> {
    let claimed = IN_FLIGHT
        .lock()
        .expect("reconcile guard set poisoned")
        .insert(guild_id);
    // Deliberately two statements: the lock is released before a guard can
    // exist. `ReconcileGuard::drop` takes the same lock, so building one while
    // still holding it deadlocks the instant the claim fails and the
    // just-built guard is dropped.
    claimed.then(|| ReconcileGuard { guild_id })
}

/// Reconcile one guild, logging the outcome. Used by every caller that is not
/// interested in the report itself.
///
/// Returns whether a reconcile actually ran to completion, which is what the
/// rate-limit window should be keyed on: a run that never started, or that
/// Discord refused, has not synced anything and must not lock the owner out of
/// pressing the button again.
async fn reconcile_and_log(db: &UltrosDb, ctx: &serenity::Context, guild_id: i64) -> bool {
    let Some(_guard) = begin_reconcile(guild_id) else {
        debug!(
            guild_id,
            "a reconcile for this guild is already running, skipping this one"
        );
        return false;
    };
    match reconcile_guild(db, ctx, guild_id).await {
        Ok(Some(report)) => {
            info!(
                guild_id,
                roles = report.roles,
                orphaned = report.orphaned,
                restored = report.restored,
                members_seen = report.members_seen,
                added = report.added,
                removed = report.removed,
                left_group = report.left_group,
                handed_over = report.handed_over,
                names_refreshed = report.names_refreshed,
                "reconciled Discord group membership"
            );
            true
        }
        Ok(None) => {
            debug!(guild_id, "no Discord roles, nothing to reconcile");
            false
        }
        Err(error) => {
            report_failure(guild_id, &error);
            false
        }
    }
}

/// Reconcile one guild in the background. Callers on a request path use this
/// so a whole-server member walk never happens inline in an HTTP handler.
///
/// The caller has already opened the rate-limit window (it has to, to answer
/// the request), so this closes it again on every path that did not sync:
/// otherwise a reconcile that bailed because the bot was offline would still
/// have the owner told "recently synced" for five minutes, over a
/// `last_synced_at` that is null or days old.
pub(crate) fn spawn_reconcile(db: UltrosDb, guild_id: i64) {
    tokio::spawn(async move {
        let Some(ctx) = crate::alerts::delivery::get_serenity_ctx() else {
            warn!(
                guild_id,
                "cannot reconcile group membership, the Discord bot is not connected"
            );
            sync_rate_limiter().release(guild_id);
            return;
        };
        if !reconcile_and_log(&db, &ctx, guild_id).await {
            sync_rate_limiter().release(guild_id);
        }
    });
}

/// The gateway context, waiting a while for it rather than writing the whole
/// cycle off. See [`CTX_RETRY_DELAY`].
async fn wait_for_serenity_ctx(
    token: &CancellationToken,
) -> Option<std::sync::Arc<serenity::Context>> {
    for attempt in 0..CTX_RETRIES {
        if let Some(ctx) = crate::alerts::delivery::get_serenity_ctx() {
            return Some(ctx);
        }
        debug!(
            attempt,
            "the Discord bot is not connected yet, waiting before the group sync cycle"
        );
        tokio::select! {
            _ = token.cancelled() => return None,
            _ = tokio::time::sleep(CTX_RETRY_DELAY) => {}
        }
    }
    warn!(
        "skipping group membership reconciliation, the Discord bot did not connect within \
         {:?}",
        CTX_RETRY_DELAY * CTX_RETRIES as u32
    );
    None
}

/// Walk every guild with at least one Discord-backed role, sequentially.
async fn reconcile_all(db: &UltrosDb, token: &CancellationToken) {
    let Some(ctx) = wait_for_serenity_ctx(token).await else {
        return;
    };
    let guilds = match db.guilds_with_discord_roles().await {
        Ok(guilds) => guilds,
        Err(error) => {
            error!("could not list guilds to reconcile: {error:?}");
            return;
        }
    };
    debug!(guilds = guilds.len(), "starting a group sync cycle");
    for guild_id in guilds {
        if token.is_cancelled() {
            return;
        }
        // The periodic cycle deliberately does not touch the rate limiter: it
        // runs far slower than the window, and it is not what the window is
        // there to throttle.
        let _ = reconcile_and_log(db, &ctx, guild_id).await;
        tokio::select! {
            _ = token.cancelled() => return,
            _ = tokio::time::sleep(BETWEEN_GUILDS) => {}
        }
    }
}

/// The six-hourly reconciliation cycle.
pub(crate) fn spawn_reconcile_scheduler(db: UltrosDb, token: CancellationToken) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval_at(
            tokio::time::Instant::now() + RECONCILE_STARTUP_DELAY,
            RECONCILE_INTERVAL,
        );
        // A cycle that overran its slot should resume the normal cadence, not
        // immediately fire every tick it missed back to back against the same
        // Discord rate limiter that just slowed it down.
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = interval.tick() => reconcile_all(&db, &token).await,
            }
        }
    });
}

/// One owner-triggered sync per guild per [`SyncRateLimiter::WINDOW`].
///
/// Reconciling walks the whole member list, so a button anyone can hold down
/// needs a floor. The periodic cycle is not subject to this — it runs far
/// slower than the window anyway.
pub(crate) struct SyncRateLimiter {
    last_run: Mutex<BTreeMap<i64, Instant>>,
}

impl SyncRateLimiter {
    pub(crate) const WINDOW: Duration = Duration::from_secs(5 * 60);

    fn new() -> Self {
        Self {
            last_run: Mutex::new(BTreeMap::new()),
        }
    }

    /// Whether a sync may run for `guild_id` now, recording it if so.
    ///
    /// `now` is a parameter rather than read inside so the window logic is a
    /// pure decision that tests can drive without sleeping.
    pub(crate) fn try_acquire(&self, guild_id: i64, now: Instant) -> bool {
        let mut last_run = self.last_run.lock().expect("sync rate limiter poisoned");
        // Guilds stop syncing, groups get deleted, and this map would
        // otherwise keep a row per guild forever. An expired entry allows the
        // next run regardless, so dropping it changes no decision.
        last_run.retain(|_, at| now.duration_since(*at) < Self::WINDOW);
        if last_run.contains_key(&guild_id) {
            return false;
        }
        last_run.insert(guild_id, now);
        true
    }

    /// Start the window without running anything, for a sync that was just
    /// kicked off by another path (a role import). Without this, importing a
    /// role and then pressing "Sync now" would walk the whole guild twice.
    pub(crate) fn record(&self, guild_id: i64, now: Instant) {
        self.last_run
            .lock()
            .expect("sync rate limiter poisoned")
            .insert(guild_id, now);
    }

    /// Reopen the window for a run that did not happen.
    ///
    /// Both callers have to claim the window before the work starts, because
    /// they answer an HTTP request and the work is spawned. When the spawned
    /// run turns out not to sync anything — the bot is offline, Discord
    /// refused, another reconcile of the guild was already going — the claim
    /// bought nothing, and leaving it would tell the owner "recently synced"
    /// for five minutes over a `last_synced_at` that never moved.
    pub(crate) fn release(&self, guild_id: i64) {
        self.last_run
            .lock()
            .expect("sync rate limiter poisoned")
            .remove(&guild_id);
    }
}

pub(crate) fn sync_rate_limiter() -> &'static SyncRateLimiter {
    static LIMITER: LazyLock<SyncRateLimiter> = LazyLock::new(SyncRateLimiter::new);
    &LIMITER
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A guild that could not be read must be skipped and retried, never
    /// treated as a guild that emptied. Only failures that will clear on their
    /// own are worth a quiet `warn!`; a definitive refusal is a real problem.
    #[test]
    fn only_rate_limits_server_errors_and_dropped_requests_are_transient() {
        for status in [429, 500, 502, 503, 504] {
            assert!(transient_status(Some(status)), "{status} should retry");
        }
        for status in [400, 401, 403, 404] {
            assert!(!transient_status(Some(status)), "{status} is an answer");
        }
        assert!(
            transient_status(None),
            "a request that never got a response is transient"
        );
    }

    /// A short page is the only way a member walk ends successfully.
    #[test]
    fn pagination_stops_on_a_short_page() {
        assert!(!wants_another_page(1500, 500, 9_000, Some(8_000)).unwrap());
        assert!(wants_another_page(2000, MEMBER_PAGE_SIZE as usize, 9_000, Some(8_000)).unwrap());
    }

    /// Reaching the hard cap is a broken cursor, not a big guild, and the
    /// pages in hand are a truncated prefix. Truncating there used to `break`
    /// and return them, which put every member past the cap into
    /// `plan.removes` — dropping them from the role and from the group.
    #[test]
    fn passing_the_member_hard_cap_fails_instead_of_truncating() {
        let error = wants_another_page(
            MEMBER_HARD_CAP,
            MEMBER_PAGE_SIZE as usize,
            900_000,
            Some(800_000),
        )
        .expect_err("a truncated member list must not be returned as the guild");
        assert!(
            error.to_string().contains("hard cap"),
            "the failure should name the cap: {error}"
        );
    }

    /// The adjacent guard, which already behaved: a cursor that does not move
    /// would otherwise loop or silently stop short.
    #[test]
    fn a_stalled_cursor_fails() {
        assert!(
            wants_another_page(1000, MEMBER_PAGE_SIZE as usize, 8_000, Some(8_000)).is_err(),
            "a cursor that did not advance is a failure, not an end"
        );
    }

    /// Only one reconcile of a guild at a time, so a burst of role imports
    /// cannot have three snapshots of one guild racing to overwrite each
    /// other's plans.
    #[test]
    fn a_guild_reconcile_excludes_another_of_the_same_guild() {
        let guild = 4_242;
        let first = begin_reconcile(guild).expect("the first run claims the guild");
        assert!(
            begin_reconcile(guild).is_none(),
            "a second run of the same guild is rejected"
        );
        // ...but a different guild is unaffected.
        let other = begin_reconcile(guild + 1).expect("other guilds are independent");
        drop(other);
        drop(first);
        assert!(
            begin_reconcile(guild).is_some(),
            "the guild is claimable again once the run finishes"
        );
    }

    /// A reconcile that never ran must not leave the owner told "recently
    /// synced" for the next five minutes.
    #[test]
    fn releasing_reopens_the_window_for_a_run_that_did_not_happen() {
        let limiter = SyncRateLimiter::new();
        let start = Instant::now();
        assert!(limiter.try_acquire(1, start));
        assert!(!limiter.try_acquire(1, start));
        limiter.release(1);
        assert!(
            limiter.try_acquire(1, start),
            "the next press should be allowed to try again"
        );
    }

    #[test]
    fn the_first_sync_runs_and_the_second_one_inside_the_window_does_not() {
        let limiter = SyncRateLimiter::new();
        let start = Instant::now();
        assert!(limiter.try_acquire(1, start));
        assert!(!limiter.try_acquire(1, start));
        assert!(!limiter.try_acquire(1, start + SyncRateLimiter::WINDOW / 2));
    }

    #[test]
    fn a_sync_runs_again_once_the_window_has_elapsed() {
        let limiter = SyncRateLimiter::new();
        let start = Instant::now();
        assert!(limiter.try_acquire(1, start));
        assert!(limiter.try_acquire(1, start + SyncRateLimiter::WINDOW));
        // ...and the fresh run opens a new window rather than leaving the
        // guild permanently unlocked.
        assert!(!limiter.try_acquire(1, start + SyncRateLimiter::WINDOW));
    }

    /// The window is per guild: one busy server must not lock out another.
    #[test]
    fn guilds_are_limited_independently() {
        let limiter = SyncRateLimiter::new();
        let start = Instant::now();
        assert!(limiter.try_acquire(1, start));
        assert!(limiter.try_acquire(2, start));
        assert!(!limiter.try_acquire(1, start));
    }

    /// An import kicks off its own reconcile, so the button that would run a
    /// second one has to see the window as already open.
    #[test]
    fn a_recorded_run_closes_the_window_the_same_way() {
        let limiter = SyncRateLimiter::new();
        let start = Instant::now();
        limiter.record(7, start);
        assert!(!limiter.try_acquire(7, start));
        assert!(limiter.try_acquire(7, start + SyncRateLimiter::WINDOW));
    }

    /// Entries older than the window are dropped, so the map tracks live
    /// guilds rather than every guild that ever synced.
    #[test]
    fn expired_entries_are_pruned() {
        let limiter = SyncRateLimiter::new();
        let start = Instant::now();
        for guild_id in 0..50 {
            limiter.record(guild_id, start);
        }
        assert!(limiter.try_acquire(99, start + SyncRateLimiter::WINDOW));
        assert_eq!(
            limiter.last_run.lock().unwrap().len(),
            1,
            "only the guild that just ran should still be tracked"
        );
    }
}
