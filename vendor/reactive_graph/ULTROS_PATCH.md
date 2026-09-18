# Vendored `reactive_graph` 0.2.14 — Ultros patch

This directory is an unmodified copy of the `reactive_graph` 0.2.14 crate
from crates.io (the version `leptos` 0.8.20 resolves to) **plus** the change
described below. It is wired in through `[patch.crates-io]` in the root
`Cargo.toml`, so every crate in the workspace that depends on `reactive_graph`
0.2.x transparently gets this copy.

`ULTROS_PATCH.diff` is the complete diff against the pristine crate, in the
shape it would be submitted upstream. Everything outside that diff (and this
file, `rustfmt.toml`, the `[workspace]` table in `Cargo.toml`, and the
`memo_concurrent` `[[test]]` entry) is byte-for-byte upstream.

## What it fixes

`MemoInner::update_if_necessary` `take()`s a memo's cached value out of its
`RwLock` while the memo's closure runs and only stores the new value
afterwards. Nothing serializes that window across threads, and the memo's
`state` is only stamped `Clean` *after* the closure. So on a multi-threaded
runtime two things go wrong:

1. **`called Option::unwrap() on a None value` at `arc_memo.rs:334`.**
   Thread A reads the memo, sees it `Clean`, and heads for the value. In
   between, a source changes and thread B starts recomputing — it `take()`s
   the value. Thread A now finds `None` and `unwrap()`s it. (A second shape:
   both threads see `Dirty`, both `take()`, and the loser's closure is seeded
   with `None` as its previous value.)
2. **Lost updates.** A source that changes while the closure is running marks
   the memo `Dirty`; the closure then finishes and overwrites that with
   `Clean`, so the memo serves the stale value until the *next* change.
3. **A signal read that coincides with a write on another thread panics as
   "already been disposed".** `ArcRwSignal::try_read_untracked` (and every
   other reader built on `Plain::try_new`) is a plain `RwLock::try_read`, so
   it fails whenever another thread holds -- or is queued for -- the write
   lock. `set` holds it only for the assignment, but that is enough for a
   `get` to land on, and a writing thread that gets preempted keeps the lock
   for a scheduler quantum. `get` then treats the `None` as a disposed
   signal. This is what actually made `memo_concurrent`'s stress test flake
   on CI after the first two fixes landed (akarras/ultros#1491): the memo's
   closure read its source signal at the moment the writer thread was
   setting it. It is not specific to memos -- any cross-thread
   `signal.get()` racing a `set()` can hit it.

On the Ultros server this is not theoretical. Leptos' `<Suspense>` SSR path
spawns an isomorphic effect onto the tokio pool (`Effect::new_isomorphic` →
`reactive_graph::spawn` → `tokio::spawn`). When the resource a boundary is
waiting on resolves, that effect `dry_resolve`s the boundary's children on a
worker thread at the same moment the response stream re-renders them on the
connection's thread — and both read the memos the resource just dirtied
(`filtered_listings`, `sale_probe_state`, … on the item page). Production saw
~1,000 of these panics per day (`docker logs ultros`), surfacing as:

- GlitchTip "SSR render task failed" (`ssr_drain.rs:152`, `task … panicked
  with message "called Option::unwrap() on a None value"`) when the panic
  lands on the render task, and
- ~900/day `abandoned SSR response still rendering past the drain cap`
  warnings when it lands on the effect task instead: the effect dies without
  signalling the boundary, the stream never completes, the visitor gives up,
  and `DrainOnDrop` times out 60 s later.

## What the patch does

- Adds a per-memo `ComputeLock` (mutex + condvar recording the computing
  thread) so recomputation is serialized: a thread that finds the memo
  needing an update while another thread is already computing it waits, then
  re-checks and skips the redundant run.
- Marks the memo `Clean` *before* running the closure (under the compute
  lock) and never overwrites a `Dirty` that arrived meanwhile, so no update
  is lost. The non-recompute path likewise only settles `Check` → `Clean`.
- Restores `Dirty` if the closure unwinds, allowing a later read to retry
  instead of leaving an empty cache marked `Clean`.
- `ArcMemo::try_read_untracked` no longer `unwrap()`s: if it finds the value
  taken out, it waits for the computing thread and retries under the same
  read lock it hands out. A read from *inside the memo's own closure*
  (reentrancy — never supported; it used to recurse until the stack
  overflowed) now returns `None` from `try_*` / the usual "disposed" panic
  from `get()`.
- `Plain::try_new` (the read guard behind `ArcRwSignal`, `ArcReadSignal`,
  `ArcStoredValue` and the memo's cached value) waits for a writer instead
  of failing at once: it retries `try_read`, yielding and then sleeping
  between attempts, for up to one second before returning `None`. The wait
  is bounded rather than a blocking `read()` because the writer may be
  *this* thread (a signal read from inside its own `update` closure), which
  would deadlock; that case still fails as before, just later. On wasm32
  there are no other threads, so it returns `None` immediately as before.

`tests/memo_concurrent.rs` reproduces the memo race (it fails on pristine
0.2.14 with tens of thousands of panics in two seconds and a stale final
value), pins the reentrancy behaviour and recovery after a panicking
computation, and reproduces the signal read/write race (four readers against
a tight `set` loop fail every run without the `Plain::try_new` change).

The follow-up in #1494 fixes a second race in the original retry logic: a
writer can publish and release compute between a cache miss and the reader's
ownership checks. Two such misses exhausted the one-retry limit and reported
an undisposed memo as disposed. The slow read path now claims compute and
checks the cache again before releasing it. A later panicking computation is
retried after releasing both locks; contention has no arbitrary retry limit.
The uncontended cached read still takes only its existing value read lock.

The follow-up for akarras/ultros#1511 fixes a regression the patch itself
introduced. After taking the compute lock, `update_if_necessary` called
`needs_update` a second time to skip a recompute another thread had just
finished. `needs_update` in the `Check` state walks the sources and calls
`update_if_necessary` on each, and that walk is not idempotent: on an
`ArcAsyncDerived` it *consumes* the dirty flag (returning `true` and stamping
`Clean`). A memo that reads a resource — the grid's `rows` memo on the Trends
page — walked the resource once, which recomputed the resource's source memo
and marked the resource dirty, then walked it again and cleared that flag; the
resource's own task woke to a clean node over a clean source and never reran
its future, so changing the window (or world) on Trends refetched nothing.
The re-check is now a state check: `Clean` means the recompute we waited for
has landed, anything else still runs the closure. `tests/async_source_walk.rs`
reproduces the page's graph (label, view and grid readers, fresh and hydrated)
and fails on the previous version of the patch in every shape that has a memo
reader over the async value.

The unit tests in `src/computed/arc_memo.rs` use thread-local, test-only
scheduling hooks and channels to force the publish-before-owner-check
interleaving (fails before the repair) and a computation unwind in the same
window. The stress tests retain caught panic messages and no longer replace
the process-wide panic hook, so a future failure preserves its cause.

## Effect bodies vs. a forced teardown (akarras/ultros#1520 follow-up)

`Effect::new`, `new_isomorphic`, `watch` and `watch_sync` run their bodies on
spawned tasks. On the multi-threaded server runtime a body can be mid-run on
one worker while another thread tears the tree's root down —
`leptos_integration_utils::from_app` calls `owner.unset_with_forced_cleanup()`
as the last item of every response stream, and an abandoned render drops the
root outright. Every arena read the body makes after that point panics with
"you tried to access a reactive value … but it has already been disposed",
although the body was started while the tree was alive. On Ultros that was
the tail left after #1520 (which fixed the *arena* mix-up): GlitchTip #7388
(`cookies.rs:174`, the home-world cookie memo read by a `<Suspense>` effect's
`dry_resolve` walk) and the `leptos_i18n context.rs:213` locale effect in
#7382, a handful per day, clustered under load when the effect task lags the
render. The pre-poll ordering is already safe — the effect's `Receiver` holds
a `Weak`, so a cleanup that lands *before* the body starts ends the task
silently — only the concurrent one panics.

`src/owner/activity.rs` adds a per-tree in-flight counter (`Activity`,
shared by every owner in a tree the same way the arena is). The four effect
constructors run each body under `Owner::run_effect_body` /
`effect_body_guard`, and the root's `unset_with_forced_cleanup` and its
`Drop` wait (bounded, 5 s) for the count to reach zero first. A thread that is
itself inside one of the tree's bodies never waits (thread-local
`ENTERED`), so a body that tears down its own tree cannot deadlock, and on
wasm — single-threaded, so `running > 0` always means the current thread —
the wait is never entered. `tests/effect_disposed_first_run.rs` reproduces
the race on a multi-thread runtime through both teardown paths (fails on the
previous version of the patch with the exact production message) and pins the
pre-poll orderings and the self-teardown case.

## Upstream status

Not yet reported upstream as of 2026-09-15; the newest published
`reactive_graph` (0.3.0-beta2) still has the same `take()` /
unconditional-`Clean` memo code and the same `try_read`-only signal reads.
Once it is fixed upstream and `leptos` picks up the new version, delete this
directory and the `[patch.crates-io]` entry.

## Maintaining

- Bumping `leptos` to a version that pulls a newer `reactive_graph` will make
  cargo warn that the patch is unused (`[patch]` only applies to the exact
  semver range it satisfies) — re-vendor the new version and re-apply
  `ULTROS_PATCH.diff`.
- Format with the crate's own `rustfmt.toml` (`cargo fmt` inside this
  directory); the root `cargo fmt --all` does not touch it.
- Run its tests with
  `cargo test --manifest-path vendor/reactive_graph/Cargo.toml --features effects`;
  `scripts/check_tests.sh` runs the unit tests and the `memo_concurrent`,
  `async_source_walk` and `effect_disposed_first_run` regression tests in CI. `--features effects` matters:
  without it `Effect::new` is a no-op, so `async_source_walk`'s readers never
  run and every shape passes against the broken code too.
