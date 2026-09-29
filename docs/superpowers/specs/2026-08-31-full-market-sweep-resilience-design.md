# Full market sweep resilience — design

**Date:** 2026-08-31
**Status:** Approved
**Scope:** `ultros/src/item_update_service.rs`, `ultros/src/discord/ffxiv/admin.rs`

## Problem

A full market sweep (`UpdateService::do_full_world_sweep`, and the saturation-triggered
per-world sweep inside `check_for_missed_items_on_world`) aborts on the first failed
Universalis chunk fetch: `check_items` does `marketboard_current_data(...).await?` per
100-item chunk, and the `?` chain abandons the rest of the chunk list **and all remaining
worlds**. Over a multi-hour run against Universalis — which sheds requests routinely with
429/5xx — an eventual abort is close to guaranteed. Observed in practice: `/rescan_market`
replied with an error in Discord and never recovered.

Additional defects:

- `claim_full_sweep_slot` stamps the 6-hour cooldown **before** the sweep runs, so a failed
  saturation-triggered sweep blocks retries for 6 hours despite recovering nothing.
- Both full-sweep call sites discard the `CatchupTally` returned by `check_items`, so full
  sweeps never increment `ultros_catchup_items_recovered`.
- `/rescan_market` holds the Discord interaction for the whole sweep. Slash-command
  interaction tokens expire after 15 minutes, so neither the success reply nor an error
  reply is deliverable for a multi-hour sweep.
- Nothing prevents two full sweeps from running concurrently.

## Design

Five changes, mostly independent.

### 1. Chunk-level retry + skip in `check_items`

Wrap the `marketboard_current_data` call in a retry helper:

- On a transient error (`universalis::Error::is_transient` — 429, 5xx, connect/timeout;
  classification from #1231), retry up to 3 times with backoff (~5s / 15s / 45s).
- If retries are exhausted, or the error is non-transient, **skip the chunk and continue**:
  log a warning, increment a new counter
  `ultros_sweep_chunks_failed{world, kind}` (`kind` = `transient` | `error`), and count the
  skip in the return value.

`check_items` stops returning `Err` for fetch failures. Its return type carries the
existing `CatchupTally` plus a `chunks_failed: u64` count (either a new field on
`CatchupTally` or a small wrapper struct — implementer's choice, but `record()` must not
emit a `chunks_failed` outcome into `ultros_catchup_items_recovered`; the new counter is
its own metric).

Skipped items remain recoverable because their `listing_last_updated` markers are never
bumped without a successful write, so the regular 5-minute catch-up loop re-flags them.

This intentionally changes the ordinary catch-up path too: a transient fetch failure there
becomes a skipped chunk instead of a failed world pass. The outer loop's
"transient = skipped cycle" warn arm becomes nearly dead code; leave it in place as a
backstop for genuinely unexpected errors (e.g. DB failures surfaced through the same path).

### 2. `do_full_world_sweep` runs to completion and reports

- Iterate **all** worlds; never abort the sweep because one world failed.
- Collect a per-world summary: tally (changed/noop/failed), chunks failed, duration.
- Call `tally.record(&world.name)` per world so full sweeps show up in
  `ultros_catchup_items_recovered`. The saturation-triggered call site records its tally
  too.
- Return a `SweepReport { worlds: Vec<WorldSweepSummary>, started/finished, ... }` instead
  of `Result<(), anyhow::Error>`.
- Accept a progress callback (`impl Fn(SweepProgress)` or similar) invoked after each
  world completes, so callers decide how to surface progress. No Discord types in the
  update service.

### 3. Cooldown stamped on completion

Split `claim_full_sweep_slot` into:

- **claim** — reserves the world's slot (refuses if within cooldown *or* currently
  claimed), preventing concurrent starts;
- **confirm** — stamps the 6-hour timestamp, called only when the sweep ran to completion;
- a failed/aborted sweep **releases** the claim without stamping, so the next saturated
  cycle can retry immediately.

With skip-and-continue, completion is the normal case; this is the backstop for hard
failures (panics excluded — no drop-guard heroics needed, a leaked claim just falls back
to today's behavior for that world).

### 4. Single-full-sweep guard

A flag on `UpdateService` (e.g. `AtomicBool sweep_running`) ensures only one **full** sweep
(manual or saturation-triggered) runs at a time. `/rescan_market` during a running sweep
replies "sweep already in progress". A saturation trigger during a running sweep logs and
skips (its cooldown is not stamped).

### 5. `/rescan_market` goes fire-and-forget

- The command captures `ctx.channel_id()` and the serenity `Http` handle, spawns the sweep
  on a tokio task, and replies immediately: item count, world count, rough ETA.
- The background task posts **plain channel messages** (not interaction follow-ups, so no
  15-minute token expiry):
  - a progress message roughly every 15 minutes — worlds done/total, items recovered,
    chunks skipped;
  - a final summary — totals, duration, and the list of worlds that had skipped chunks;
  - on an unexpected error, a failure message instead of silence.

## Out of scope (YAGNI)

- Persisting sweep state across restarts / resumable sweeps — the 5-minute catch-up loop
  is the durable safety net; a restarted sweep starts over on request.
- Any web UI for sweeps.
- Changing sweep pacing (chunk size 100, 1s inter-chunk sleep stays as-is).

## Testing

- Retry helper: injected failing/succeeding closures, `tokio::time::pause` so backoff
  tests run instantly; verify transient-retries-then-skip, non-transient-skips-immediately,
  and success-after-retry.
- Cooldown claim/confirm/release semantics (no real sleeps; drive with `Instant`s or
  paused tokio time).
- `SweepReport` aggregation and the Discord summary formatting (pure functions).
- Existing tests already cover transient-error classification through `anyhow` and the
  marker-integrity invariants; keep them green.

## Success criteria

- A full sweep survives arbitrary interleaved Universalis 429/5xx/timeouts and reports how
  much it could not cover, instead of aborting.
- `/rescan_market` always yields a Discord outcome message (progress + final summary or an
  explicit failure), regardless of sweep duration.
- A failed saturation-triggered sweep no longer costs the world its 6-hour slot.
- Grafana shows full-sweep recovery volume (`ultros_catchup_items_recovered`) and skipped
  coverage (`ultros_sweep_chunks_failed`).
