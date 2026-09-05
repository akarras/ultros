# Full Market Sweep Resilience Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the full market sweep survive Universalis failures (retry transient errors, skip what still fails, never abort), stamp the 6h cooldown only on completion, record tallies, and turn `/rescan_market` into a fire-and-forget Discord command with progress and summary messages.

**Architecture:** All sweep logic stays in `ultros/src/item_update_service.rs`. `check_items` becomes infallible (retry + skip per chunk, `chunks_failed` counted on the tally). `do_full_world_sweep` iterates all worlds, returns a `SweepReport`, and calls a progress callback. Cooldown becomes a claim/confirm/release state machine over pure functions. A `SweepLock` (RAII guard over an `AtomicBool`) serializes full sweeps. The Discord command spawns the sweep on a tokio task and posts plain channel messages (immune to the 15-minute interaction-token expiry).

**Tech Stack:** Rust, tokio (paused-time tests), poise/serenity (Discord), `metrics` crate, existing `universalis::Error::is_transient` classification.

**Spec:** `docs/superpowers/specs/2026-08-31-full-market-sweep-resilience-design.md`

## Global Constraints

- Run `./check_ci.sh` (fmt + clippy `-D warnings`) before every push; at minimum `cargo fmt --all -- --check` before every commit. On Windows, prepend Strawberry Perl to PATH first (see CLAUDE.md).
- Check exit codes directly, never through a pipe: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"`.
- **Windows link caveat:** `cargo test -p ultros` may fail to link with a `/SYM64/` archive error (known machine issue, not your code). If that happens, verify with `cargo check --all-targets -p ultros` + clippy and note it in the PR; the test code still ships.
- Discord bot strings are NOT subject to the leptos-i18n rule (that applies only to `ultros-frontend/ultros-app/`). Plain English strings are correct here.
- No new dependencies. `futures` (for `FutureExt::catch_unwind`), `metrics`, and `tokio` are already in `ultros`'s Cargo.toml.
- Metric names are load-bearing (Grafana): the new counter is exactly `ultros_sweep_chunks_failed` with labels `world` and `kind` (`"transient"` | `"error"`). Do not add a `chunks_failed` outcome to `ultros_catchup_items_recovered`.
- All work happens in this worktree on branch `claude/market-sweep-error-handling-e16bb3`. Commit after each task.

---

### Task 1: Transient-retry helper

**Files:**
- Modify: `ultros/src/item_update_service.rs` (new constants + free async fn near the top, tests in the existing `mod tests`)

**Interfaces:**
- Consumes: `universalis::Error::is_transient` (exists).
- Produces: `async fn retry_transient<T, F, Fut>(op: F) -> Result<T, universalis::Error> where F: FnMut() -> Fut, Fut: Future<Output = Result<T, universalis::Error>>` and `const CHUNK_RETRY_BACKOFF: [Duration; 3]`. Task 2 wraps the chunk fetch with this.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `ultros/src/item_update_service.rs` (the `universalis_status` helper at the top of the module builds `anyhow`-wrapped errors; these tests need the bare `universalis::Error`, so add a bare variant):

```rust
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
        let out = if attempts < 3 { Err(bare_status(504)) } else { Ok(42) };
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
```

Note the closure-borrow shape in the first test: compute `out` *outside* the `async move` block so the closure's `&mut attempts` borrow ends before the future is returned.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros item_update_service -- --list` first to confirm the harness links on this machine, then `cargo test -p ultros retry_transient`
Expected: compile error — `retry_transient` and `CHUNK_RETRY_BACKOFF` not found. (If the *link* fails with `/SYM64/`, see Global Constraints: fall back to `cargo check --all-targets -p ultros` showing the same missing-item errors.)

- [ ] **Step 3: Write the implementation**

Near the other constants at the top of `item_update_service.rs`:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ultros retry_transient`
Expected: 3 passed. Also run `cargo fmt --all -- --check`.

- [ ] **Step 5: Commit**

```bash
git add ultros/src/item_update_service.rs
git commit -m "feat(sweep): transient-retry helper for universalis chunk fetches"
```

---

### Task 2: `check_items` becomes infallible — retry, skip, count

**Files:**
- Modify: `ultros/src/item_update_service.rs` — `CatchupTally`, `check_items`, and its three call sites (`check_for_missed_items_on_world` ×2, `do_full_world_sweep`)

**Interfaces:**
- Consumes: `retry_transient` + `CHUNK_RETRY_BACKOFF` (Task 1).
- Produces: `CatchupTally` gains `chunks_failed: u64`; `async fn check_items(&self, world: &world::Model, item_ids: &[i32]) -> CatchupTally` (no more `Result`). Tasks 4–5 rely on this signature and on the field.

- [ ] **Step 1: Write the failing test**

`record()` must keep emitting only the three item outcomes — `chunks_failed` gets its own metric at the skip site. Encode the struct shape:

```rust
/// `chunks_failed` counts whole skipped fetch chunks (up to 100 items each),
/// not items — it rides the tally for aggregation but is emitted through
/// `ultros_sweep_chunks_failed`, never `ultros_catchup_items_recovered`.
#[test]
fn tally_default_has_no_failed_chunks() {
    let tally = CatchupTally::default();
    assert_eq!(tally.chunks_failed, 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p ultros tally_default_has_no_failed_chunks`
Expected: compile error — no field `chunks_failed` on `CatchupTally`.

- [ ] **Step 3: Implement**

3a. Add the field to `CatchupTally` (leave `add` and `record` untouched — `record`'s three-entry array stays exactly as it is):

```rust
#[derive(Default, Debug, PartialEq, Eq)]
struct CatchupTally {
    changed: u64,
    noop: u64,
    failed: u64,
    /// Fetch chunks skipped after retries — see `ultros_sweep_chunks_failed`.
    chunks_failed: u64,
}
```

Fix the existing `tally_counts_each_outcome_separately` test's struct literal by adding `chunks_failed: 0`.

3b. Rework `check_items`. Signature drops the `Result`; destructured-parameter form changes to a plain `world` binding so the world stays nameable:

```rust
async fn check_items(&self, world: &world::Model, item_ids: &[i32]) -> CatchupTally {
    let world_id = WorldId(world.id);
    let world_name = &world.name;
    let mut tally = CatchupTally::default();
    let total_chunks = item_ids.chunks(100).len();
    for (chunk_index, item_ids) in item_ids.chunks(100).enumerate() {
        let market_data = match retry_transient(|| {
            self.universalis.marketboard_current_data(world_name, item_ids)
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
        // ... existing buffer_unordered(50) body unchanged ...
        for outcome in outcomes {
            tally.add(outcome);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    tally
}
```

The `stream::iter(...)` / `update_listings` / `update_sales` body is unchanged except that `world_id` now comes from the binding above. `Ok(tally)` at the end becomes `tally`.

3c. Update the call sites in `check_for_missed_items_on_world`:

- Line ~248: `let tally = self.check_items(world, &item_ids).await;` (drop the `?`), keep `tally.record(&world.name);`.
- Saturation branch (~line 258): `self.check_items(world, &Self::all_marketable_items()).await?;` → bind and record the tally (previously discarded — spec §2):
  ```rust
  let tally = self.check_items(world, &Self::all_marketable_items()).await;
  tally.record(&world.name);
  ```
- Price-drift branch (~line 294): drop the `?` on its `check_items` call.
- `do_full_world_sweep` (~line 193): `self.check_items(world, &all_marketable_items).await?;` → `let _ = self.check_items(world, &all_marketable_items).await;` (temporary — Task 4 rewrites this function; the `let _` just keeps this task compiling).

Do NOT remove the transient/error classification arm in `start_service` — `check_for_missed_items_on_world` still returns `Err` from `get_missing_updates` and DB reads.

- [ ] **Step 4: Run the full module's tests**

Run: `cargo test -p ultros item_update_service` and `cargo clippy --all-targets -p ultros -- -D warnings`
Expected: all pass, no warnings (watch for a dead `Result` import or unused `anyhow` in `check_items`' signature chain).

- [ ] **Step 5: Commit**

```bash
git add ultros/src/item_update_service.rs
git commit -m "feat(sweep): check_items retries transient fetches and skips instead of aborting"
```

---

### Task 3: Cooldown claim/confirm/release state machine

**Files:**
- Modify: `ultros/src/item_update_service.rs` — replace `claim_full_sweep_slot` and the `full_sweep_cooldowns` map's value type; tests in `mod tests`
- Modify: `ultros/src/main.rs:490` — no change needed (`Default::default()` still works), verify only

**Interfaces:**
- Consumes: `FULL_SWEEP_COOLDOWN` (exists).
- Produces: `enum SweepSlot { Running, CompletedAt(Instant) }`; pure fns `claim_slot(&mut HashMap<i32, SweepSlot>, world_id: i32, now: Instant) -> bool`, `confirm_slot(&mut HashMap<i32, SweepSlot>, world_id: i32, now: Instant)`, `release_slot(&mut HashMap<i32, SweepSlot>, world_id: i32)`; methods `UpdateService::{claim_full_sweep_slot(&self, i32) -> bool, confirm_full_sweep(&self, i32), release_full_sweep_slot(&self, i32)}`. Field becomes `full_sweep_cooldowns: Mutex<HashMap<i32, SweepSlot>>`. Tasks 4–5 call the methods.

- [ ] **Step 1: Write the failing tests**

The fns take `now` explicitly, so no paused-time machinery — manufacture instants with `Instant::now() + offset` (this `Instant` is `tokio::time::Instant`, already imported):

```rust
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
    assert!(!claim_slot(&mut slots, WORLD_ID, t0 + FULL_SWEEP_COOLDOWN - Duration::from_secs(1)));
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
    assert!(!claim_slot(&mut slots, WORLD_ID, t0 + Duration::from_secs(1)));
}

#[test]
fn slots_are_per_world() {
    let mut slots = HashMap::new();
    let t0 = Instant::now();
    assert!(claim_slot(&mut slots, 1, t0));
    assert!(claim_slot(&mut slots, 2, t0));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros slot`
Expected: compile error — `claim_slot` etc. not found.

- [ ] **Step 3: Implement**

```rust
/// State of a world's full-sweep slot. `Running` reserves the slot while a
/// sweep is in flight; only a *completed* sweep stamps `CompletedAt`, so a
/// sweep that dies never costs the world its [`FULL_SWEEP_COOLDOWN`] (a
/// claim that leaks on panic degrades to the old stamp-upfront behavior).
#[derive(Clone, Copy, Debug)]
enum SweepSlot {
    Running,
    CompletedAt(Instant),
}

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

fn confirm_slot(slots: &mut HashMap<i32, SweepSlot>, world_id: i32, now: Instant) {
    slots.insert(world_id, SweepSlot::CompletedAt(now));
}

/// Frees a claimed-but-unfinished slot. A confirmed cooldown is left alone.
fn release_slot(slots: &mut HashMap<i32, SweepSlot>, world_id: i32) {
    if let Some(SweepSlot::Running) = slots.get(&world_id) {
        slots.remove(&world_id);
    }
}
```

Change the field to `pub(crate) full_sweep_cooldowns: Mutex<HashMap<i32, SweepSlot>>` (still `Default::default()` in `main.rs` — note `SweepSlot` must then be `pub(crate)` too, or keep the field private-in-module by leaving visibility as-is; it's constructed in `main.rs`, so `pub(crate)` on both) and replace the old method with three thin wrappers:

```rust
fn claim_full_sweep_slot(&self, world_id: i32) -> bool {
    claim_slot(
        &mut self.full_sweep_cooldowns.lock().expect("full_sweep_cooldowns poisoned"),
        world_id,
        Instant::now(),
    )
}

fn confirm_full_sweep(&self, world_id: i32) {
    confirm_slot(
        &mut self.full_sweep_cooldowns.lock().expect("full_sweep_cooldowns poisoned"),
        world_id,
        Instant::now(),
    )
}

fn release_full_sweep_slot(&self, world_id: i32) {
    release_slot(
        &mut self.full_sweep_cooldowns.lock().expect("full_sweep_cooldowns poisoned"),
        world_id,
    )
}
```

The saturation call site still calls `claim_full_sweep_slot` and compiles; `confirm`/`release` are wired in Task 5 (add `#[allow(dead_code)]` NOTHING — if clippy flags them as unused, wire Task 5's two one-line calls in this task instead of allowing).

- [ ] **Step 4: Run tests**

Run: `cargo test -p ultros item_update_service && cargo clippy --all-targets -p ultros -- -D warnings`
Expected: pass. If `confirm_full_sweep`/`release_full_sweep_slot` trip `dead_code`, pull Task 5's call-site wiring forward into this commit (see Task 5 Step 2 for the exact block).

- [ ] **Step 5: Commit**

```bash
git add ultros/src/item_update_service.rs ultros/src/main.rs
git commit -m "feat(sweep): cooldown stamps on completion via claim/confirm/release slots"
```

---

### Task 4: `SweepLock`, `SweepReport`, and the `do_full_world_sweep` rework

**Files:**
- Modify: `ultros/src/item_update_service.rs` — new types + rework `do_full_world_sweep`
- Modify: `ultros/src/main.rs:484-492` — add the new field to the `UpdateService` literal

**Interfaces:**
- Consumes: infallible `check_items` (Task 2), `confirm_full_sweep` (Task 3).
- Produces (all `pub(crate)`, used by Tasks 5–6):
  - `struct SweepLock(AtomicBool)` with `fn try_claim(self: &Arc<Self>) -> Option<SweepLockGuard>`; `struct SweepLockGuard` (releases on `Drop`).
  - `UpdateService` field `sweep_lock: Arc<SweepLock>` and method `fn try_begin_full_sweep(&self) -> Option<SweepLockGuard>`.
  - `struct WorldSweepSummary { world_name: String, tally: CatchupTally, duration: std::time::Duration }` (module-private fields are fine — everything lives in this file).
  - `struct SweepProgress { worlds_done: usize, worlds_total: usize, items_changed: u64, chunks_failed: u64 }` with `pub(crate) fn summary_text(&self) -> String`.
  - `struct SweepReport { worlds: Vec<WorldSweepSummary>, duration: std::time::Duration }` with `pub(crate) fn summary_text(&self) -> String`.
  - `pub(crate) async fn do_full_world_sweep(&self, progress: impl FnMut(SweepProgress)) -> SweepReport`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn sweep_lock_is_exclusive_and_releases_on_drop() {
    let lock = Arc::new(SweepLock::default());
    let guard = lock.try_claim().expect("free lock claims");
    assert!(lock.try_claim().is_none(), "held lock refuses a second sweep");
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
    assert!(text.contains("Ravana") && text.contains("Cerberus"), "{text}");
    assert!(!text.contains("Sargatanas"), "clean worlds are not listed: {text}");
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ultros sweep_`
Expected: compile errors — the types don't exist.

- [ ] **Step 3: Implement**

3a. The lock (needs `use std::sync::atomic::{AtomicBool, Ordering};`):

```rust
/// Serializes full sweeps (manual and saturation-triggered): a full sweep
/// fetches every marketable item for a world, and two at once doubles the
/// load on Universalis for zero extra coverage.
#[derive(Default)]
pub(crate) struct SweepLock(AtomicBool);

/// Held for the duration of a full sweep; frees the lock on drop (including
/// panics, so a crashed sweep never wedges the command).
pub(crate) struct SweepLockGuard(Arc<SweepLock>);

impl SweepLock {
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
```

Add `pub(crate) sweep_lock: Arc<SweepLock>` to `UpdateService`, `sweep_lock: Default::default()` to the literal in `main.rs:484`, and:

```rust
pub(crate) fn try_begin_full_sweep(&self) -> Option<SweepLockGuard> {
    self.sweep_lock.try_claim()
}
```

3b. Report types (`std::time::Duration` for the durations — that's what `Instant::elapsed` returns):

```rust
pub(crate) struct WorldSweepSummary {
    world_name: String,
    tally: CatchupTally,
    duration: std::time::Duration,
}

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

pub(crate) struct SweepReport {
    worlds: Vec<WorldSweepSummary>,
    duration: std::time::Duration,
}

/// Worlds listed by name before the count collapses to "+N more" — keeps the
/// summary safely under Discord's 2000-character message cap.
const REPORT_MAX_LISTED_WORLDS: usize = 10;

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
```

3c. Rework `do_full_world_sweep` (spec §2 — runs to completion, records tallies, confirms cooldowns, drives progress):

```rust
/// Sweeps over every single marketable item in the game, ignoring the recency
/// cache. Only should be used if data is known to be lost. Never aborts:
/// failed chunks are skipped and reported via the returned [`SweepReport`].
/// `progress` fires after each world completes.
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
```

(`get_all_worlds()` yields `&&world::Model` — hence `.copied()`.) Also make `all_marketable_items` `pub(crate)` (Task 6's initial reply needs the item count) — `pub(crate) fn all_marketable_items() -> Box<[i32]>`.

The old caller in `admin.rs` (`do_full_world_sweep().await?`) no longer compiles. Patch it minimally in this task so the build stays green — Task 6 replaces it wholesale:

```rust
ctx.reply("Beginning scan").await?;
let report = ctx.data().update_service.do_full_world_sweep(|_| {}).await;
ctx.reply(report.summary_text()).await?;
Ok(())
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p ultros item_update_service && cargo clippy --all-targets -p ultros -- -D warnings`
Expected: all pass. `try_begin_full_sweep` may be dead code until Tasks 5–6 — if clippy flags it, wire the Task 5 call sites now (they're two lines) rather than allowing.

- [ ] **Step 5: Commit**

```bash
git add ultros/src/item_update_service.rs ultros/src/main.rs ultros/src/discord/ffxiv/admin.rs
git commit -m "feat(sweep): SweepReport with progress callback; full sweep runs to completion"
```

---

### Task 5: Saturation-path integration (guard + confirm/release ordering)

**Files:**
- Modify: `ultros/src/item_update_service.rs` — the saturation branch of `check_for_missed_items_on_world` (currently ~lines 250–267)

**Interfaces:**
- Consumes: `try_begin_full_sweep` (Task 4), `claim_full_sweep_slot`/`confirm_full_sweep`/`release_full_sweep_slot` (Task 3), infallible `check_items` (Task 2).
- Produces: nothing new — behavior wiring only.

- [ ] **Step 1: Rewrite the saturation branch**

Replace the body inside `if item_ids.len() >= usize::from(RECENTLY_UPDATED_WINDOW) { ... }`:

```rust
metrics::counter!("ultros_catchup_window_saturated", "world" => world.name.clone())
    .increment(1);
if self.claim_full_sweep_slot(world.id) {
    // The world slot is ours; the global lock keeps us from overlapping a
    // manual /rescan_market sweep. If it's busy, hand the world slot back
    // unstamped so the next saturated cycle retries.
    if let Some(_guard) = self.try_begin_full_sweep() {
        warn!(world = %world.name, "recency window saturated, running full item sweep");
        let tally = self.check_items(world, &Self::all_marketable_items()).await;
        tally.record(&world.name);
        self.confirm_full_sweep(world.id);
    } else {
        self.release_full_sweep_slot(world.id);
        warn!(world = %world.name, "recency window saturated, but a full sweep is already running");
    }
} else {
    warn!(world = %world.name, "recency window saturated, full sweep on cooldown");
}
// Either way every window item just got (or recently got) a full
// refetch; probing them for drift now would only re-answer the
// question the refetch already settled.
return Ok(());
```

(If Task 3/4 already pulled some of this forward to satisfy `dead_code`, reconcile to exactly this shape.)

- [ ] **Step 2: Verify**

Run: `cargo test -p ultros item_update_service && cargo clippy --all-targets -p ultros -- -D warnings`
Expected: pass, no dead-code warnings anywhere in the module now.

- [ ] **Step 3: Commit**

```bash
git add ultros/src/item_update_service.rs
git commit -m "feat(sweep): saturation sweep honors the global lock and stamps cooldown on completion"
```

---

### Task 6: Fire-and-forget `/rescan_market` with progress messages

**Files:**
- Modify: `ultros/src/discord/ffxiv/admin.rs` (full rewrite of the command)

**Interfaces:**
- Consumes: `try_begin_full_sweep` → `SweepLockGuard`, `do_full_world_sweep(progress)`, `SweepProgress::summary_text`, `SweepReport::summary_text`, `UpdateService::all_marketable_items` (Tasks 4–5). Poise `Context` from `super`.
- Produces: user-facing behavior only.

- [ ] **Step 1: Rewrite `admin.rs`**

```rust
use std::{panic::AssertUnwindSafe, time::Duration};

use futures::FutureExt;
use tokio::time::Instant;

use super::{Context, Error};

/// Minimum gap between progress messages in the channel.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(15 * 60);

#[poise::command(slash_command, prefix_command, owners_only)]
pub(crate) async fn rescan_market(ctx: Context<'_>) -> Result<(), Error> {
    let service = ctx.data().update_service.clone();
    // Claim the global sweep lock *before* replying; the guard travels into
    // the background task and frees the lock on drop, panics included.
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

    // Plain channel messages, not interaction follow-ups: slash-command
    // interaction tokens expire after 15 minutes and this task outlives that
    // by hours.
    let channel = ctx.channel_id();
    let http = ctx.serenity_context().http.clone();
    tokio::spawn(async move {
        let _guard = guard;
        // The progress callback is sync, so it can't post to Discord itself;
        // it throttles and forwards through a channel to this poster task.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let poster_http = http.clone();
        let poster = tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                if let Err(e) = channel.say(&poster_http, text).await {
                    tracing::error!(error = ?e, "failed to post sweep message to Discord");
                }
            }
        });
        let mut last_post = Instant::now();
        let progress_tx = tx.clone();
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
        drop(tx);
        let _ = poster.await;
    });
    Ok(())
}
```

Notes for the implementer:
- `world_cache` and `update_service` are `pub(crate)` fields on `UpdateService`/`Data` respectively — if `Data.update_service` is private to `discord/mod.rs` (it is: no `pub(crate)` on the field), commands in submodules access it via `ctx.data().update_service` only if `ffxiv` is a child module of `discord` — it is (`discord/ffxiv/admin.rs`), and the existing code already does `ctx.data().update_service`, so no visibility change is needed.
- `channel.say` takes any `impl CacheHttp`; `&Arc<Http>` satisfies it in this serenity version — match the pattern used by `alerts/delivery` if the compiler disagrees.
- Panics inside `tokio::spawn`ed tasks don't unwind through `catch_unwind` of *inner* awaits automatically — `catch_unwind` here wraps the sweep future itself, which is where all the work happens; that is the case the spec's "failure message instead of silence" clause targets.

- [ ] **Step 2: Verify the whole crate**

Run: `cargo clippy --all-targets -p ultros -- -D warnings && cargo test -p ultros item_update_service`
Expected: clean. There is no automated test for the command itself (poise contexts aren't constructible in unit tests); the formatting it posts is covered by Task 4's `summary_text` tests.

- [ ] **Step 3: Commit**

```bash
git add ultros/src/discord/ffxiv/admin.rs
git commit -m "feat(discord): /rescan_market runs in the background and reports progress"
```

---

### Task 7: Full-repo verification and PR

**Files:** none new.

- [ ] **Step 1: Run the repo CI script**

```bash
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: `REAL_EXIT=0`. On Windows, prepend Strawberry Perl to PATH first (CLAUDE.md); if clippy exits 137 it was OOM-killed — re-run with `-j 2`.

- [ ] **Step 2: Run the module tests one last time**

Run: `cargo test -p ultros item_update_service`
Expected: all pass (or the known `/SYM64/` link failure, noted in the PR).

- [ ] **Step 3: Push and open the PR**

```bash
git push -u origin claude/market-sweep-error-handling-e16bb3
```

Then `gh pr create` against `main` with a body covering: the abort-on-first-error root cause (the `/rescan_market` Discord error), the five design changes, the new `ultros_sweep_chunks_failed` metric, and a link to the spec. End the body with the standard Claude Code attribution footer.
