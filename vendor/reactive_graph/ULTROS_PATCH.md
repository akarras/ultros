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
- `ArcMemo::try_read_untracked` no longer `unwrap()`s: if it finds the value
  taken out, it waits for the computing thread and retries under the same
  read lock it hands out. A read from *inside the memo's own closure*
  (reentrancy — never supported; it used to recurse until the stack
  overflowed) now returns `None` from `try_*` / the usual "disposed" panic
  from `get()`.

`tests/memo_concurrent.rs` reproduces the race (it fails on pristine 0.2.14
with tens of thousands of panics in two seconds and a stale final value) and
pins the reentrancy behaviour.

## Upstream status

Not yet reported upstream as of 2026-09-13; `reactive_graph` on `main` still
has the same `take()` / unconditional-`Clean` code. Once it is fixed upstream
and `leptos` picks up the new version, delete this directory and the
`[patch.crates-io]` entry.

## Maintaining

- Bumping `leptos` to a version that pulls a newer `reactive_graph` will make
  cargo warn that the patch is unused (`[patch]` only applies to the exact
  semver range it satisfies) — re-vendor the new version and re-apply
  `ULTROS_PATCH.diff`.
- Format with the crate's own `rustfmt.toml` (`cargo fmt` inside this
  directory); the root `cargo fmt --all` does not touch it.
- Run its tests with `cargo test --manifest-path vendor/reactive_graph/Cargo.toml`;
  `scripts/check_tests.sh` runs the regression test in CI.
