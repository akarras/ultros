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

/// How long a resumed sweep waits for the Discord bot to finish connecting
/// before it starts anyway. Only the reporting depends on the gateway — the
/// sweep itself runs either way, logging instead of posting.
const RESUME_GATEWAY_WAIT: Duration = Duration::from_secs(60);

/// Interval between checks for the gateway during [`RESUME_GATEWAY_WAIT`].
const RESUME_GATEWAY_POLL: Duration = Duration::from_secs(2);

const RESUME_RETRY: Duration = Duration::from_secs(30);

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
/// [`spawn_interrupted_sweep_resume`].)
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

/// Picks up a full sweep that a restart interrupted, if there is one.
///
/// A sweep takes hours and deploys land more often than that, so without this
/// a sweep essentially never finished: every restart threw the run away and an
/// operator had to notice and re-issue `/rescan_market`. The per-chunk cursor
/// in `market_sweep_world` is what makes resuming cheap; this is what makes it
/// automatic.
///
/// Reporting waits [`RESUME_GATEWAY_WAIT`] for the bot to connect so progress
/// lands in the channel the sweep was originally started from. If the gateway
/// never comes up — a deployment without Discord configured — the sweep still
/// runs and reports to the log.
pub(crate) fn spawn_interrupted_sweep_resume(
    service: Arc<UpdateService>,
    token: CancellationToken,
) {
    tokio::spawn(async move {
        // Cheap early-out on the overwhelmingly common path: no sweep was in
        // flight, so nothing here needs a lease or the gateway.
        match service.db.active_market_sweep().await {
            Ok(None) => return,
            Ok(Some(_)) => {}
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "could not check for an interrupted market sweep; retrying"
                );
            }
        }
        let reporter = wait_for_gateway(&token).await;
        if token.is_cancelled() {
            // Shut down before the bot came up. The sweep is on disk; the next
            // start picks it up.
            return;
        }
        // Claimed after the wait, so a replica that loses the race does not
        // hold the lease for a minute first.
        let Some((guard, run)) = await_resume_claim(&token, RESUME_RETRY, || async {
            // A rolling deploy can start this replica before the old worker
            // releases its lease. Stay eligible until the sweep is finished,
            // rather than abandoning its durable cursor after one busy claim.
            match service.db.active_market_sweep().await {
                Ok(None) => return ResumeAttempt::Finished,
                Ok(Some(_)) => {}
                Err(error) => {
                    tracing::warn!(?error, "could not check interrupted sweep; retrying");
                    return ResumeAttempt::Busy;
                }
            }
            let Some(guard) = service.try_begin_full_sweep().await else {
                return ResumeAttempt::Busy;
            };
            match service.resume_sweep().await {
                Ok(None) => ResumeAttempt::Finished,
                Ok(Some(run)) => ResumeAttempt::Acquired((guard, run)),
                Err(error) => {
                    tracing::warn!(?error, "could not load interrupted sweep; retrying");
                    ResumeAttempt::Busy
                }
            }
        })
        .await
        else {
            return;
        };
        let worlds_total = service.world_cache.get_all_worlds().count();
        let announcement = format!(
            "Resuming the market sweep interrupted by a restart: {}/{worlds_total} worlds already done.",
            run.worlds_done()
        );
        let reporter = match (reporter, run.discord_channel_id()) {
            (Some(http), Some(channel)) => Some((http, ChannelId::new(channel as u64))),
            _ => None,
        };
        match &reporter {
            Some((http, channel_id)) => {
                if let Err(error) = channel_id
                    .send_message(http, CreateMessage::new().content(&announcement))
                    .await
                {
                    tracing::error!(?error, "failed to announce the resumed market sweep");
                }
            }
            None => tracing::info!("{announcement}"),
        }
        spawn_sweep(service, guard, run, reporter);
    });
}

/// The bot's HTTP client once the gateway is up, or `None` if it has not
/// connected within [`RESUME_GATEWAY_WAIT`].
async fn wait_for_gateway(token: &CancellationToken) -> Option<Arc<Http>> {
    let deadline = Instant::now() + RESUME_GATEWAY_WAIT;
    loop {
        if let Some(ctx) = crate::alerts::delivery::get_serenity_ctx() {
            return Some(ctx.http.clone());
        }
        if Instant::now() >= deadline {
            tracing::warn!(
                "Discord did not connect in time; the resumed market sweep will report to the log"
            );
            return None;
        }
        tokio::select! {
            _ = token.cancelled() => return None,
            _ = tokio::time::sleep(RESUME_GATEWAY_POLL) => {}
        }
    }
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
) {
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
                        "full market sweep paused for shutdown; the next start resumes it"
                    );
                }
                let _ = tx.send(report.summary_text());
            }
            Some(Err(_)) => {
                // The sweep row is left unfinished, so the next start resumes
                // it from the last recorded chunk rather than from nothing.
                tracing::error!("full market sweep panicked");
                let _ = tx.send(
                    "Full market sweep crashed — check the server logs. It resumes on the next \
                     restart."
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
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
