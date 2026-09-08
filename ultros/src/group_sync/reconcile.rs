//! Periodic reconciliation: the source of truth for Discord-backed group
//! membership.
//!
//! Gateway events (see [`super::events`]) keep membership fresh between runs,
//! but they can be missed — a dropped shard, a restart, an event Discord never
//! sent. This walks the guild from scratch and makes the database match, so
//! any drift is bounded by the cycle interval rather than lasting forever.
//!
//! Both paths apply the same idempotent plans through the same DB primitive,
//! so a reconcile racing an event converges to the same state whichever wins.

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

/// A guild larger than this is not something this bot is in, so hitting the
/// cap means the cursor is not advancing the way we think it is. Stop and say
/// so rather than paginating forever against Discord's rate limiter.
const MEMBER_HARD_CAP: usize = 250_000;

/// How often every guild with a synced role is walked.
const RECONCILE_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

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

/// Every member of the guild, paginated at Discord's maximum page size.
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
        // Discord pages members by ascending user id and returns a short page
        // at the end.
        if page_len < MEMBER_PAGE_SIZE as usize {
            break;
        }
        if Some(highest) <= after {
            anyhow::bail!("Discord member pagination did not advance");
        }
        if members.len() >= MEMBER_HARD_CAP {
            warn!(
                guild_id = guild_id.get(),
                fetched = members.len(),
                "stopping member pagination at the hard cap"
            );
            break;
        }
        after = Some(highest);
    }
    Ok(members)
}

/// Reconcile one guild against Discord. `Ok(None)` means the guild has nothing
/// synced and was skipped without touching Discord at all.
pub(crate) async fn reconcile_guild(
    db: &UltrosDb,
    ctx: &serenity::Context,
    guild_id: i64,
) -> Result<Option<GuildSyncReport>> {
    let synced = db.synced_roles_for_guild(guild_id).await?;
    if synced.is_empty() {
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

    // A role Discord no longer has stops syncing but keeps its members, so the
    // list share pointing at it does not silently lose everyone.
    let mut roles = Vec::with_capacity(synced.len());
    for role in synced {
        // `@everyone` is returned by the roles endpoint, but orphaning it by
        // accident would silently switch off whole-server sync, so it is never
        // a candidate for orphaning on the strength of a role listing.
        if diff::is_everyone_role(guild_id, role.discord_role_id)
            || live.contains(&role.discord_role_id)
        {
            roles.push(role);
        } else {
            db.mark_role_orphaned(guild_id, role.discord_role_id)
                .await?;
            report.orphaned += 1;
        }
    }
    report.roles = roles.len();
    if roles.is_empty() {
        return Ok(Some(report));
    }

    let members = fetch_guild_members(ctx, discord_guild).await?;
    report.members_seen = members.len();

    // Every user we already had in one of these roles: their `discord_user`
    // row certainly exists, which is what makes the bulk name refresh below
    // safe to run over them.
    let mut known: HashSet<i64> = HashSet::new();
    let mut by_group: BTreeMap<i32, Vec<RoleSyncPlan>> = BTreeMap::new();
    for role in &roles {
        let desired = diff::desired_members(guild_id, role.discord_role_id, &members);
        let current = db.role_member_ids(role.role_id).await?;
        known.extend(current.iter().copied());
        by_group.entry(role.group_id).or_default().push(diff::plan(
            role.role_id,
            &desired,
            &current,
        ));
    }

    for (group_id, plans) in by_group {
        report.absorb(db.apply_role_sync(group_id, plans).await?);
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

/// Reconcile one guild, logging the outcome. Used by every caller that is not
/// interested in the report itself.
async fn reconcile_and_log(db: &UltrosDb, ctx: &serenity::Context, guild_id: i64) {
    match reconcile_guild(db, ctx, guild_id).await {
        Ok(Some(report)) => info!(
            guild_id,
            roles = report.roles,
            orphaned = report.orphaned,
            members_seen = report.members_seen,
            added = report.added,
            removed = report.removed,
            left_group = report.left_group,
            handed_over = report.handed_over,
            names_refreshed = report.names_refreshed,
            "reconciled Discord group membership"
        ),
        Ok(None) => debug!(guild_id, "no synced roles, nothing to reconcile"),
        Err(error) => report_failure(guild_id, &error),
    }
}

/// Reconcile one guild in the background. Callers on a request path use this
/// so a whole-server member walk never happens inline in an HTTP handler.
pub(crate) fn spawn_reconcile(db: UltrosDb, guild_id: i64) {
    tokio::spawn(async move {
        let Some(ctx) = crate::alerts::delivery::get_serenity_ctx() else {
            warn!(
                guild_id,
                "cannot reconcile group membership, the Discord bot is not connected"
            );
            return;
        };
        reconcile_and_log(&db, &ctx, guild_id).await;
    });
}

/// Walk every guild with at least one synced role, sequentially.
async fn reconcile_all(db: &UltrosDb, token: &CancellationToken) {
    let Some(ctx) = crate::alerts::delivery::get_serenity_ctx() else {
        warn!("skipping group membership reconciliation, the Discord bot is not connected");
        return;
    };
    let guilds = match db.guilds_with_synced_roles().await {
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
        reconcile_and_log(db, &ctx, guild_id).await;
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
