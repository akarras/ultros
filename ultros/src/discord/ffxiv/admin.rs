use std::{panic::AssertUnwindSafe, time::Duration};

use futures::FutureExt;
use poise::serenity_prelude::CreateMessage;
use tokio::time::Instant;

use super::{Context, Error};

/// Minimum gap between progress messages posted to the channel during a
/// sweep. A full sweep visits dozens of worlds; without a throttle the
/// per-world progress callback would post a message every few seconds and
/// spam the channel into uselessness.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(15 * 60);

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
#[poise::command(slash_command, prefix_command, owners_only)]
pub(crate) async fn rescan_market(ctx: Context<'_>) -> Result<(), Error> {
    let service = ctx.data().update_service.clone();
    // Claim the global sweep lock *before* replying, so the "a sweep is
    // already running" case can reply instead of announcing a sweep that
    // never actually starts.
    let Some(guard) = service.try_begin_full_sweep() else {
        ctx.reply("A full market sweep is already running.").await?;
        return Ok(());
    };
    let worlds_total = service.world_cache.get_all_worlds().count();
    let items_total = crate::item_update_service::UpdateService::all_marketable_items().len();
    ctx.reply(format!(
        "Starting full market sweep: {items_total} items across {worlds_total} worlds. \
         This takes hours; progress lands here every ~15 minutes."
    ))
    .await?;

    // Everything the background task needs, owned and 'static — the poise
    // `Context` itself borrows from the invoking interaction and cannot
    // outlive it, so nothing from `ctx` can be moved into `tokio::spawn`
    // directly.
    let channel_id = ctx.channel_id();
    let http = ctx.serenity_context().http.clone();
    tokio::spawn(async move {
        // Moves the lock guard into the background task so it lives for the
        // whole sweep and only frees (on drop, panic included) once the
        // sweep is truly done. Binding it `_guard` (never bare `_`) matters:
        // a bare `_` drops the value immediately at this statement instead
        // of at the end of the async block, which would release the lock
        // before the sweep even starts and defeat the single-sweep guarantee
        // `try_begin_full_sweep` exists to provide.
        let _guard = guard;

        // The progress callback handed to `do_full_world_sweep` is
        // synchronous (it's called inline between world sweeps), so it can't
        // `.await` a Discord post itself. It throttles and forwards
        // formatted text through this channel; the task below drains it and
        // does the actual posting.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let poster_http = http.clone();
        let poster = tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                if let Err(e) = channel_id
                    .send_message(&poster_http, CreateMessage::new().content(text))
                    .await
                {
                    tracing::error!(error = ?e, "failed to post market sweep update to Discord");
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
        let sweep = AssertUnwindSafe(service.do_full_world_sweep(move |progress| {
            if last_post.elapsed() >= PROGRESS_INTERVAL {
                last_post = Instant::now();
                let _ = progress_tx.send(progress.summary_text());
            }
        }))
        .catch_unwind()
        .await;
        match sweep {
            Ok(report) => {
                let _ = tx.send(report.summary_text());
            }
            Err(_) => {
                tracing::error!("full market sweep panicked");
                let _ = tx.send("Full market sweep crashed — check the server logs.".to_string());
            }
        }
        // Dropping the sender lets the poster's `while let Some` loop end
        // once every queued message has been sent, and awaiting it here
        // ensures the final report is actually posted before this task
        // exits (and the guard drops).
        drop(tx);
        let _ = poster.await;
    });
    Ok(())
}
