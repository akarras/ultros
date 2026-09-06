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

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;

use super::cells::Enrich;

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
    unavailable: HashSet<K>,
}

// Hand-written: a derived `Default` would demand `K: Default + V: Default`,
// and the hook resets the store for any `K` / `V`.
impl<K, V> Default for Enrichment<K, V> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            settled: HashSet::new(),
            unavailable: HashSet::new(),
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

    /// A failed request cannot establish whether history is missing. Keep
    /// that state distinct for filters and their data-coverage notices.
    pub fn state(&self, key: &K) -> Enrich<&V> {
        if self.unavailable.contains(key) {
            return Enrich::Unavailable;
        }
        match (self.map.get(key), self.settled.contains(key)) {
            (Some(v), _) => Enrich::Ready(v),
            (None, true) => Enrich::Missing,
            (None, false) => Enrich::Loading,
        }
    }

    /// Merge one batch: every value folds into `map` — a new key is
    /// inserted, an existing one [`Absorb`]s the newer value — and every
    /// `requested` key is settled whether or not a value came back for it.
    /// Only a successful empty response establishes that history is missing.
    /// Failed requests go through `merge_batch` instead.
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
        for key in requested {
            self.unavailable.remove(key);
        }
    }

    pub fn merge_batch(&mut self, requested: &[K], batch: EnrichmentBatch<K, V>)
    where
        V: Absorb,
    {
        match batch {
            EnrichmentBatch::Ready(values) => self.merge(requested, values),
            EnrichmentBatch::Unavailable => {
                self.settled.extend(requested.iter().copied());
                self.unavailable.extend(requested.iter().copied());
            }
        }
    }
}

/// Providers can preserve legacy vector results or explicitly report failure
/// with a Result. Empty successful responses and failed requests stay distinct.
pub enum EnrichmentBatch<K, V> {
    Ready(Vec<(K, V)>),
    Unavailable,
}

impl<K, V> From<Vec<(K, V)>> for EnrichmentBatch<K, V> {
    fn from(values: Vec<(K, V)>) -> Self {
        Self::Ready(values)
    }
}

impl<K, V, E> From<Result<Vec<(K, V)>, E>> for EnrichmentBatch<K, V> {
    fn from(result: Result<Vec<(K, V)>, E>) -> Self {
        match result {
            Ok(values) => Self::Ready(values),
            Err(_) => Self::Unavailable,
        }
    }
}

/// A sparkline key: `(item_id, the quality the row's statistics resolved
/// to)`. The recipe analyzer's Trend and Drift columns share it — one feed,
/// two columns, one request.
pub type SparkKey = (i32, bool);

/// One key's hourly price series plus the percent from its first traded
/// price to its last, which colours the sparkline and is what the Drift
/// column shows. A `Vec<u32>` rather than an `Arc<[u32]>`: the `Sparkline`
/// component takes a `Vec` and would copy either way.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SparkValue {
    pub points: Vec<u32>,
    /// `None` when nothing traded anywhere in the window — `first_price`
    /// is the first *non-zero* point, so it is 0 only for an all-empty
    /// series (`analysis::first_to_last_pct`).
    pub delta_pct: Option<f32>,
}

impl Absorb for SparkValue {
    /// One indivisible feed: a key fetched again replaces.
    fn absorb(&mut self, newer: Self) {
        *self = newer;
    }
}

pub type SparkStore = Enrichment<SparkKey, SparkValue>;

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

/// Scope epochs outlive individual scroll windows. A completed request may
/// enrich an older window, but must never enrich a previous visit to a scope
/// (including A -> B -> A). Pending debounces additionally need their window
/// generation to match, even when the newest window has no keys to fetch.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct FetchGeneration {
    scope: u64,
    window: u64,
}

struct RequestTracker<S, K> {
    scope: Option<S>,
    generation: FetchGeneration,
    requested: HashSet<K>,
}

impl<S, K> Default for RequestTracker<S, K> {
    fn default() -> Self {
        Self {
            scope: None,
            generation: FetchGeneration {
                scope: 0,
                window: 0,
            },
            requested: HashSet::new(),
        }
    }
}

impl<S: Clone + PartialEq, K: Copy + Eq + Hash> RequestTracker<S, K> {
    /// Start a new window and report whether its scope needs a fresh store.
    /// Called even for an empty window, which cancels any pending debounce.
    fn advance(&mut self, scope: &S) -> bool {
        let changed = self.scope.as_ref() != Some(scope);
        if changed {
            self.scope = Some(scope.clone());
            self.generation.scope += 1;
            self.requested.clear();
        }
        self.generation.window += 1;
        changed
    }

    fn claim(&mut self, generation: FetchGeneration, keys: &[K]) -> bool {
        if self.generation != generation {
            return false;
        }
        self.requested.extend(keys.iter().copied());
        true
    }

    fn accepts_result(&self, generation: FetchGeneration) -> bool {
        self.generation.scope == generation.scope
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
/// The request tracker is a `StoredValue` — non-reactive on purpose: the page's
/// filter memo reads `store`, so a reactive claim set would loop
/// recompute -> refetch. The scope-change reset (store cleared, claims
/// cleared, generation bumped) lives here too.
///
/// Call it inside a component: it creates an `Effect::new`, whose body
/// never run under `leptos/ssr` (`Effect::new` only spawns when
/// `reactive_graph/effects` is on — a runtime `cfg!`, so the bodies still
/// compile on the server), which is what keeps `fetch` client-only. Never
/// `new_isomorphic` / `new_sync` here, and never a `spawn_local` or
/// `TimeoutFuture` outside an effect body.
pub fn use_visible_enrichment<T, K, V, S, F, Fut, B>(
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
    Fut: Future<Output = B> + 'static,
    B: Into<EnrichmentBatch<K, V>>,
{
    let tracker = StoredValue::new(RequestTracker::<S, K>::default());

    // Scope reset and window selection share one effect: changing scope
    // must refetch even when the rows and scroll range remain identical.
    Effect::new(move |_| {
        let scope_now = scope.get();
        let range = visible_range.get(); // reactive: scroll
        let mut reset = false;
        tracker.update_value(|t| reset = t.advance(&scope_now));
        if reset {
            store.set(Enrichment::default());
        }
        let generation = tracker.with_value(|t| t.generation);
        let keys = rows.with(|data| {
            tracker.with_value(|t| {
                visible_keys(data, range, cfg.prefetch_margin, &t.requested, key_of)
            })
        });
        if keys.is_empty() {
            return;
        }
        let fetch = fetch.clone();
        leptos::task::spawn_local(async move {
            TimeoutFuture::new(cfg.debounce_ms).await; // debounce
            // Past this await the component can be disposed (navigated away,
            // scope remounted), which disposes these signals: every access
            // below is a `try_*` read through `verdict`.
            if verdict(scope.try_get_untracked(), &scope_now) != Verdict::Proceed {
                return;
            }
            // Claim post-debounce so superseded generations never claim.
            let mut claimed = false;
            let _ = tracker.try_update_value(|t| claimed = t.claim(generation, &keys));
            if !claimed {
                return; // superseded by another window/scope, or disposed
            }
            let results = futures::future::join_all(
                chunk_keys(&keys, cfg.max_keys_per_request)
                    .into_iter()
                    .map(|chunk| {
                        let request = fetch(scope_now.clone(), chunk.clone());
                        async move { (chunk, request.await.into()) }
                    }),
            )
            .await;
            // The requests awaited the network: the scope may have changed
            // (the reset above already cleared `requested`, so the new scope
            // refetches these keys) or the component been disposed. Never
            // merge one scope's data into another's store.
            if verdict(scope.try_get_untracked(), &scope_now) != Verdict::Proceed {
                return;
            }
            if tracker.try_with_value(|t| t.accepts_result(generation)) != Some(true) {
                return; // the scope changed and returned, or was disposed
            }
            // Merge whatever came back and settle every requested key —
            // success or error — so cells switch loading -> value / "—". No
            // retry loop: a scope change resets everything.
            let _ = store.try_update(|s| {
                for (chunk, batch) in results {
                    s.merge_batch(&chunk, batch);
                }
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_the_fetch_gate_cancels_a_pending_debounce_without_claiming_keys() {
        let mut tracker = RequestTracker::<&str, i32>::default();
        tracker.advance(&"A");
        let pending = tracker.generation;
        // The next effect sees an empty row mirror after a column toggle or
        // viewport change. It advances before returning at keys.is_empty().
        assert!(!tracker.advance(&"A"));
        assert!(!tracker.claim(pending, &[1, 2]));
        assert!(tracker.requested.is_empty());
        // Reopening fetches those keys normally; cancellation never claimed
        // them and therefore cannot strand their cells in Loading.
        tracker.advance(&"A");
        assert!(tracker.claim(tracker.generation, &[1, 2]));
    }

    #[test]
    fn changing_scope_refetches_identical_rows_and_rejects_the_old_debounce() {
        let mut tracker = RequestTracker::<&str, i32>::default();
        assert!(tracker.advance(&"A"));
        assert!(tracker.claim(tracker.generation, &[1]));
        tracker.advance(&"A");
        let pending = tracker.generation;
        assert!(tracker.advance(&"B"));
        assert!(!tracker.claim(pending, &[2]));
        // No row or range change is necessary to make A's keys fetchable
        // for B: the scope-triggered advance already cleared its claims.
        let keys = visible_keys(&[1, 2], (0, 2), 0, &tracker.requested, |k| *k);
        assert_eq!(keys, vec![1, 2]);
        assert!(tracker.claim(tracker.generation, &keys));
    }

    #[test]
    fn returning_to_a_scope_rejects_its_previous_in_flight_results() {
        let mut tracker = RequestTracker::<&str, i32>::default();
        tracker.advance(&"A");
        let first_a = tracker.generation;
        assert!(tracker.claim(first_a, &[1]));
        tracker.advance(&"B");
        tracker.advance(&"A");
        let second_a = tracker.generation;
        assert!(tracker.claim(second_a, &[1]));

        let mut store = Enrichment::<i32, u8>::default();
        assert!(tracker.accepts_result(second_a));
        store.merge(&[1], vec![(1, 20)]);
        // The slow response from the first visit must not overwrite the
        // fresh response, even though both requests name the same world.
        assert!(!tracker.accepts_result(first_a));
        assert_eq!(store.get(&1), Some(&20));
    }

    #[test]
    fn scrolling_or_hiding_preserves_in_flight_results_and_cached_claims() {
        let mut tracker = RequestTracker::<&str, i32>::default();
        tracker.advance(&"A");
        let first_window = tracker.generation;
        assert!(tracker.claim(first_window, &[1, 2]));
        assert!(!tracker.advance(&"A"));
        // An already started request still contributes to the cache after
        // scrolling away or hiding its columns. Only debounces are cancelled.
        assert!(tracker.accepts_result(first_window));
        let keys = visible_keys(&[1, 2, 3], (0, 3), 0, &tracker.requested, |k| *k);
        assert_eq!(keys, vec![3]);
        assert!(tracker.claim(tracker.generation, &keys));
        tracker.advance(&"A");
        assert!(visible_keys(&[1, 2, 3], (0, 3), 0, &tracker.requested, |k| *k).is_empty());
    }

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
        assert_eq!(
            verdict(Some("Cactuar".to_string()), &started),
            Verdict::Stale
        );
        assert_eq!(verdict(None::<String>, &started), Verdict::Disposed);
    }

    #[test]
    fn failed_batches_are_distinct_from_successful_empty_history() {
        let mut store = Enrichment::<u8, u8>::default();
        store.merge_batch(&[1], Err::<Vec<(u8, u8)>, ()>(()).into());
        store.merge_batch(&[2], Ok::<Vec<(u8, u8)>, ()>(Vec::new()).into());
        store.merge_batch(&[3], vec![(3, 0)].into());
        assert_eq!(store.state(&1), Enrich::Unavailable);
        assert_eq!(store.state(&2), Enrich::Missing);
        assert_eq!(store.state(&3), Enrich::Ready(&0));
        assert_eq!(store.state(&4), Enrich::Loading);
        assert!(store.is_settled(&1));
        // Failure of one chunk does not erase successfully loaded chunks.
        store.merge_batch(&[1], vec![(1, 7)].into());
        assert_eq!(store.state(&1), Enrich::Ready(&7));
        assert_eq!(store.state(&2), Enrich::Missing);
    }

    #[test]
    fn state_tells_loading_from_missing_from_ready() {
        let mut store: SparkStore = SparkStore::default();
        let key: SparkKey = (42, true);
        assert!(store.state(&key).is_loading());
        store.merge(
            &[key, (43, false)],
            vec![(
                key,
                SparkValue {
                    points: vec![1, 2, 3],
                    delta_pct: Some(200.0),
                },
            )],
        );
        // The key that came back: Ready, with its payload.
        match store.state(&key) {
            Enrich::Ready(v) => {
                assert_eq!(v.points, vec![1, 2, 3]);
                assert_eq!(v.delta_pct, Some(200.0));
            }
            other => panic!("{other:?}"),
        }
        // Requested, nothing came back: settled without a value.
        assert_eq!(store.state(&(43, false)), Enrich::Missing);
        // Never requested: still loading.
        assert!(store.state(&(44, false)).is_loading());
        // Absorb replaces: one indivisible feed per key.
        let mut v = SparkValue {
            points: vec![1],
            delta_pct: None,
        };
        v.absorb(SparkValue {
            points: vec![9, 9],
            delta_pct: Some(0.0),
        });
        assert_eq!(v.points, vec![9, 9]);
        assert_eq!(v.delta_pct, Some(0.0));
    }
}
