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
            requested
                .with_value(|seen| visible_keys(data, range, cfg.prefetch_margin, seen, key_of))
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
}
