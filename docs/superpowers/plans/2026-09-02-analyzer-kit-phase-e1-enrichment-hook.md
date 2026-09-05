# Analyzer Kit Phase E1: Enrichment Hook Extracted, Flip Finder Switched — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The flip finder's visible-window enrichment effect becomes a kit hook, `use_visible_enrichment`, in a new `analyzer_kit/enrichment.rs`, and the flip finder runs on it. Same requests, same payloads, same cell states, same numbers, same URL contract; every existing enrichment, width and URL test green. No changelog, no i18n key, no Labs flag, no DOM change.

**Architecture:** `enrichment.rs` owns a generic accumulating store `Enrichment<K, V> { map, settled }` whose values fold per key through a one-method `Absorb` trait, the pure window/chunk/guard helpers (`visible_keys`, `chunk_keys`, `verdict`), an `EnrichmentConfig`, and the hook: two client `Effect`s (scope reset; window → debounce → claim → chunked fetch → guard → merge) exactly as the flip finder runs them today, generic over key `K: Copy + Eq + Hash`, value `V`, scope `S` and a `fetch: Fn(S, Vec<K>) -> impl Future<Output = Vec<(K, V)>>`. The flip finder's two maps plus one `settled` set become one store whose value is the composite `FlipEnrichment { quality: Option<ResaleQualityRow>, sparkline: Option<Vec<u32>> }`, absorbed per feed so a re-fetched key keeps the half a failed feed did not return (today's two independent `extend`s exactly); its two POSTs fold into one `async fn fetch_flip_enrichment` through a pure, tested `zip_flip_enrichment`. The three filter memos and five cells keep `with`-style keyed reads (the store deliberately has no `Clone`, so a whole-store `.get()` cannot come back). The realtime market-subscription effect keeps its own window slice and reads the margin constant from the kit. The only new behaviour is chunking above `max_keys_per_request = 200`, a no-op for today's 88–92-key window (a second chunk needs a usable viewport above 5280 px — innerHeight 5412 — where the old single sparklines POST was rejected with a 400).

**Tech Stack:** Rust 2024, Leptos 0.8.20 / reactive_graph 0.2.14 (SSR + hydrate), gloo-timers 0.4 (`futures`), futures 0.3, the analyzer kit (`ultros-frontend/ultros-app/src/analyzer_kit/`), `ultros-api-types`.

**Specs:** `docs/superpowers/specs/2026-09-01-analyzer-kit-design.md` — §3 module table (`enrichment.rs`, L122), §6 "Lazy" (the hook signature and its nine behaviours, L322–335), §8 Phase E1 (L414–416) and the variant ledger (L373–379), §11 Labs (L499–521: refactor phases ship unflagged). The lifted effect's own design: `docs/superpowers/specs/2026-06-06-analyzer-visible-window-enrichment-design.md`. Line numbers below are against HEAD `19a4da63` (branch `claude/issue-1233-phase-d-signal-columns`); they shift as tasks land — search for the quoted code.

## Global Constraints

- **Pure refactor.** `/flip-finder` issues the same two POSTs (`/api/v1/resale_quality/{world}` with `window_days: 30`, `/api/v1/sparklines/{world}` with `hours: 168`) for the same key sets at every viewport under ~5400 CSS px (above it a window exceeds 200 keys: the old single sparklines POST was rejected with a 400 and Trend went empty, so the chunked path there is the one declared improvement), renders the same DOM for every store state (skeleton / value / "—"), computes the same filter results, and keeps every `?cols=`, `?sort=`, filter key and width. No changelog, no i18n key (nothing user-facing is added), no Labs token, no `?labs=`. Not touched: `sort_rows`, `optional_column_width_px`, `analyzer_skeleton_columns`, any `w-[..]` class, the `hydrated` gate, the realtime subscription's behaviour.
- `./check_ci.sh` (fmt-check + `cargo clippy --all-targets -- -D warnings`) must exit 0 before the PR; **no `#[allow(dead_code)]`**. Read its exit code from a file, never through a pipe: `./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"`. On Windows, Strawberry Perl must lead `PATH` (`export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"`).
- Under `pub(crate)` modules and `-D warnings`, any field, fn, variant or `pub use` whose only readers are tests fails CI. Kit items are dead **between** tasks by design (Task 1's `Enrichment`, `Absorb`, `chunk_keys` and `EnrichmentConfig` have no reader until Task 2's hook; the hook has none until Task 4); the branch-level gate is `check_ci.sh` in Task 5. Each task's own gate is `cargo test -p ultros-app --lib -- <filter>`, which tolerates dead-code warnings. E1's hook has its consumer (the flip finder) in the same PR, so nothing in the kit is dead at the PR gate.
- **No HashMap iteration order may reach the DOM** (hydration). The store is only ever looked up by key; `zip_flip_enrichment` iterates a map into a `Vec` that feeds another map, never a view.
- **The fetch stays inside a client `Effect`.** `post_api`'s SSR arm is `unreachable!` (`api.rs:1196-1202`); `Effect::new` never runs on the server — its body is compiled out unless `reactive_graph/effects` is on, which only `leptos/hydrate` and `leptos/csr` enable (reactive_graph 0.2.14 `effect/effect.rs:168-172`; nothing in the `ssr` feature graph turns it on) — which is the only reason the flip finder's fetch has never panicked a server render. The hook uses `Effect::new` only: `Effect::new_isomorphic` (`effect.rs:403`) runs under SSR and would put `fetch` on the server render path, and `spawn_local` / `TimeoutFuture` appear only inside an `Effect::new` body, never at the hook's top level (which runs during SSR component construction). The route's other client-only code is gated with `#[cfg(feature = "hydrate")]` (`analyzer.rs:1442-1484`, DOM listeners); the hook needs no cfg because its effect bodies are compiled out. No `Resource`, `LocalResource`, memo or `Suspense` may call `fetch`. The store starts empty on both sides so lazy cells render the skeleton on both first paints (kit §3 hydration invariant, L220–222).
- **`try_*` on every signal touched after an `.await`** (`fetch_id`, `requested`, `scope`, `store`) — a plain read of a disposed signal panics and takes the wasm bundle down (`search_box.rs:147-153`, GlitchTip #6874). The guard goes through `verdict(sig.try_get_untracked(), &captured)`.
- **`requested` stays a non-reactive `StoredValue`** (kit §6 L325–326): the filter memo reads the store, so a reactive claim set would loop recompute → refetch.
- **No wasm-only code in a test.** `TimeoutFuture`, `spawn_local` and the `Effect` bodies are exercised by the existing suite and the manual checks; unit tests cover the pure pieces only.
- Run `cargo` in the **foreground** inside subagents (a backgrounded build that outlives its session leaves uncommitted work behind). No bare `git stash`.
- Branch `claude/issue-1233-phase-e1-enrichment-hook`, rebased onto `main` at `5bb273e3` (Phase D, #1259, merged). The PR targets `main`, so `rust.yml` runs; the local `./check_ci.sh` stays the pre-push gate.

## Decisions taken in this plan (the spec and the controller left them open, or the spec is overridden)

| Question | Decision |
|---|---|
| The variant ledger puts `Layer::Lazy`, `LazyFeed`, `Sortability::LazyNever` and the lazy `CellValue`s in E1 (kit L377–378) | **Deferred to their first constructor** (E2 for `Layer::Lazy(LazyFeed::Sparklines)`, `LazyNever` and `CellValue::Sparkline`; G for the rest), with `Enrich<V>`, `AnalyzerRow::enrich_key` and any `AnalyzerGrid` `visible_range` prop. E1 is a pure refactor and the flip finder is not on the kit's table until G, so no E1 production code would construct them; under `-D warnings` an unconstructed variant fails CI (kit decision 2). Same call Phase A took for the spec's `layers.rs` items (kit L393–395; they now live in `columns.rs` / `needed.rs`) and Phase B for `visible_range` (Phase B plan L1081). |
| One store or two (two maps share one `settled` set today) | **One store, composite value** `FlipEnrichment { quality: Option<ResaleQualityRow>, sparkline: Option<Vec<u32>> }`. Readers keep their exact semantics: `quality_for` / `sparkline_for` are `get(key).and_then(..)`, `is_settled` is the shared set. Values fold per feed through `Absorb` (next row). |
| Composite merge: replace the whole value, or merge per feed? | **Per feed** (the coordinator's option (a)). `Enrichment::merge` folds each returned value into the stored one through a one-method trait `Absorb { fn absorb(&mut self, newer: Self) }`; `FlipEnrichment` takes each half only when the newer value has it — exactly today's `m.quality.extend(..)` / `m.sparkline.extend(..)` pair. A whole-value replace would differ once: after a world switch-and-back while the first world's batch is in flight (`AnalyzerTable` is updated in place, not remounted — the reset clears the claim set, the in-flight batch still passes the value-equality scope guard, and a later fetch re-claims the same keys), the second merge would drop the half the later batch's failed feed did not return. The trait costs one bound on `V` and a two-line impl per consumer (E2's recipe value is `*self = newer`); test fixtures implement it for `&'static str` / `u8` inside the test module. |
| Shape of `fetch` | `Fn(S, Vec<K>) -> Fut`, `Fut: Future<Output = Vec<(K, V)>> + 'static`, no error channel. The page's fetch absorbs each feed's `Result` (today's `if let Ok(..)` merges, one per feed) in the pure `zip_flip_enrichment`; the hook settles every requested key whatever came back, which is the spec's "settle on success or error". `Fut` is not `Send` (gloo-net) — fine, `Effect::new` takes a `'static` closure with no `Send` bound (reactive_graph 0.2.14 `effect/effect.rs:168`) and `spawn_local` wants `'static` only. |
| Where `PREFETCH_MARGIN` and `DEBOUNCE_MS` live | **`enrichment.rs`**, `pub`. Both consumers in `analyzer.rs` import them: `FLIP_ENRICHMENT` (the config) and the realtime market-subscription effect, which keeps its own inline `[start - margin, end + margin)` slice (it wants item ids only, deduped, no `seen`). E2's recipe config reuses them. |
| `visible_keys` | **Moves to the kit** with its five tests verbatim: it is the hook's window logic, and a kit hook must not reach into a route's private fn. Generic over `K` (`(i32, bool)` at the flip finder). |
| Who owns `visible_range` | **The page.** The hook takes `Signal<(usize, usize)>`; the realtime effect and the `VirtualScroller` prop keep the page's `RwSignal`. |
| `key_of` type | A plain `fn(&T) -> K` pointer (`flip_key`, a named fn), not `impl Fn`: `Copy` across effect runs, pins `T` for inference, and dodges the fn-item/`dyn Fn` lifetime trap (review trap 3) by never being `dyn`. |
| `rows` type | `Signal<Vec<T>>` (the flip finder passes `sorted_data.into()`; `From<Memo<T>> for Signal<T>` at reactive_graph `wrappers.rs:937`). |
| Chunk cap and shape | `max_keys_per_request = 200` (the smaller of sparklines 200 / resale quality 250). Chunks go out in parallel through `futures::future::join_all` — one chunk today, so one call to `fetch`, whose `futures::join!` of the two POSTs is unchanged. `chunk_keys` treats `max == 0` as 1 (never a `chunks(0)` panic). |
| The guard as a pure fn | `verdict(observed: Option<T>, expected: &T) -> Verdict { Proceed, Stale, Disposed }`, the shape of `search_box::search_outcome`, used for both the generation check and the scope check. |
| `Enrichment` derives | `Debug` only, `Default` hand-written (a derived `Default` would bound `K: Default + V: Default`, and the hook resets the store for any `K`/`V`). **No `Clone`**: the five cells and the suspicious filter drop their whole-store `.get()` clones for keyed `with` reads, and the missing impl makes a regression a compile error. |
| The suspicious filter's per-row `enrichment.get()` (`analyzer.rs:1814`) | Becomes a `with` read (allowed: no behaviour change; it cloned two maps and a set once per row per recompute). |
| Store placement | Stays inside `AnalyzerTable` (`analyzer.rs:1601`), reset by the hook on a world change as today. The spec's "recipe store lives at page level" is an E2 statement. |
| The spec's "tests compute the window from `rows_for_viewport`" | `rows_for_viewport` (`virtual_scroller.rs:121`) becomes `pub(crate)` (it keeps its live caller in that file) — a visibility-only edit that overrides kit §3's "Untouched: `virtual_scroller.rs`" (L133–135) in favour of §6's own test requirement (L331–332); no code path changes. The three geometry literals the flip finder passes to `VirtualScroller` (`row_height=40.0`, `overscan=8`, `header_height=56.0`, `analyzer.rs:2541-2552`) are hoisted into `FLIP_ROW_HEIGHT_PX` / `FLIP_OVERSCAN_ROWS` / `FLIP_HEADER_HEIGHT_PX` (identical values, no DOM change) so `flip_window_is_one_request_below_the_derived_threshold` binds to the values the `view!` uses and goes red on drift instead of staying silently green; it derives 28 / 32 rows → 88 / 92 keys and the viewport at which a second chunk starts (132 usable rows = 5280 px, innerHeight 5412) rather than quoting them. |
| `#[allow(dead_code)]` on `api.rs::{get_resale_quality, post_sparklines}` | Left alone: not a kit file, and the fns are live. |
| PR base | `main` — #1259 merged as `5bb273e3` and this branch is rebased onto it. |

## File map

| File | Responsibility in this phase |
|---|---|
| `ultros-frontend/ultros-app/src/analyzer_kit/enrichment.rs` (new) | `PREFETCH_MARGIN`, `DEBOUNCE_MS`, `EnrichmentConfig`, `Enrichment<K, V>`, `visible_keys`, `chunk_keys` (Task 1); `Verdict`, `verdict`, `use_visible_enrichment` (Task 2). |
| `ultros-frontend/ultros-app/src/analyzer_kit/mod.rs` | `pub mod enrichment;` + module doc line (Task 1). |
| `ultros-frontend/ultros-app/src/routes/analyzer.rs` | Import the moved constants and `visible_keys`, drop the local copies and the five moved tests (Task 1); `FlipKey`, `FlipEnrichment` (+ its `Absorb` impl), `FlipStore`, `quality_for`, `sparkline_for`, `zip_flip_enrichment`, all readers on `with` (Task 3); `flip_key`, `fetch_flip_enrichment`, `FLIP_ENRICHMENT`, the three geometry consts at their `VirtualScroller` prop sites, the hook call replacing two effects and two signals, three retargeted comments, the window test (Task 4). |
| `ultros-frontend/ultros-app/src/components/virtual_scroller.rs:121` | `pub(crate) fn rows_for_viewport` (Task 4). |
| `C:\Users\chw11\AppData\Local\Temp\claude\C--Users-chw11-code-ultros--claude-worktrees-issue-1233-solution-44f845\f9452a6d-a8e8-4c58-bf83-a594d061fa3d\scratchpad\phase-e1\phase-e1-pr-body.md` (scratchpad, not committed) | PR body (Task 5). |

## Test commands used below

```bash
cargo test -p ultros-app --lib -- analyzer_kit::enrichment
cargo test -p ultros-app --lib -- routes::analyzer
cargo test -p ultros-app --lib
cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown
```

All from the worktree root. The default feature is `ssr`, so `cargo test` compiles the server flavour; the wasm check is what proves the hook and the `async fn` fetch compile for the client. Run the wasm check with **no `RUSTFLAGS` in the environment** — an env `RUSTFLAGS` replaces `[build] rustflags` and fakes `i32`/`f64` web-sys errors.

---

### Task 1: `enrichment.rs` — the store, the config, the constants, and the pure window and chunk helpers

**Files:**
- Create: `ultros-frontend/ultros-app/src/analyzer_kit/enrichment.rs`
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/mod.rs` (add `pub mod enrichment;`)
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs:581-610` (delete the two constants and `visible_keys`, plus the blank line after it), `:3834-3885` (delete the five `visible_keys_*` tests, plus the blank line after them), imports.

**Interfaces:**
- Consumes: nothing from the tree yet.
- Produces (all `pub` in a `pub(crate)` module):
  - `pub const PREFETCH_MARGIN: usize = 30;` and `pub const DEBOUNCE_MS: u32 = 150;` — read by `analyzer.rs` from this task on.
  - `pub struct EnrichmentConfig { pub prefetch_margin: usize, pub debounce_ms: u32, pub max_keys_per_request: usize }` (Copy, Clone, Debug, PartialEq, Eq).
  - `pub struct Enrichment<K, V>` (Debug; hand-written `Default`) with `pub fn get(&self, key: &K) -> Option<&V>`, `pub fn is_settled(&self, key: &K) -> bool`, `pub fn merge(&mut self, requested: &[K], results: Vec<(K, V)>) where V: Absorb`.
  - `pub trait Absorb { fn absorb(&mut self, newer: Self); }` — how a value fetched again folds into the one already stored (replace for an indivisible value; per feed for the flip finder's composite).
  - `pub fn visible_keys<T, K: Eq + Hash>(data: &[T], range: (usize, usize), margin: usize, seen: &HashSet<K>, key_of: impl Fn(&T) -> K) -> Vec<K>` — the flip finder's fn, generic over `K`.
  - `pub fn chunk_keys<K: Copy>(keys: &[K], max: usize) -> Vec<Vec<K>>`.

- [ ] **Step 1: Write the module with its tests**

Create `enrichment.rs`:

```rust
//! Visible-window lazy enrichment: a keyed store that fills in behind a
//! virtual scroller, and the hook that fills it — a lift of the flip
//! finder's effect (kit spec §6 "Lazy"). The flip finder is the first
//! consumer; the recipe analyzer's Trend column (Phase E2) is the second.
//!
//! The store starts empty on the server and on the first client paint
//! (`Effect::new` never runs during SSR), so lazy cells render their
//! skeleton identically on both sides — the kit's hydration invariant.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// Rows fetched above and below the rendered window, so enrichment lands
/// just before a row scrolls into view. The flip finder's Window-mode
/// scroller renders 28 rows on the SSR shape and about 32 at 1080p, so a
/// batch is 88–92 keys — under the 200-key sparklines and 250-key
/// resale-quality caps (`routes::analyzer::tests::
/// flip_window_is_one_request_below_the_derived_threshold` derives the row
/// counts from `rows_for_viewport` with the flip finder's scroller settings,
/// and the viewport height at which a second chunk starts).
pub const PREFETCH_MARGIN: usize = 30;
/// Debounce window for scroll-driven fetches (ms). Mirrors search_box.rs.
pub const DEBOUNCE_MS: u32 = 150;

/// Per-page tuning for [`use_visible_enrichment`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EnrichmentConfig {
    pub prefetch_margin: usize,
    pub debounce_ms: u32,
    /// Keys per request: a window larger than this goes out as several
    /// parallel requests. Set it to the smallest cap among the endpoints
    /// the page's `fetch` calls.
    pub max_keys_per_request: usize,
}

/// How a value fetched again folds into the one already stored for its
/// key. A consumer whose value is one indivisible datum replaces
/// (`*self = newer`); the flip finder's composite absorbs per feed, so a
/// batch that lost one feed keeps the earlier half — today's two
/// independent map `extend`s exactly.
pub trait Absorb {
    fn absorb(&mut self, newer: Self);
}

/// Accumulating per-key enrichment. `settled` holds every key a fetch has
/// completed for, with or without data, so a reader can tell "still
/// loading" (absent from both) from "fetched, nothing known" (settled,
/// absent from `map`). Only ever looked up by key — nothing iterates it
/// into the DOM — and deliberately not `Clone`: readers go through
/// `RwSignal::with`, never a whole-store `get()`.
#[derive(Debug)]
pub struct Enrichment<K, V> {
    map: HashMap<K, V>,
    settled: HashSet<K>,
}

// Hand-written: a derived `Default` would demand `K: Default + V: Default`,
// and the hook resets the store for any `K` / `V`.
impl<K, V> Default for Enrichment<K, V> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            settled: HashSet::new(),
        }
    }
}

impl<K: Copy + Eq + Hash, V> Enrichment<K, V> {
    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    pub fn is_settled(&self, key: &K) -> bool {
        self.settled.contains(key)
    }

    /// Merge one batch: every value folds into `map` — a new key is
    /// inserted, an existing one [`Absorb`]s the newer value — and every
    /// `requested` key is settled whether or not a value came back for it.
    /// A fetch that failed contributes an empty `results` and still settles
    /// its keys, so cells switch loading -> "—" instead of shimmering
    /// forever.
    pub fn merge(&mut self, requested: &[K], results: Vec<(K, V)>)
    where
        V: Absorb,
    {
        for (key, value) in results {
            match self.map.entry(key) {
                Entry::Occupied(mut slot) => slot.get_mut().absorb(value),
                Entry::Vacant(slot) => {
                    slot.insert(value);
                }
            }
        }
        self.settled.extend(requested.iter().copied());
    }
}

/// Keys in the `[start - margin, end + margin)` slice of `data`, minus `seen`.
/// Generic over the row type + a key extractor so it unit-tests with plain
/// `(i32, bool)` fixtures — no `CalculatedProfitData` / DOM needed.
pub fn visible_keys<T, K: Eq + Hash>(
    data: &[T],
    range: (usize, usize),
    margin: usize,
    seen: &HashSet<K>,
    key_of: impl Fn(&T) -> K,
) -> Vec<K> {
    let (start, end) = range;
    let lo = start.saturating_sub(margin);
    let hi = (end + margin).min(data.len());
    data.get(lo..hi)
        .unwrap_or(&[])
        .iter()
        .map(key_of)
        .filter(|k| !seen.contains(k))
        .collect()
}

/// Split a batch into requests of at most `max` keys, row order preserved.
/// `max == 0` is treated as 1 (a `chunks(0)` would panic).
pub fn chunk_keys<K: Copy>(keys: &[K], max: usize) -> Vec<Vec<K>> {
    keys.chunks(max.max(1)).map(<[K]>::to_vec).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test values are indivisible: absorbing replaces.
    impl Absorb for &'static str {
        fn absorb(&mut self, newer: Self) {
            *self = newer;
        }
    }

    impl Absorb for u8 {
        fn absorb(&mut self, newer: Self) {
            *self = newer;
        }
    }

    #[test]
    fn visible_keys_includes_window_and_margin() {
        let data: Vec<(i32, bool)> = (0..100).map(|i| (i, false)).collect();
        let seen = std::collections::HashSet::new();
        // rendered rows [40, 50), margin 5 => slice [35, 55)
        let keys = visible_keys(&data, (40, 50), 5, &seen, |k| *k);
        assert_eq!(keys.len(), 20);
        assert_eq!(keys.first(), Some(&(35, false)));
        assert_eq!(keys.last(), Some(&(54, false)));
    }

    #[test]
    fn visible_keys_clamps_at_start_and_end() {
        let data: Vec<(i32, bool)> = (0..10).map(|i| (i, false)).collect();
        let seen = std::collections::HashSet::new();
        // start clamp: lo = 2.saturating_sub(5) = 0
        // end clamp: hi = (8 + 5).min(10) = 10 (would be 13 unclamped) => slice [0, 10)
        let keys = visible_keys(&data, (2, 8), 5, &seen, |k| *k);
        assert_eq!(keys.len(), 10);
        assert_eq!(keys.first(), Some(&(0, false)));
        assert_eq!(keys.last(), Some(&(9, false)));
    }

    #[test]
    fn visible_keys_excludes_already_seen() {
        let data: Vec<(i32, bool)> = (0..10).map(|i| (i, false)).collect();
        let mut seen = std::collections::HashSet::new();
        seen.insert((3, false));
        seen.insert((5, false));
        let keys = visible_keys(&data, (0, 10), 0, &seen, |k| *k);
        assert_eq!(keys.len(), 8);
        assert!(!keys.contains(&(3, false)));
        assert!(!keys.contains(&(5, false)));
    }

    #[test]
    fn visible_keys_empty_data_yields_empty() {
        let data: Vec<(i32, bool)> = Vec::new();
        let seen = std::collections::HashSet::new();
        let keys = visible_keys(&data, (0, 0), 30, &seen, |k| *k);
        assert!(keys.is_empty());
    }

    #[test]
    fn visible_keys_out_of_range_yields_empty() {
        let data: Vec<(i32, bool)> = (0..5).map(|i| (i, false)).collect();
        let seen = std::collections::HashSet::new();
        // lo = 95, hi = (110 + 5).min(5) = 5 => get(95..5) is an invalid range => &[]
        let keys = visible_keys(&data, (100, 110), 5, &seen, |k| *k);
        assert!(keys.is_empty());
    }

    #[test]
    fn chunk_keys_splits_above_the_cap_and_keeps_order() {
        assert_eq!(
            chunk_keys(&[1, 2, 3, 4, 5], 2),
            vec![vec![1, 2], vec![3, 4], vec![5]]
        );
        assert_eq!(chunk_keys(&[1, 2, 3, 4], 2), vec![vec![1, 2], vec![3, 4]]);
    }

    #[test]
    fn chunk_keys_is_one_request_at_or_under_the_cap() {
        let keys: Vec<i32> = (0..92).collect();
        assert_eq!(chunk_keys(&keys, 200), vec![keys.clone()]);
        assert_eq!(chunk_keys(&keys, 92).len(), 1);
    }

    #[test]
    fn chunk_keys_handles_empty_and_a_zero_cap() {
        assert!(chunk_keys::<i32>(&[], 200).is_empty());
        assert_eq!(chunk_keys(&[7, 8], 0), vec![vec![7], vec![8]]);
    }

    #[test]
    fn a_settled_key_without_a_value_is_missing_not_loading() {
        let mut store: Enrichment<(i32, bool), &'static str> = Enrichment::default();
        // Nothing fetched: loading.
        assert!(!store.is_settled(&(1, false)));
        assert_eq!(store.get(&(1, false)), None);
        store.merge(&[(1, false), (2, false)], vec![((2, false), "two")]);
        // 1 was asked for, nothing came back: fetched, no data -> "—".
        assert!(store.is_settled(&(1, false)));
        assert_eq!(store.get(&(1, false)), None);
        // 2 came back: ready.
        assert!(store.is_settled(&(2, false)));
        assert_eq!(store.get(&(2, false)), Some(&"two"));
        // 3 was never asked for: still loading.
        assert!(!store.is_settled(&(3, false)));
        assert_eq!(store.get(&(3, false)), None);
    }

    #[test]
    fn merge_accumulates_and_a_failed_batch_still_settles() {
        let mut store: Enrichment<i32, u8> = Enrichment::default();
        store.merge(&[1], vec![(1, 10)]);
        // A failed batch is an empty result set: its keys settle, earlier
        // values stay.
        store.merge(&[1, 2], Vec::new());
        assert_eq!(store.get(&1), Some(&10));
        assert!(store.is_settled(&2));
        assert_eq!(store.get(&2), None);
        // A value fetched again is absorbed: for an indivisible value, replaced.
        store.merge(&[1], vec![(1, 11)]);
        assert_eq!(store.get(&1), Some(&11));
        // A reset (what the scope-change effect does) drops everything.
        store = Enrichment::default();
        assert_eq!(store.get(&1), None);
        assert!(!store.is_settled(&1));
    }
}
```

Add `pub mod enrichment;` to `analyzer_kit/mod.rs` (alphabetical: between `columns` and `formula`) and extend the module doc's list with "the visible-window enrichment store and hook (`enrichment`)".

- [ ] **Step 2: Move the flip finder onto the kit's `visible_keys` and constants**

In `routes/analyzer.rs`:

1. Delete lines 581–610 — the `PREFETCH_MARGIN` doc + const, the `DEBOUNCE_MS` doc + const, and `fn visible_keys` with its doc comment (everything from `/// Rows fetched above & below the rendered window` through the closing `}` of `visible_keys` at 609), plus the blank line 610 so no double blank is left for fmt-check.
2. Add, after the `use crate::analysis::{ … };` block at the top of the file:

```rust
use crate::analyzer_kit::enrichment::{DEBOUNCE_MS, PREFETCH_MARGIN, visible_keys};
```

3. In `mod tests`, delete the five `visible_keys_*` tests (`visible_keys_includes_window_and_margin`, `visible_keys_clamps_at_start_and_end`, `visible_keys_excludes_already_seen`, `visible_keys_empty_data_yields_empty`, `visible_keys_out_of_range_yields_empty`, lines 3834–3885 — the block between `estimated_sale_price_uses_median_not_min` and `fn calc(`: the five tests through the closing `}` at 3884, plus the trailing blank line 3885 so no double blank is left for fmt-check). They now live in `enrichment.rs` verbatim.

Nothing else changes: the fetch effect's call `visible_keys(data, range, PREFETCH_MARGIN, seen, |(_, d)| { … })` resolves to the kit fn with `K = (i32, bool)`, and `TimeoutFuture::new(DEBOUNCE_MS)` reads the kit constant.

- [ ] **Step 3: Run the tests**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::enrichment`
Expected: PASS, 10 tests (`visible_keys_*` ×5, `chunk_keys_*` ×3, `a_settled_key_without_a_value_is_missing_not_loading`, `merge_accumulates_and_a_failed_batch_still_settles`). Dead-code warnings on `EnrichmentConfig`, `Enrichment`, `Absorb` and `chunk_keys` are expected until Task 2/4.

Run: `cargo test -p ultros-app --lib -- routes::analyzer`
Expected: PASS, 63 tests (68 minus the five moved).

- [ ] **Step 4: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/enrichment.rs ultros-frontend/ultros-app/src/analyzer_kit/mod.rs ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "feat(analyzer-kit): enrichment.rs — keyed store, window keys, chunking; flip finder reads the kit's constants"
```

---

### Task 2: The hook — `use_visible_enrichment` and its after-await guard

**Files:**
- Modify: `ultros-frontend/ultros-app/src/analyzer_kit/enrichment.rs`

**Interfaces:**
- Consumes: Task 1's `Enrichment`, `EnrichmentConfig`, `visible_keys`, `chunk_keys`; `gloo_timers::future::TimeoutFuture`; `futures::future::join_all`; `leptos::task::spawn_local`.
- Produces:
  - `pub enum Verdict { Proceed, Stale, Disposed }` (Copy, Clone, Debug, PartialEq, Eq) and `pub fn verdict<T: PartialEq>(observed: Option<T>, expected: &T) -> Verdict`.
  - `pub fn use_visible_enrichment<T, K, V, S, F, Fut>(store: RwSignal<Enrichment<K, V>>, rows: Signal<Vec<T>>, visible_range: Signal<(usize, usize)>, scope: Signal<S>, key_of: fn(&T) -> K, fetch: F, cfg: EnrichmentConfig)` — exactly seven parameters (`too_many_arguments` fires above seven; do not add an eighth).

- [ ] **Step 1: Write the failing tests**

Append to `enrichment.rs`'s `mod tests`:

```rust
    #[test]
    fn verdict_proceeds_only_on_an_unchanged_live_signal() {
        assert_eq!(verdict(Some(7u64), &7), Verdict::Proceed);
        assert_eq!(verdict(Some(8u64), &7), Verdict::Stale);
        assert_eq!(verdict(None::<u64>, &7), Verdict::Disposed);
    }

    #[test]
    fn verdict_treats_a_scope_change_as_stale() {
        let started = "Gilgamesh".to_string();
        assert_eq!(
            verdict(Some("Gilgamesh".to_string()), &started),
            Verdict::Proceed
        );
        assert_eq!(verdict(Some("Cactuar".to_string()), &started), Verdict::Stale);
        assert_eq!(verdict(None::<String>, &started), Verdict::Disposed);
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::enrichment::tests::verdict`
Expected: compile error, `verdict` / `Verdict` not found.

- [ ] **Step 3: Add the guard and the hook**

In `enrichment.rs`, extend the imports:

```rust
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
```

and insert, after `chunk_keys`:

```rust
/// What an in-flight fetch does with a signal it re-reads after an await.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Alive and unchanged since the fetch was scheduled.
    Proceed,
    /// Alive but moved on: a newer window superseded this generation, or
    /// the scope (world) changed while the request was in flight.
    Stale,
    /// The owning component was disposed: the `try_*` read returned `None`.
    Disposed,
}

/// The guard a fetch runs after every await. `observed` is a
/// `try_get_untracked()` of the signal whose value was captured as
/// `expected` when the fetch was scheduled. Reading through `try_*` is
/// load-bearing (see `search_box::search_outcome`): a plain read of a
/// disposed signal panics and takes the wasm bundle down.
pub fn verdict<T: PartialEq>(observed: Option<T>, expected: &T) -> Verdict {
    match observed {
        None => Verdict::Disposed,
        Some(seen) if seen == *expected => Verdict::Proceed,
        Some(_) => Verdict::Stale,
    }
}

/// Fill `store` for the rows the scroller shows, `cfg.prefetch_margin` rows
/// either side, as the window moves — accumulating, never wholesale-replaced
/// except on a scope change.
///
/// Behaviours, in order (kit spec §6): visible keys with the margin; a
/// generation bump per trigger; a debounce; bail if superseded or disposed;
/// claim keys only after the debounce (so a superseded generation never
/// claims); chunk above `cfg.max_keys_per_request`; bail if the scope
/// changed while the requests were in flight; merge; settle every requested
/// key on success or error.
///
/// `requested` is a `StoredValue` — non-reactive on purpose: the page's
/// filter memo reads `store`, so a reactive claim set would loop
/// recompute -> refetch. The scope-change reset (store cleared, claims
/// cleared, generation bumped) lives here too.
///
/// Call it inside a component: it creates two `Effect::new`s, whose bodies
/// are compiled out under `leptos/ssr` (no `reactive_graph/effects`), which
/// is what keeps `fetch` — a `post_api` caller whose SSR arm is
/// `unreachable!` — client-only. Never `new_isomorphic` / `new_sync` here,
/// and never a `spawn_local` or `TimeoutFuture` outside an effect body.
pub fn use_visible_enrichment<T, K, V, S, F, Fut>(
    store: RwSignal<Enrichment<K, V>>,
    rows: Signal<Vec<T>>,
    visible_range: Signal<(usize, usize)>,
    scope: Signal<S>,
    key_of: fn(&T) -> K,
    fetch: F,
    cfg: EnrichmentConfig,
) where
    T: Send + Sync + 'static,
    K: Copy + Eq + Hash + Send + Sync + 'static,
    V: Absorb + Send + Sync + 'static,
    S: Clone + PartialEq + Send + Sync + 'static,
    F: Fn(S, Vec<K>) -> Fut + Clone + 'static,
    Fut: Future<Output = Vec<(K, V)>> + 'static,
{
    // Dedupe / loop-breaker: keys a fetch has been scheduled for.
    let requested = StoredValue::new(HashSet::<K>::new());
    // Generation counter for debounce-with-cancellation (`gen` is a reserved
    // keyword in edition 2024).
    let fetch_id = RwSignal::new(0u64);

    // Scope change: drop everything and invalidate any in-flight fetch.
    // Bumping the generation makes it bail at the guard below before it
    // claims keys, so a stale batch can neither repopulate `requested`
    // (which would strand those rows on the skeleton) nor merge another
    // scope's data. Runs once on mount too, so the first generation is >= 1.
    Effect::new(move |_| {
        let _ = scope.get(); // subscribe: re-run on scope change
        store.set(Enrichment::default());
        requested.update_value(|s| s.clear());
        fetch_id.update(|n| *n += 1);
    });

    // Select the window's keys (honouring the page's sort/filter through
    // `rows`), debounce, fetch, merge.
    Effect::new(move |_| {
        let range = visible_range.get(); // reactive: scroll
        let keys = rows.with(|data| {
            requested.with_value(|seen| {
                visible_keys(data, range, cfg.prefetch_margin, seen, key_of)
            })
        });
        if keys.is_empty() {
            return;
        }
        fetch_id.update(|n| *n += 1);
        let current_id = fetch_id.get_untracked();
        let scope_now = scope.get_untracked();
        let fetch = fetch.clone();
        leptos::task::spawn_local(async move {
            TimeoutFuture::new(cfg.debounce_ms).await; // debounce
            // Past this await the component can be disposed (navigated away,
            // scope remounted), which disposes these signals: every access
            // below is a `try_*` read through `verdict`.
            if verdict(fetch_id.try_get_untracked(), &current_id) != Verdict::Proceed {
                return; // superseded by a newer window, or disposed
            }
            // Claim post-debounce so superseded generations never claim.
            if requested
                .try_update_value(|s| s.extend(keys.iter().copied()))
                .is_none()
            {
                return; // disposed
            }
            let results: Vec<(K, V)> = futures::future::join_all(
                chunk_keys(&keys, cfg.max_keys_per_request)
                    .into_iter()
                    .map(|chunk| fetch(scope_now.clone(), chunk)),
            )
            .await
            .into_iter()
            .flatten()
            .collect();
            // The requests awaited the network: the scope may have changed
            // (the reset above already cleared `requested`, so the new scope
            // refetches these keys) or the component been disposed. Never
            // merge one scope's data into another's store.
            if verdict(scope.try_get_untracked(), &scope_now) != Verdict::Proceed {
                return;
            }
            // Merge whatever came back and settle every requested key —
            // success or error — so cells switch loading -> value / "—". No
            // retry loop: a scope change resets everything.
            let _ = store.try_update(|s| s.merge(&keys, results));
        });
    });
}
```

Why these bounds: `StoredValue::new` and `RwSignal::new` need `Send + Sync + 'static` values (reactive_graph `owner/stored_value.rs:105-111`); `Signal<Vec<T>>` exists only for `T: Send + Sync + 'static`; `scope.try_get_untracked()` needs `S: Clone`; `Effect::new` takes `impl EffectFunction + 'static` with no `Send` (`effect/effect.rs:168`), so `F` and `Fut` need not be `Send` (gloo-net futures are not); `spawn_local` needs `'static` only; `V: Absorb` is what `merge` needs (Task 1). `join_all` collects the futures eagerly, so the borrow of `fetch` / `scope_now` in the `map` closure ends before the first `.await`.

- [ ] **Step 4: Run the tests and the two compiles**

Run: `cargo test -p ultros-app --lib -- analyzer_kit::enrichment`
Expected: PASS, 12 tests. `use_visible_enrichment` is dead until Task 4 (warning expected).

Run: `cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown`
Expected: exit 0 (dead-code warnings only).

- [ ] **Step 5: Commit**

```bash
git add ultros-frontend/ultros-app/src/analyzer_kit/enrichment.rs
git commit -m "feat(analyzer-kit): use_visible_enrichment — the flip finder's lazy fetch as a generic hook"
```

---

### Task 3: The flip finder's store becomes `Enrichment<(i32, bool), FlipEnrichment>`; every reader goes through `with`

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs:47-74` (store type), `:1598-1601` (declaration), `:1678`, `:1735`, `:1806-1821` (filters), `:1927-1936` (reset effect's `set`), `:1995-2008` (fetch effect's merge), `:2794-2807`, `:2848-2867`, `:2912-2937`, `:2938-2963`, `:2964-2976` (cells), imports, tests.

**Interfaces:**
- Consumes: `Enrichment`, `Absorb` (Task 1); `ResaleQualityResponse`, `ResaleQualityRow`, `SparklinesResponse` (`ultros-api-types`); `AppResult` (`crate::error`).
- Produces (private to `analyzer.rs`):
  - `type FlipKey = (i32, bool);`
  - `struct FlipEnrichment { quality: Option<ResaleQualityRow>, sparkline: Option<Vec<u32>> }` (Clone, Debug, Default, PartialEq) with an `impl Absorb` that takes each half only when the newer value has it.
  - `type FlipStore = Enrichment<FlipKey, FlipEnrichment>;`
  - `fn quality_for<'a>(store: &'a FlipStore, key: &FlipKey) -> Option<&'a ResaleQualityRow>`, `fn sparkline_for<'a>(store: &'a FlipStore, key: &FlipKey) -> Option<&'a [u32]>`.
  - `fn zip_flip_enrichment(quality: AppResult<ResaleQualityResponse>, sparklines: AppResult<SparklinesResponse>) -> Vec<(FlipKey, FlipEnrichment)>`.

- [ ] **Step 1: Write the failing tests**

In `analyzer.rs`'s `mod tests`, add next to `calc` (the imports go inside the test module so the lib build sees no unused import):

```rust
    use ultros_api_types::sparklines::SparklineSeries;

    fn quality_row(item_id: i32, hq: bool, band: ConfidenceBand, launder: f32) -> ResaleQualityRow {
        ResaleQualityRow {
            item_id,
            hq,
            world_id: 100,
            window_days: 30,
            vwap: 1_000,
            sample_size: 12,
            sales_per_day: 0.4,
            confidence_band: band,
            launder_suspicion: launder,
        }
    }

    fn series(item_id: i32, hq: bool, points: Vec<u32>) -> SparklineSeries {
        SparklineSeries {
            item_id,
            hq,
            world_id: 100,
            points,
            first_price: 0,
            last_price: 0,
        }
    }

    #[test]
    fn zip_folds_both_feeds_into_one_value_per_key() {
        let quality = Ok(ResaleQualityResponse {
            world_id: 100,
            window_days: 30,
            rows: vec![
                quality_row(1, false, ConfidenceBand::High, 0.0),
                quality_row(2, true, ConfidenceBand::Low, 0.9),
            ],
        });
        let sparklines = Ok(SparklinesResponse {
            world_id: 100,
            series: vec![series(1, false, vec![5, 6]), series(3, false, vec![1])],
        });
        let mut got = zip_flip_enrichment(quality, sparklines);
        got.sort_by_key(|(k, _)| *k);
        assert_eq!(got.len(), 3);
        // Both halves.
        assert_eq!(got[0].0, (1, false));
        assert_eq!(
            got[0].1.quality.as_ref().map(|q| q.confidence_band),
            Some(ConfidenceBand::High)
        );
        assert_eq!(got[0].1.sparkline, Some(vec![5, 6]));
        // Quality only.
        assert_eq!(got[1].0, (2, true));
        assert!(got[1].1.quality.is_some());
        assert_eq!(got[1].1.sparkline, None);
        // Sparkline only.
        assert_eq!(got[2].0, (3, false));
        assert_eq!(got[2].1.quality, None);
        assert_eq!(got[2].1.sparkline, Some(vec![1]));
    }

    #[test]
    fn zip_keeps_the_feed_that_succeeded() {
        let sparklines = Ok(SparklinesResponse {
            world_id: 100,
            series: vec![series(1, false, vec![2, 3])],
        });
        assert_eq!(
            zip_flip_enrichment(Err(AppError::NoItem), sparklines),
            vec![(
                (1, false),
                FlipEnrichment {
                    quality: None,
                    sparkline: Some(vec![2, 3]),
                }
            )]
        );
        assert!(zip_flip_enrichment(Err(AppError::NoItem), Err(AppError::NoItem)).is_empty());
    }

    /// The three states every lazy cell and floor distinguishes, read the
    /// way the page reads them after the switch: keyed, through the store.
    #[test]
    fn flip_store_reads_tell_loading_from_missing_from_ready() {
        let mut store = FlipStore::default();
        // Nothing fetched: loading everywhere.
        assert!(quality_for(&store, &(1, false)).is_none());
        assert!(sparkline_for(&store, &(1, false)).is_none());
        assert!(!store.is_settled(&(1, false)));
        store.merge(
            &[(1, false), (2, false)],
            zip_flip_enrichment(
                Ok(ResaleQualityResponse {
                    world_id: 100,
                    window_days: 30,
                    rows: vec![quality_row(1, false, ConfidenceBand::Medium, 0.1)],
                }),
                Err(AppError::NoItem),
            ),
        );
        // One half ready, the other missing, on the same settled key.
        assert_eq!(
            quality_for(&store, &(1, false)).map(|q| q.confidence_band),
            Some(ConfidenceBand::Medium)
        );
        assert!(sparkline_for(&store, &(1, false)).is_none());
        assert!(store.is_settled(&(1, false)));
        // Asked for, nothing known: settled with both halves absent -> "—".
        assert!(quality_for(&store, &(2, false)).is_none());
        assert!(store.is_settled(&(2, false)));
        // Never asked for: skeleton.
        assert!(!store.is_settled(&(3, false)));
    }

    /// Today's two maps `extend` independently; the composite must not lose
    /// a half when a later batch for the same key lost one feed.
    #[test]
    fn flip_enrichment_absorbs_per_feed() {
        let mut store = FlipStore::default();
        store.merge(
            &[(1, false)],
            vec![(
                (1, false),
                FlipEnrichment {
                    quality: Some(quality_row(1, false, ConfidenceBand::High, 0.0)),
                    sparkline: Some(vec![1, 2]),
                },
            )],
        );
        // Sparklines came back, quality did not: the quality half survives,
        // the sparkline half is the newer one.
        store.merge(
            &[(1, false)],
            vec![(
                (1, false),
                FlipEnrichment {
                    quality: None,
                    sparkline: Some(vec![3]),
                },
            )],
        );
        assert_eq!(
            quality_for(&store, &(1, false)).map(|q| q.confidence_band),
            Some(ConfidenceBand::High)
        );
        assert_eq!(sparkline_for(&store, &(1, false)), Some(&[3u32][..]));
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- routes::analyzer::tests::zip`
Expected: compile error, `zip_flip_enrichment` / `FlipEnrichment` not found.

- [ ] **Step 3: Replace the store type**

Replace `analyzer.rs:47-74` (from `use ultros_api_types::{ resale_quality::ResaleQualityRow, …` through the closing `}` of `impl EnrichmentMaps` at 74) with:

```rust
use ultros_api_types::{
    resale_quality::{ResaleQualityResponse, ResaleQualityRow},
    sparklines::{SparklinesRequest, SparklinesResponse},
    trends::ConfidenceBand,
};

/// The flip finder's enrichment key: `(item_id, hq)`.
type FlipKey = (i32, bool);

/// What one `(item_id, hq)` gets back from the two ClickHouse feeds. Either
/// half can be absent: the rollup has no row for most items (~7% coverage),
/// and a feed that errored contributes nothing for its batch.
#[derive(Clone, Debug, Default, PartialEq)]
struct FlipEnrichment {
    quality: Option<ResaleQualityRow>,
    sparkline: Option<Vec<u32>>,
}

// Per feed, exactly as the two maps used to `extend` independently: a batch
// that lost one feed keeps the half already stored.
impl Absorb for FlipEnrichment {
    fn absorb(&mut self, newer: Self) {
        if newer.quality.is_some() {
            self.quality = newer.quality;
        }
        if newer.sparkline.is_some() {
            self.sparkline = newer.sparkline;
        }
    }
}

/// ClickHouse-backed per-row enrichment for the analyzer table, grown by
/// the visible-window hook (`use_visible_enrichment`) from one
/// `resale_quality` + one `sparklines` batch per window and looked up by
/// `(item_id, hq)` while filtering and rendering rows. A key is *settled*
/// once its batch completed, with or without data, which is how cells tell
/// "still loading" from "fetched, no CH data".
type FlipStore = Enrichment<FlipKey, FlipEnrichment>;

fn quality_for<'a>(store: &'a FlipStore, key: &FlipKey) -> Option<&'a ResaleQualityRow> {
    store.get(key).and_then(|v| v.quality.as_ref())
}

fn sparkline_for<'a>(store: &'a FlipStore, key: &FlipKey) -> Option<&'a [u32]> {
    store.get(key).and_then(|v| v.sparkline.as_deref())
}

/// Fold the two feed responses into one value per key. A feed that failed
/// contributes nothing — its keys still settle (the hook settles every
/// requested key), so those cells show "—" rather than a skeleton forever,
/// exactly as before the lift. Errors stay silent, as they were.
fn zip_flip_enrichment(
    quality: AppResult<ResaleQualityResponse>,
    sparklines: AppResult<SparklinesResponse>,
) -> Vec<(FlipKey, FlipEnrichment)> {
    let mut by_key: HashMap<FlipKey, FlipEnrichment> = HashMap::new();
    // The key is bound before the `Some(row)` move: an assignment evaluates
    // its value before its place, so `entry((row.item_id, row.hq)) = Some(row)`
    // would read a moved `row` (E0382).
    if let Ok(q) = quality {
        for row in q.rows {
            let key = (row.item_id, row.hq);
            by_key.entry(key).or_default().quality = Some(row);
        }
    }
    if let Ok(s) = sparklines {
        for series in s.series {
            let key = (series.item_id, series.hq);
            by_key.entry(key).or_default().sparkline = Some(series.points);
        }
    }
    // Map order is irrelevant: this feeds another map, never the DOM.
    by_key.into_iter().collect()
}
```

Update the imports: `error::AppError,` (line 38) becomes `error::{AppError, AppResult},`, and the Task 1 import becomes

```rust
use crate::analyzer_kit::enrichment::{
    Absorb, DEBOUNCE_MS, Enrichment, PREFETCH_MARGIN, visible_keys,
};
```

Declaration (`analyzer.rs:1598-1601`):

```rust
    // Accumulating CH enrichment (quality + sparkline + settled), grown by the
    // visible-window fetch below; never wholesale-replaced (except on a world
    // change). Cells + three filter passes read it reactively, by key.
    let enrichment = RwSignal::new(FlipStore::default());
```

- [ ] **Step 4: The three filter reads**

Velocity floor (`:1678`):

```rust
                        let ch =
                            enrichment.with(|store| quality_for(store, &key).map(|q| q.sales_per_day));
```

Combined drift/confidence/volume pass (`:1735-1738`):

```rust
                    let ch = enrichment.with(|store| {
                        quality_for(store, &key).map(|q| (q.confidence_band, q.sample_size))
                    });
```

Suspicious filter (`:1806-1821`) — the closure body after the `show_suspicious_active()` early return becomes:

```rust
                let key = (data.inner.sale_summary.item_id, data.inner.sale_summary.hq);
                // Keyed `with` read: the previous per-row `get()` cloned the
                // whole store once per row per recompute.
                enrichment.with(|store| {
                    quality_for(store, &key).is_none_or(|q| {
                        !(matches!(q.confidence_band, ConfidenceBand::Unusable)
                            || q.launder_suspicion > 0.7)
                    })
                })
```

(Same truth table: no quality row → keep; `Unusable` or launder > 0.7 → drop.)

- [ ] **Step 5: The two effects, minimally**

The reset effect (`:1927-1936`): `enrichment.set(EnrichmentMaps::default());` → `enrichment.set(FlipStore::default());`.

The fetch effect's merge (`:1995-2008`, the `let _ = enrichment.try_update(|m| { … });` block) becomes:

```rust
            let _ = enrichment
                .try_update(|store| store.merge(&keys, zip_flip_enrichment(quality, sparklines)));
```

(Both effects are deleted in Task 4; this keeps Task 3 compiling and behaving identically.)

- [ ] **Step 6: The five cells**

Inline confidence in the Item cell (`:2794-2807`):

```rust
                                            {move || {
                                                if visible_cols().contains(COL_CONFIDENCE) {
                                                    return None;
                                                }
                                                enrichment
                                                    .with(|store| {
                                                        quality_for(store, &row_key)
                                                            .map(|q| (q.confidence_band, q.sample_size))
                                                    })
                                                    .map(|(band, sample_size)| {
                                                        view! { <ConfidenceBadge band=band sample_size=sample_size /> }
                                                    })
                                            }}
```

Confidence column (`:2848-2867`) — replace the two lines `let maps = enrichment.get();` and `let (label, class) = match maps.quality_for(&row_key).map(|q| q.confidence_band) {` with:

```rust
                                        let ch_band = enrichment
                                            .with(|store| quality_for(store, &row_key).map(|q| q.confidence_band));
                                        let (label, class) = match ch_band {
```

(the match arms and the `view!` are unchanged).

Trend (`:2912-2937`):

```rust
                                    {move || visible_cols().contains(COL_TREND).then(|| {
                                        let (points, vwap, settled) = enrichment.with(|store| (
                                            sparkline_for(store, &row_key).map(<[u32]>::to_vec),
                                            quality_for(store, &row_key).map(|q| q.vwap),
                                            store.is_settled(&row_key),
                                        ));
                                        let inner = if let Some(pts) = points {
                                            let pct = vwap
                                                .map(|vwap| {
                                                    let vwap = vwap as f32;
                                                    if vwap <= 0.0 {
                                                        0.0
                                                    } else {
                                                        (row_cheapest_price as f32 - vwap) / vwap * 100.0
                                                    }
                                                })
                                                .unwrap_or(0.0);
                                            view! { <Sparkline points=pts pct_change=pct /> }.into_any()
                                        } else if settled {
                                            // fetched, no series -> empty sparkline (prior behavior)
                                            view! { <Sparkline points=Vec::new() pct_change=0.0 /> }.into_any()
                                        } else {
                                            view! { <SingleLineSkeleton /> }.into_any()
                                        };
                                        view! {
                                            <div role="cell" class="px-3 py-2 w-[100px] shrink-0 flex items-center justify-center">
                                                {inner}
                                            </div>
                                        }
                                    })}
```

Sales/day (`:2938-2963`) — replace `let maps = enrichment.get();` and the `match (maps.quality_for(&row_key), maps.is_settled(&row_key))` head with:

```rust
                                        let (quality, settled) = enrichment.with(|store| (
                                            quality_for(store, &row_key).map(|q| (q.sales_per_day, q.sample_size)),
                                            store.is_settled(&row_key),
                                        ));
                                        let inner = match (quality, settled) {
                                            (Some((sales_per_day, sample_size)), _) => {
                                                let cadence = get_sales_cadence(sales_per_day, sample_size as usize);
                                                view! { <SalesCadenceBadge cadence sales_per_day=sales_per_day compact=true /> }.into_any()
                                            }
```

(the `(None, true)` arm with `row_velocity` / `row_num_sold` / `"—"` and the `(None, false)` skeleton arm are unchanged).

30d Volume (`:2964-2976`):

```rust
                                    {move || visible_cols().contains(COL_VOLUME_30D).then(|| {
                                        let (sample_size, settled) = enrichment.with(|store| (
                                            quality_for(store, &row_key).map(|q| q.sample_size),
                                            store.is_settled(&row_key),
                                        ));
                                        let inner = match (sample_size, settled) {
                                            (Some(n), _) => view! { {n.to_string()} }.into_any(),
                                            (None, true) => view! { "—" }.into_any(),
                                            (None, false) => view! { <SingleLineSkeleton /> }.into_any(),
                                        };
                                        view! {
                                            <div role="cell" class="px-3 py-2 w-[88px] shrink-0 flex items-center justify-end font-mono tabular-nums">
                                                {inner}
                                            </div>
                                        }
                                    })}
```

Every cell keeps its element shapes, classes and state machine; only the read changed. If any `enrichment.get()` survives, the build fails with "`Enrichment<…>: Clone` is not satisfied" — that is the guard working, not a reason to add `Clone`.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p ultros-app --lib -- routes::analyzer`
Expected: PASS, 67 tests (63 + `zip_folds_both_feeds_into_one_value_per_key`, `zip_keeps_the_feed_that_succeeded`, `flip_store_reads_tell_loading_from_missing_from_ready`, `flip_enrichment_absorbs_per_feed`).

Run: `cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown`
Expected: exit 0.

- [ ] **Step 8: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/analyzer.rs
git commit -m "refactor(flip-finder): one Enrichment store with a composite value; every reader is a keyed with()"
```

---

### Task 4: The flip finder runs on the hook

**Files:**
- Modify: `ultros-frontend/ultros-app/src/routes/analyzer.rs` — `:1843-1853` (state), `:1924-2010` (the reset and fetch effects), `:2541`, `:2542`, `:2552` (the three `VirtualScroller` geometry props), `:1155-1156`, `:1672-1673`, `:1717-1718` (comments naming `requested`), imports, a config const, three geometry consts and two fns near `zip_flip_enrichment`, tests.
- Modify: `ultros-frontend/ultros-app/src/components/virtual_scroller.rs:121` (`pub(crate) fn rows_for_viewport`).

**Interfaces:**
- Consumes: `use_visible_enrichment`, `EnrichmentConfig`, `chunk_keys` (kit); `get_resale_quality`, `post_sparklines` (`api.rs`); `rows_for_viewport`, `SSR_FALLBACK_ROWS` (`virtual_scroller.rs`); `STICKY_BAR_HEIGHT` (`control_bar.rs`).
- Produces (private): `fn flip_key(row: &(usize, CalculatedProfitData)) -> FlipKey`; `async fn fetch_flip_enrichment(world: String, keys: Vec<FlipKey>) -> Vec<(FlipKey, FlipEnrichment)>`; `const FLIP_ENRICHMENT: EnrichmentConfig`; `const FLIP_ROW_HEIGHT_PX: f64 = 40.0`, `const FLIP_OVERSCAN_ROWS: u32 = 8`, `const FLIP_HEADER_HEIGHT_PX: f64 = 56.0` (the `VirtualScroller` props, typed as `virtual_scroller.rs:141-144` declares them).

- [ ] **Step 1: Write the failing tests**

In `analyzer.rs`'s `mod tests`:

```rust
    use crate::analyzer_kit::enrichment::chunk_keys;

    #[test]
    fn flip_key_is_item_and_hq() {
        let mut row = calc(0, 0, 0);
        Arc::make_mut(&mut row.inner).sale_summary.item_id = 42;
        Arc::make_mut(&mut row.inner).sale_summary.hq = true;
        assert_eq!(flip_key(&(0, row)), (42, true));
    }

    /// Row counts from `rows_for_viewport` with the values the `view!` passes
    /// to `VirtualScroller` (the `FLIP_*` geometry consts), not copied
    /// literals: the SSR shape (20 rows) and a 1080p window each fit in one
    /// request under the smaller endpoint cap, and the viewport at which a
    /// second chunk starts is derived, not quoted.
    #[test]
    fn flip_window_is_one_request_below_the_derived_threshold() {
        // `viewport_px` in Window mode: SSR_FALLBACK_ROWS * row_height until
        // hydrated, then (innerHeight - sticky bar) - header.
        let rows_at = |viewport: f64| {
            rows_for_viewport(viewport, FLIP_ROW_HEIGHT_PX, FLIP_OVERSCAN_ROWS) as usize
        };
        let ssr_rows = rows_at(SSR_FALLBACK_ROWS as f64 * FLIP_ROW_HEIGHT_PX);
        let hd_rows = rows_at(1080.0 - STICKY_BAR_HEIGHT - FLIP_HEADER_HEIGHT_PX);
        assert_eq!((ssr_rows, hd_rows), (28, 32));
        assert_eq!(FLIP_ENRICHMENT.max_keys_per_request, 200);
        let cap = FLIP_ENRICHMENT.max_keys_per_request;
        let margin = FLIP_ENRICHMENT.prefetch_margin;
        let chunks_for = |rows: usize| {
            let keys: Vec<FlipKey> = (0..rows + 2 * margin).map(|i| (i as i32, false)).collect();
            chunk_keys(&keys, cap).len()
        };
        assert_eq!((chunks_for(ssr_rows), chunks_for(hd_rows)), (1, 1));
        // The most rendered rows that still fit one request: the cap minus the
        // margin either side and the overscan — 132 rows, a 5280 px usable
        // viewport (innerHeight 5412 with the sticky bar and header). One
        // pixel more and the window chunks; there the old single sparklines
        // POST was rejected with a 400 instead.
        let fits_rows = cap - 2 * margin - FLIP_OVERSCAN_ROWS as usize;
        assert_eq!(fits_rows, 132);
        let fits_px = fits_rows as f64 * FLIP_ROW_HEIGHT_PX;
        assert_eq!(chunks_for(rows_at(fits_px)), 1);
        assert_eq!(chunks_for(rows_at(fits_px + 1.0)), 2);
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ultros-app --lib -- routes::analyzer::tests::flip_`
Expected: compile error, `flip_key` / `FLIP_ENRICHMENT` / `FLIP_ROW_HEIGHT_PX` / `rows_for_viewport` not found.

- [ ] **Step 3: Expose `rows_for_viewport`**

In `virtual_scroller.rs:121`: `fn rows_for_viewport(` → `pub(crate) fn rows_for_viewport(`. Its doc gains one line: "Also read by `routes::analyzer`'s window test." The glob `virtual_scroller::*` already in `analyzer.rs` brings it and `SSR_FALLBACK_ROWS` into scope.

- [ ] **Step 4: The key fn, the fetch, the config and the geometry consts**

After `zip_flip_enrichment` in `analyzer.rs`:

```rust
/// The hook's `key_of` for the sorted rows: `(item_id, hq)`.
fn flip_key((_, row): &(usize, CalculatedProfitData)) -> FlipKey {
    (row.inner.sale_summary.item_id, row.inner.sale_summary.hq)
}

/// The hook's `fetch`: both ClickHouse feeds for one batch of keys on
/// `world`, in parallel — a 30-day resale-quality window and a 168-hour
/// sparkline. Client-only by construction: the hook calls it from an
/// `Effect`, and `post_api`'s SSR arm is `unreachable!`.
async fn fetch_flip_enrichment(
    world: String,
    keys: Vec<FlipKey>,
) -> Vec<(FlipKey, FlipEnrichment)> {
    let (quality, sparklines) = futures::join!(
        get_resale_quality(&world, keys.clone(), 30),
        post_sparklines(
            &world,
            SparklinesRequest {
                items: keys,
                hours: Some(168),
            },
        ),
    );
    zip_flip_enrichment(quality, sparklines)
}

/// Both endpoints cap a batch — sparklines at 200 keys, resale quality at
/// 250 — and the smaller wins. The window is 88–92 keys, so this never
/// chunks below a 5280 px usable viewport
/// (`flip_window_is_one_request_below_the_derived_threshold` derives that);
/// above it the old single sparklines POST was rejected (400, `movers.rs:134`)
/// and Trend showed the empty series, so the chunked path only adds data.
const FLIP_ENRICHMENT: EnrichmentConfig = EnrichmentConfig {
    prefetch_margin: PREFETCH_MARGIN,
    debounce_ms: DEBOUNCE_MS,
    max_keys_per_request: 200,
};

/// The `VirtualScroller` geometry the table passes in `view!`, named so the
/// window test binds to the same values instead of copied literals.
const FLIP_ROW_HEIGHT_PX: f64 = 40.0;
const FLIP_OVERSCAN_ROWS: u32 = 8;
const FLIP_HEADER_HEIGHT_PX: f64 = 56.0;
```

In the `<VirtualScroller … />` at `:2538-2556`, replace `row_height=40.0` (`:2541`) with `row_height=FLIP_ROW_HEIGHT_PX`, `overscan=8` (`:2542`) with `overscan=FLIP_OVERSCAN_ROWS`, and `header_height=56.0` (`:2552`) with `header_height=FLIP_HEADER_HEIGHT_PX`. Identical values, so no DOM change; the prop types match (`row_height: f64`, `#[prop(optional)] header_height: f64`, `#[prop(optional)] overscan: u32` — `virtual_scroller.rs:141-144`; `#[prop(optional)]` on a non-`Option` keeps the bare type), and the comment at `:2549` quoting `overscan=8` (320px) stays true. `AnalyzerTableSkeleton`'s `rows=14` is unrelated and untouched.

Imports: the kit line becomes

```rust
use crate::analyzer_kit::enrichment::{
    Absorb, DEBOUNCE_MS, Enrichment, EnrichmentConfig, PREFETCH_MARGIN, use_visible_enrichment,
};
```

(`visible_keys` drops out: nothing in `analyzer.rs` calls it after this task. `gloo_timers::future::TimeoutFuture` stays — `AnalyzerWorldView` still debounces market refetches with it at `:3147`.)

- [ ] **Step 5: Replace the state and the two effects with the hook call**

Replace `analyzer.rs:1843-1853` (the `// --- Visible-window lazy enrichment ---` banner, `requested`, `visible_range`, `fetch_id`) with:

```rust
    // --- Visible-window lazy enrichment -------------------------------------
    // Rendered row range published by the VirtualScroller (see view! below).
    // Page-owned: the realtime market subscription below slices the same
    // window, so the hook only reads it.
    let visible_range = RwSignal::new((0usize, 0usize));
```

Leave `analyzer_market_subscription`, `worlds_for_market`, the realtime subscription effect (`:1857-1918`, it reads `PREFETCH_MARGIN` from the kit import) and its `on_cleanup` (`:1920-1922`) exactly as they are.

Delete the world-reset effect (`:1924-1936`, comment included) and the fetch effect (`:1938-2010`, comment included). In their place, immediately after the `on_cleanup`:

```rust
    // Fill `enrichment` for the rows in and around the window, debounced,
    // deduped, reset on a world change; see `analyzer_kit::enrichment`.
    use_visible_enrichment(
        enrichment,
        sorted_data.into(),
        visible_range.into(),
        world,
        flip_key,
        fetch_flip_enrichment,
        FLIP_ENRICHMENT,
    );
```

Effect order is preserved: the realtime subscription effect, then the hook's reset effect, then its fetch effect — the same creation order as the three effects it replaces.

Retarget the three comments that still name the old local, so `grep requested` in this file matches nothing after the switch:

- `:1155-1156` (the `profits` prop doc): `the `requested` dedupe set, and the realtime subscription that had just` → `the enrichment hook's claim set, and the realtime subscription that had just`.
- `:1672-1673` (velocity floor): `the non-reactive `requested` dedupe breaks the recompute ->` / `refetch loop.` → `the hook's non-reactive claim set (`analyzer_kit::enrichment`)` / `breaks the recompute -> refetch loop.`
- `:1717-1718` (combined pass): `filter's pattern; the non-reactive `requested` dedupe is` / `what keeps recompute -> refetch from looping.` → `filter's pattern; the hook's non-reactive claim set` / `(`analyzer_kit::enrichment`) keeps recompute -> refetch from looping.`

- [ ] **Step 6: Run the tests and the two compiles**

Run: `cargo test -p ultros-app --lib -- routes::analyzer`
Expected: PASS, 69 tests (67 + `flip_key_is_item_and_hq`, `flip_window_is_one_request_below_the_derived_threshold`).

Run: `cargo test -p ultros-app --lib -- analyzer_kit`
Expected: PASS (enrichment 12, plus the other kit modules unchanged).

Run: `cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown`
Expected: exit 0, and **no dead-code warning anywhere in `analyzer_kit::enrichment`** — every item now has its non-test reader (`use_visible_enrichment`, `Enrichment::{get, is_settled, merge}`, `Absorb`, `EnrichmentConfig`, `visible_keys`, `chunk_keys`, `verdict`, both constants).

- [ ] **Step 7: Commit**

```bash
git add ultros-frontend/ultros-app/src/routes/analyzer.rs ultros-frontend/ultros-app/src/components/virtual_scroller.rs
git commit -m "refactor(flip-finder): switch to use_visible_enrichment; delete the hand-rolled fetch and reset effects"
```

---

### Task 5: Suite, CI gate, PR body and the manual check list

**Files:**
- Create (scratchpad, not committed): `C:\Users\chw11\AppData\Local\Temp\claude\C--Users-chw11-code-ultros--claude-worktrees-issue-1233-solution-44f845\f9452a6d-a8e8-4c58-bf83-a594d061fa3d\scratchpad\phase-e1\phase-e1-pr-body.md`

**Interfaces:**
- Consumes: everything above.
- Produces: a green `./check_ci.sh`, the PR.

- [ ] **Step 1: fmt, tests, clippy, wasm**

```bash
cargo fmt --all
cargo test -p ultros-app --lib > /tmp/tests.log 2>&1; echo "REAL_EXIT=$?"; tail -5 /tmp/tests.log
cargo check -p ultros-app --no-default-features --features hydrate --target wasm32-unknown-unknown > /tmp/wasm.log 2>&1; echo "REAL_EXIT=$?"; tail -5 /tmp/wasm.log
export PATH="/c/Strawberry/perl/bin:/c/Strawberry/c/bin:$PATH"
./check_ci.sh > /tmp/ci.log 2>&1; echo "REAL_EXIT=$?"; tail -30 /tmp/ci.log
```

Expected: every `REAL_EXIT=0`. Clippy runs `--all-targets` with `-D warnings`; fix in place, never `#[allow]`. Likely candidates from this phase: an import left behind in `analyzer.rs` (`visible_keys`, `Entry`, `HashMap` if the zip is the last user and it moved), `clippy::unnecessary_map_or` if `is_none_or` was written as `map_or(true, ..)`, `clippy::type_complexity` if a signature spells `Vec<((i32, bool), FlipEnrichment)>` instead of using `FlipKey`, `too_many_arguments` if anything added an eighth parameter to the hook, and — a rustc error, not clippy — E0382 in `zip_flip_enrichment` if the key tuple is not bound before the `Some(row)` assignment (an assignment evaluates its value before its place). If clippy is OOM-killed (exit 137), re-run `cargo clippy --all-targets -j 2 -- -D warnings`.

Commit any fixes with `git commit -am "chore(phase-e1): fmt and clippy"`.

- [ ] **Step 2: PR body**

Write `phase-e1-pr-body.md` in the scratchpad — substitute `<N>` with the `test result: ok. N passed` total from `/tmp/tests.log` (Step 1); it is the one placeholder in this plan:

```markdown
# Analyzer kit phase E1: enrichment hook extracted, flip finder switched

**Base branch: `main`** (Phase D, #1259, merged as `5bb273e3`; this branch is rebased onto it).

Part of #1233. **Pure refactor, no player-visible change**: the flip finder issues the same two POSTs for the same keys, renders the same cells in the same states, filters the same rows, and keeps every URL token and width. No changelog, no i18n key, no Labs flag. Plan: `docs/superpowers/plans/2026-09-02-analyzer-kit-phase-e1-enrichment-hook.md`.

## What's in it

- **`analyzer_kit/enrichment.rs`** — `Enrichment<K, V>` (accumulating keyed store with a `settled` set; no `Clone` on purpose; values fold per key through a one-method `Absorb` trait), `EnrichmentConfig`, `visible_keys` (moved from the flip finder with its five tests), `chunk_keys`, `verdict` (the after-await guard as a pure fn), and `use_visible_enrichment(store, rows, visible_range, scope, key_of, fetch, cfg)`: the flip finder's effect lifted verbatim — 30-row margin, generation bump, 150 ms debounce, `try_*` guards after every await, claim after the debounce, chunk above the cap, scope-change bail, merge, settle every requested key on success or error. `requested` stays a non-reactive `StoredValue`.
- **Flip finder on the hook.** Two maps + one settled set → one `Enrichment<(i32, bool), FlipEnrichment>` with a composite value (`quality`, `sparkline`, each `Option`) that absorbs per feed, so a re-merged key keeps the half a failed feed did not return — today's two independent `extend`s exactly, pinned by `flip_enrichment_absorbs_per_feed`; the two POSTs fold through a pure `zip_flip_enrichment`; the hand-rolled reset and fetch effects, `requested` and `fetch_id` are gone. Every reader (three filter passes, five cells) is a keyed `with()`; the suspicious filter's per-row whole-store clone is gone. `visible_range` stays page-owned because the realtime subscription slices the same window; `PREFETCH_MARGIN` / `DEBOUNCE_MS` now live in the kit and both consumers import them.
- **Only new behaviour:** chunking above `max_keys_per_request = 200` (min of the 200-key sparklines / 250-key resale-quality caps). The window is 88 keys on the SSR shape and 92 at 1080p, so it never triggers at any viewport under ~5400 CSS px (a >200-key window needs a 5280 px usable viewport, innerHeight 5412); above that the old single sparklines POST was rejected (400, `movers.rs:134`) and Trend showed the empty series, so the chunked path only adds data. `flip_window_is_one_request_below_the_derived_threshold` derives the row counts and that threshold from `rows_for_viewport` (now `pub(crate)`, a visibility-only edit to `virtual_scroller.rs`) and the table's own geometry consts.
- **Deferred to E2/G, deviating from the spec's variant ledger:** `Layer::Lazy`, `LazyFeed`, `Sortability::LazyNever`, `Enrich<V>`, the lazy `CellValue`s, `AnalyzerRow::enrich_key`, a grid `visible_range` prop — none has an E1 constructor and `-D warnings` rejects an unconstructed variant (same call Phase A made for the spec's `layers.rs` items, which Phase B homed in `columns.rs` / `needed.rs`).

## Verification

- `cargo test -p ultros-app --lib`: <N> passed (analyzer.rs 69: the 5 `visible_keys_*` moved to the kit, 6 added; enrichment.rs 12).
- `cargo check … --features hydrate --target wasm32-unknown-unknown`: exit 0.
- `./check_ci.sh`: `REAL_EXIT=0`, no `#[allow]` added.

## Manual checks (reviewer step; prod `https://ultros.app` vs this build, same window size)

1. `/flip-finder/Gilgamesh`, DevTools Network filtered to `resale_quality` / `sparklines`: on load one POST to each, same `items` length on both builds, body `window_days: 30` / `hours: 168`.
2. Scroll one screen and stop: one new pair of POSTs; the union of `items` across requests has no repeats.
3. Turn on Trend and 30d Volume: cells go skeleton → sparkline / count / "—" identically on both builds; Confidence never shows a skeleton (it falls back to the derived band); Sales/day shows a skeleton until the row's window settles, then the CH cadence badge where the rollup has a row, else the buffer-derived badge (or "—" with no buffer rate) — identical on both builds; Trend colour matches (buy price vs 30d VWAP).
4. Filters `?confidence=medium`, `?min-volume=10`, `?vel=0.2`, and the Show suspicious toggle: the row count and the "rows lack data" note match prod once the visible window has settled.
5. Switch world in the picker: skeletons reappear, then the new world's POSTs; no row shows the previous world's numbers.
6. Scroll and navigate away within ~150 ms, and switch world while a request is in flight: no console error / `pageerror`.
7. `./scripts/run_e2e.sh` (desktop + mobile; `STRICT_CONSOLE` on).
```

- [ ] **Step 3: Push and open the PR**

```bash
git push -u origin claude/issue-1233-phase-e1-enrichment-hook
gh pr create --base main --title "Analyzer kit phase E1: enrichment hook extracted, flip finder switched" --body-file "<scratchpad>/phase-e1/phase-e1-pr-body.md"
```

If `main` has moved: `git fetch origin && git rebase origin/main`, re-run Step 1, `git push --force-with-lease`.

---

## Self-review (done while writing; kept for the executor)

**Spec coverage — the nine behaviours (kit §6 L322–327), each with its home:** (1) visible keys with a 30-row margin → `visible_keys` + `cfg.prefetch_margin = PREFETCH_MARGIN` (Tasks 1, 4); (2) generation bump → `fetch_id.update` per trigger (Task 2); (3) 150 ms debounce → `TimeoutFuture::new(cfg.debounce_ms)`, `DEBOUNCE_MS` (Tasks 1, 2); (4) bail if superseded or disposed → `verdict(fetch_id.try_get_untracked(), &current_id)` (Task 2, tested); (5) claim after the debounce → `requested.try_update_value` after the await (Task 2); (6) chunk above the cap → `chunk_keys` + `join_all`, cap 200 (Tasks 1, 2, 4; tested, and the window test derives the 5280 px usable-viewport threshold below which it is a no-op); (7) bail if the scope changed → `verdict(scope.try_get_untracked(), &scope_now)` (Task 2, tested); (8) merge → `Enrichment::merge` folding through `Absorb` (Tasks 1, 3; tested, including the per-feed absorb); (9) settle every requested key on success and error → `merge` settles `requested` regardless of `results`, and the page's `zip_flip_enrichment` absorbs each feed's `Err` (Tasks 1, 3; both tested). `requested` non-reactive (L325) → `StoredValue` (Task 2). Hydration invariant (L220–222) → the store is only written by client `Effect`s. "Every existing enrichment, width and URL test green" (L415) → the five `visible_keys_*` move verbatim; the width, `?cols=`, `?sort=`, filter-key and `passes_*` tests are untouched. "Tests compute the window from `rows_for_viewport`" (L331) → `flip_window_is_one_request_below_the_derived_threshold`, bound to the table's own geometry consts. Unflagged (L501) → no Labs token. No changelog (L416).

**The pure-refactor promise, checked item by item:** the two POST bodies are built by the same two calls with the same arguments (`fetch_flip_enrichment`); key selection is the same fn with the same margin over the same `sorted_data`; the reset effect does the same three writes on the same trigger; the guards read the same signals at the same points; each cell's three states map to the same `(quality_for, sparkline_for, is_settled)` reads; the suspicious filter's truth table is unchanged; the realtime effect is untouched; no `w-[..]`, class, `<!>` marker or `Option` child changed. Two things are *not* identical and are stated as such: chunking exists (a second chunk starts at a 5280 px usable viewport — innerHeight 5412 — where the old single sparklines POST was rejected with a 400, so the chunked path only adds data), and whole-store clones are gone (no observable effect). One near-miss is closed by design rather than declared: a key can be merged twice after a world switch-and-back while the first world's batch is in flight (the table is updated in place, not remounted — the reset clears the claim set, the in-flight batch still passes the value-equality scope guard, and a later fetch re-claims the same keys), and `FlipEnrichment::absorb` merges that second batch per feed exactly as today's two `extend`s did, pinned by `flip_enrichment_absorbs_per_feed`.

**Deferrals stated:** the ledger deviation is the first row of the Decisions table and a bullet in the PR body.

**Placeholder scan:** no "TBD", no "similar to Task N"; the one placeholder is the PR body's `<N>` test count, which Task 5 Step 2 says how to fill; every code step shows the code; every test step shows the test; every Run has an Expected.

**Type consistency across tasks:** `Enrichment<K, V>` bounds — `impl<K: Copy + Eq + Hash, V>` for the methods (Task 1) match the hook's `K: Copy + Eq + Hash + Send + Sync + 'static` (Task 2) and `FlipKey = (i32, bool)` (Task 3). `merge(&mut self, requested: &[K], results: Vec<(K, V)>) where V: Absorb` is called as `s.merge(&keys, results)` in the hook (whose `V: Absorb + Send + Sync + 'static` bound supplies it) and `store.merge(&keys, zip_flip_enrichment(..))` in Task 3's interim effect and tests; `FlipEnrichment: Absorb` (Task 3) and the test fixtures' `&'static str` / `u8` impls (Task 1) satisfy it. The three `FLIP_*` geometry consts are typed as the `VirtualScroller` props are (`row_height: f64`, `overscan: u32`, `header_height: f64`, `virtual_scroller.rs:141-144`) and as `rows_for_viewport(f64, f64, u32)` takes them. `visible_keys<T, K: Eq + Hash>(.., seen: &HashSet<K>, key_of: impl Fn(&T) -> K)` takes the fn pointer `key_of: fn(&T) -> K` in the hook and the `|(_, d)| ..` closure in Task 1's interim call. `fetch_flip_enrichment(String, Vec<FlipKey>) -> Vec<(FlipKey, FlipEnrichment)>` satisfies `F: Fn(S, Vec<K>) -> Fut` with `S = String` (from `world: Signal<String>`, the `AnalyzerTable` prop), and an `async fn` item is `Clone + 'static`. `rows: Signal<Vec<T>>` takes `sorted_data.into()` with `T = (usize, CalculatedProfitData)`, which `flip_key(&(usize, CalculatedProfitData))` pins. `visible_range.into()` turns the page's `RwSignal<(usize, usize)>` into the hook's `Signal<(usize, usize)>`. `EnrichmentConfig` is `Copy`, so `FLIP_ENRICHMENT` is a `const` and the effect closure captures it by value. `quality_for` / `sparkline_for` return `Option<&'a _>` tied to the store borrow and are only used inside `with` closures that return owned tuples.

**The six review traps (memory `reference_leptos_plan_review_traps`):** (1) `#[prop(optional)]` strips `Option` — no component or prop is added; the hook is a plain fn. (2) A hidden optional column still emits `<!>` — no `Option` child is added or removed; the cells' outer `.then(|| ..)` gates are untouched. (3) A `fn` item cannot unsize into `&'a dyn Fn` returning a borrowed value — `key_of` is a `fn(&T) -> K` pointer in a generic position, `fetch` is a generic `F`, and `Absorb` is a plain trait bound; nothing is `dyn`, and nothing returns a borrow. (4) Plain-key `t_string!` is `&'static str` — no `t_string!` is added or moved. (5) Locale keys — none added; the seven-locale check is vacuous. (6) `type_complexity` — `FlipKey` keeps `Vec<(FlipKey, FlipEnrichment)>` and `HashMap<FlipKey, FlipEnrichment>` far under 250; every hook parameter is a single wrapper around a short type. Also from that note: `Memo::get()` clones — every read is a `with`; the `Enrichment` type has no `Clone`, so a `.get()` cannot compile.

**Every new pub item has a non-test reader in this PR:** `PREFETCH_MARGIN` (realtime effect, `FLIP_ENRICHMENT`), `DEBOUNCE_MS` (`FLIP_ENRICHMENT`), `EnrichmentConfig` and its three fields (the hook reads all three), `Enrichment` + `get` / `is_settled` / `merge` / `Default` (cells, filters, hook), `Absorb` (its `absorb` is called by `merge`; `FlipEnrichment`'s impl is a trait impl, never flagged), `visible_keys` (hook), `chunk_keys` (hook), `Verdict::{Proceed, Stale, Disposed}` (all constructed in `verdict`, compared in the hook), `verdict` (hook, twice), `use_visible_enrichment` (`AnalyzerTable`), `rows_for_viewport` (its existing caller in `virtual_scroller.rs`). Private `analyzer.rs` items: `FlipKey`, `FlipEnrichment` and both fields, `FlipStore`, `quality_for`, `sparkline_for`, `zip_flip_enrichment`, `flip_key`, `fetch_flip_enrichment`, `FLIP_ENRICHMENT`, `FLIP_ROW_HEIGHT_PX` / `FLIP_OVERSCAN_ROWS` / `FLIP_HEADER_HEIGHT_PX` (read by the `VirtualScroller` props) — all read by the cells, filters, the `view!` or the hook call.

**Not in this plan, by decision:** the lazy variants of `columns::Layer` / `columns::Sortability` and a `LazyFeed` enum (the spec's `layers.rs` was homed in `columns.rs` / `needed.rs` by Phase B; E2/G); the recipe analyzer (E2); `AnalyzerGrid` `visible_range` (E2); `Enrich<V>` (G); the `api.rs` `#[allow(dead_code)]` pair; error logging for a failed feed (silent today, silent after; a separate change if wanted).
