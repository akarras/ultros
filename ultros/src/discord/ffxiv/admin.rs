use std::{panic::AssertUnwindSafe, sync::Arc, time::Duration};

use futures::FutureExt;
use poise::serenity_prelude::{ChannelId, CreateMessage, Http};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::{Context, Error};
use crate::item_update_service::{SweepLockGuard, SweepRun, UpdateService};

/// Minimum gap between progress messages posted to the channel during a
/// sweep. A full sweep visits dozens of worlds; without a throttle the
/// per-world progress callback would post a message every few seconds and
/// spam the channel into uselessness.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// Poll while another replica holds the lease, or the database is unavailable.
const RESUME_RETRY: Duration = Duration::from_secs(30);
/// Rest between full coverage passes. Recent-item catch-up continues separately.
const RECONCILIATION_INTERVAL: chrono::Duration = chrono::Duration::hours(6);
/// Retry an interrupted/failed pass without spinning against an unhealthy upstream.
const RECONCILIATION_RETRY: Duration = Duration::from_secs(5 * 60);

/// A resumed pre-boot pass may have scanned early worlds before the outage.
/// Follow it with a new pass; a pass begun after this boot also satisfies other
/// replicas that were already alive when it began.
fn reconciliation_due(
    last: Option<&ultros_db::entity::market_sweep::Model>,
    boot: chrono::DateTime<chrono::FixedOffset>,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> bool {
    last.is_none_or(|last| {
        last.started_at < boot
            || last
                .finished_at
                .is_none_or(|finished| now - finished >= RECONCILIATION_INTERVAL)
    })
}
enum ResumeAttempt<T> {
    Finished,
    Busy,
    Acquired(T),
}

async fn await_resume_claim<T, F, Fut>(
    token: &CancellationToken,
    retry: Duration,
    mut attempt: F,
) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ResumeAttempt<T>>,
{
    loop {
        let result = tokio::select! {
            biased;
            _ = token.cancelled() => return None,
            result = attempt() => result,
        };
        match result {
            ResumeAttempt::Finished => return None,
            ResumeAttempt::Acquired(value) => return Some(value),
            ResumeAttempt::Busy => {}
        }
        tokio::select! {
            _ = token.cancelled() => return None,
            _ = tokio::time::sleep(retry) => {}
        }
    }
}

// A full sweep walks every marketable item across every world and can run
// for hours, while a Discord slash-command interaction token is only valid
// for 15 minutes. Holding the interaction for the whole sweep — the
// previous implementation — meant neither a progress update nor the final
// result could ever be delivered through it; the command would just go
// silent from the user's perspective, which was the entire complaint this
// rewrite exists to fix. Posting through the channel via the bot's HTTP
// client instead (rather than `ctx.reply`/`ctx.say`, which are interaction
// follow-ups under the hood) has no such expiry, so it works no matter how
// long the sweep runs. Do not "simplify" this back to replying on the
// interaction — that is the bug this rewrite fixes.
/// Starts a full market sweep in the background; progress and results are
/// posted to this channel.
///
/// A sweep that a restart interrupted is resumed rather than restarted —
/// [`UpdateService::begin_or_resume_sweep`] — so re-running this after a
/// deploy costs nothing. (It normally does not need re-running at all: the
/// next process picks the sweep up on its own, see
/// [`spawn_market_reconciliation`].)
#[poise::command(slash_command, prefix_command, owners_only)]
pub(crate) async fn rescan_market(ctx: Context<'_>) -> Result<(), Error> {
    let service = ctx.data().update_service.clone();
    // Claim the global sweep lock *before* replying, so the "a sweep is
    // already running" case can reply instead of announcing a sweep that
    // never actually starts.
    let Some(guard) = service.try_begin_full_sweep().await else {
        ctx.reply("A full market sweep is already running.").await?;
        return Ok(());
    };
    let channel_id = ctx.channel_id();
    // Records the run — and adopts an unfinished one — before announcing
    // anything, so the message can say which of the two happened.
    let run = match service
        .begin_or_resume_sweep(Some(channel_id.get() as i64))
        .await
    {
        Ok(run) => run,
        Err(e) => {
            tracing::error!(error = ?e, "could not record the market sweep");
            ctx.reply("Couldn't record the sweep in the database — check the server logs.")
                .await?;
            return Ok(());
        }
    };
    let worlds_total = service.world_cache.get_all_worlds().count();
    let items_total = UpdateService::all_marketable_items().len();
    ctx.reply(if run.resumed {
        format!(
            "Resuming the market sweep already in progress: {}/{worlds_total} worlds done. \
             Progress lands here every ~15 minutes.",
            run.worlds_done()
        )
    } else {
        format!(
            "Starting full market sweep: {items_total} items across {worlds_total} worlds. \
             This takes hours; progress lands here every ~15 minutes."
        )
    })
    .await?;

    // Everything the background task needs, owned and 'static — the poise
    // `Context` itself borrows from the invoking interaction and cannot
    // outlive it, so nothing from `ctx` can be moved into `tokio::spawn`
    // directly.
    spawn_sweep(
        service,
        guard,
        run,
        Some((ctx.serenity_context().http.clone(), channel_id)),
    );
    Ok(())
}

/// Start full reconciliation immediately, resume durable progress after failures,
/// and repeat coverage independently of the bounded recent-update endpoint.
/// Discord availability never delays the work. The advisory lease serializes
/// automatic passes with manual sweeps and workers on other replicas.
pub(crate) fn spawn_market_reconciliation(service: Arc<UpdateService>, token: CancellationToken) {
    // QA instances can share a database without starting a broad upstream scan.
    // Manual /rescan_market and the existing recent-item loop remain available.
    if crate::env_flag_enabled(
        "ULTROS_DISABLE_AUTOMATIC_RECONCILIATION",
        std::env::var("ULTROS_DISABLE_AUTOMATIC_RECONCILIATION")
            .ok()
            .as_deref(),
    ) {
        tracing::warn!("automatic full market reconciliation disabled");
        return;
    }
    let boot = chrono::Utc::now().fixed_offset();
    tokio::spawn(async move {
        loop {
            let claimed = await_resume_claim(&token, RESUME_RETRY, || async {
                let Some(guard) = service.try_begin_full_sweep().await else {
                    return ResumeAttempt::Busy;
                };
                // Recheck durable state under the lease: another replica may
                // have finished while we waited.
                let run = match service.resume_sweep().await {
                    Ok(Some(run)) => run,
                    Ok(None) => {
                        let last = match service.db.latest_completed_market_sweep().await {
                            Ok(last) => last,
                            Err(error) => {
                                tracing::warn!(?error, "could not check reconciliation coverage");
                                return ResumeAttempt::Busy;
                            }
                        };
                        if !reconciliation_due(
                            last.as_ref(),
                            boot,
                            chrono::Utc::now().fixed_offset(),
                        ) {
                            return ResumeAttempt::Finished;
                        }
                        match service.begin_or_resume_sweep(None).await {
                            Ok(run) => run,
                            Err(error) => {
                                tracing::warn!(?error, "could not record reconciliation pass");
                                return ResumeAttempt::Busy;
                            }
                        }
                    }
                    Err(error) => {
                        tracing::warn!(?error, "could not restore reconciliation progress");
                        return ResumeAttempt::Busy;
                    }
                };
                ResumeAttempt::Acquired((guard, run))
            })
            .await;
            if let Some((guard, run)) = claimed {
                tracing::info!(
                    resumed = run.resumed,
                    worlds_done = run.worlds_done(),
                    "starting automatic full market reconciliation"
                );
                let reporter = run.discord_channel_id().and_then(|channel| {
                    crate::alerts::delivery::get_serenity_ctx()
                        .map(|ctx| (ctx.http.clone(), ChannelId::new(channel as u64)))
                });
                // The sweep observes cancellation at chunk boundaries and
                // persists its cursor; do not abort it halfway through a write.
                let _ = spawn_sweep(service.clone(), guard, run, reporter).await;
            }
            tokio::select! {
                _ = token.cancelled() => return,
                _ = tokio::time::sleep(RECONCILIATION_RETRY) => {}
            }
        }
    });
}

/// Runs `run` to completion on a background task, posting throttled progress
/// and the closing report to `reporter`'s channel (or the log when Discord is
/// unavailable).
///
/// Takes `guard` by value so the lock lives for the whole sweep and only frees
/// (on drop, panic included) once the sweep is truly done.
fn spawn_sweep(
    service: Arc<UpdateService>,
    guard: SweepLockGuard,
    run: SweepRun,
    reporter: Option<(Arc<Http>, ChannelId)>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Binding the guard `_guard` (never bare `_`) matters: a bare `_`
        // drops the value immediately at this statement instead of at the end
        // of the async block, which would release the lock before the sweep
        // even starts and defeat the single-sweep guarantee
        // `try_begin_full_sweep` exists to provide.
        let mut guard = guard;

        // The progress callback handed to `do_full_world_sweep` is
        // synchronous (it's called inline between world sweeps), so it can't
        // `.await` a Discord post itself. It throttles and forwards
        // formatted text through this channel; the task below drains it and
        // does the actual posting.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let poster = tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                match &reporter {
                    Some((http, channel_id)) => {
                        if let Err(e) = channel_id
                            .send_message(http, CreateMessage::new().content(text))
                            .await
                        {
                            tracing::error!(error = ?e, "failed to post market sweep update to Discord");
                        }
                    }
                    None => tracing::info!("{text}"),
                }
            }
        });

        let mut last_post = Instant::now();
        let progress_tx = tx.clone();
        // `do_full_world_sweep` is infallible (it swallows and tallies
        // per-chunk failures internally), but a bug in it — or in anything
        // it calls — could still panic partway through a multi-hour run.
        // Without `catch_unwind` that panic would simply unwind the spawned
        // task and the operator would see nothing at all in the channel:
        // exactly the silent-failure mode this whole task exists to
        // eliminate. Wrapping the sweep future (not the outer spawned task)
        // lets the `Err` arm below still post a failure message before the
        // task ends.
        let sweep = AssertUnwindSafe(service.do_full_world_sweep(run, move |progress| {
            if last_post.elapsed() >= PROGRESS_INTERVAL {
                last_post = Instant::now();
                let _ = progress_tx.send(progress.summary_text());
            }
        }))
        .catch_unwind();
        match guard.run(sweep).await {
            Some(Ok(report)) => {
                if report.is_complete() {
                    tracing::info!("full market sweep finished");
                } else {
                    tracing::warn!(
                        "full market sweep incomplete; saved progress will be retried automatically"
                    );
                }
                let _ = tx.send(report.summary_text());
            }
            Some(Err(_)) => {
                // The sweep row is left unfinished, so the next start resumes
                // it from the last recorded chunk rather than from nothing.
                tracing::error!("full market sweep panicked");
                let _ = tx.send(
                    "Full market sweep crashed — check the server logs. Saved progress will \
                     be retried automatically."
                        .to_string(),
                );
            }
            None => {
                let _ = tx.send(
                    "Full market sweep paused because its database lease was lost. \
                     Its saved progress can be resumed."
                        .to_string(),
                );
            }
        }
        // Dropping the sender lets the poster's `while let Some` loop end
        // once every queued message has been sent, and awaiting it here
        // ensures the final report is actually posted before this task
        // exits (and the guard drops).
        drop(tx);
        let _ = poster.await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completed_pass(start: i64, finish: i64) -> ultros_db::entity::market_sweep::Model {
        ultros_db::entity::market_sweep::Model {
            id: 1,
            started_at: chrono::DateTime::from_timestamp(start, 0)
                .unwrap()
                .fixed_offset(),
            finished_at: Some(
                chrono::DateTime::from_timestamp(finish, 0)
                    .unwrap()
                    .fixed_offset(),
            ),
            discord_channel_id: None,
        }
    }

    #[test]
    fn startup_requires_coverage_even_when_old_event_markers_look_fresh() {
        let boot = completed_pass(100, 100).started_at;
        let now = completed_pass(200, 200).started_at;
        assert!(reconciliation_due(None, boot, now));
        // A resumed sweep finishing after boot does not recheck worlds visited
        // before the downtime. A fresh pass must follow it.
        assert!(reconciliation_due(
            Some(&completed_pass(50, 150)),
            boot,
            now
        ));
        // A manual/other-replica pass started after boot satisfies this process.
        assert!(!reconciliation_due(
            Some(&completed_pass(110, 190)),
            boot,
            now
        ));
    }

    #[test]
    fn full_coverage_repeats_without_a_restart_or_recent_uploads() {
        let last = completed_pass(100, 200);
        let boot = last.started_at;
        let finished = last.finished_at.unwrap();
        assert!(!reconciliation_due(
            Some(&last),
            boot,
            finished + RECONCILIATION_INTERVAL - chrono::Duration::seconds(1)
        ));
        assert!(reconciliation_due(
            Some(&last),
            boot,
            finished + RECONCILIATION_INTERVAL
        ));
        assert!(!reconciliation_due(
            Some(&last),
            boot,
            finished - chrono::Duration::seconds(1)
        ));
    }

    #[tokio::test]
    async fn reconciliation_attempts_before_any_poll_delay() {
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            await_resume_claim(&CancellationToken::new(), Duration::from_secs(3600), || {
                std::future::ready(ResumeAttempt::Acquired(42))
            }),
        )
        .await
        .unwrap();
        assert_eq!(result, Some(42));
    }

    #[tokio::test]
    async fn interrupted_sweep_waits_for_the_old_replica_to_release_its_lease() {
        let mut attempts = 0;
        let claimed = await_resume_claim(&CancellationToken::new(), Duration::ZERO, || {
            attempts += 1;
            std::future::ready(if attempts < 3 {
                ResumeAttempt::Busy
            } else {
                ResumeAttempt::Acquired(42)
            })
        })
        .await;
        assert_eq!(claimed, Some(42));
        assert_eq!(attempts, 3);
    }

    #[tokio::test]
    async fn a_sweep_finished_by_the_other_replica_is_not_started_again() {
        let mut attempts = 0;
        let claimed = await_resume_claim(&CancellationToken::new(), Duration::ZERO, || {
            attempts += 1;
            std::future::ready(if attempts == 1 {
                ResumeAttempt::<()>::Busy
            } else {
                ResumeAttempt::Finished
            })
        })
        .await;
        assert_eq!(claimed, None);
        assert_eq!(attempts, 2);
    }

    #[tokio::test]
    async fn shutdown_interrupts_the_resume_retry_delay() {
        let token = CancellationToken::new();
        let claimed = await_resume_claim(&token, Duration::from_secs(3600), || {
            token.cancel();
            std::future::ready(ResumeAttempt::<()>::Busy)
        })
        .await;
        assert_eq!(claimed, None);
    }
}
