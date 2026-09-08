//! Bounded stale-while-revalidate cache for bulk market-stat responses —
//! sale stats and listing stats, one instance each.
//!
//! Each value is serialized JSON because serialization is material for a
//! whole-market payload. Per-key slots coalesce cold misses, while stale
//! entries return immediately and refresh once in the background. A shared
//! semaphore and timeout keep cold traffic from turning an analytical store
//! slowdown into process-wide resource exhaustion.

use std::{
    collections::HashMap,
    future::Future,
    marker::PhantomData,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{body::Bytes, response::IntoResponse};
use tokio::sync::{Mutex as AsyncMutex, Notify, Semaphore};
use ultros_db::world_data::world_cache::AnySelector;

use super::error::{ClickHouseQueryError, WebError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CacheKey {
    pub selector: AnySelector,
    pub window_days: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheDisposition {
    Fresh,
    Loaded,
    Stale,
}

impl CacheDisposition {
    /// The `x-ultros-cache` header value and the metric label.
    pub fn as_str(self) -> &'static str {
        match self {
            CacheDisposition::Fresh => "fresh",
            CacheDisposition::Loaded => "loaded",
            CacheDisposition::Stale => "stale",
        }
    }
}

pub(crate) struct CacheValue {
    pub body: Bytes,
    pub disposition: CacheDisposition,
}

/// The response every cached bulk-stat endpoint returns: JSON, shared-cacheable
/// for as long as a value is fresh here, `stale-while-revalidate` matching the
/// stale window, and the disposition exposed for debugging.
pub(crate) fn cached_response(body: Bytes, disposition: &'static str) -> axum::response::Response {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/json".to_string(),
            ),
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=300, s-maxage=300, stale-while-revalidate=1800".to_string(),
            ),
            (
                axum::http::header::HeaderName::from_static("x-ultros-cache"),
                disposition.to_string(),
            ),
        ],
        body,
    )
        .into_response()
}

#[derive(Default)]
struct SlotState {
    body: Option<Bytes>,
    fresh_until: Option<Instant>,
    stale_until: Option<Instant>,
    failed_until: Option<Instant>,
    refreshing: bool,
}

#[derive(Default)]
struct Slot {
    state: AsyncMutex<SlotState>,
    changed: Notify,
    cached: AtomicBool,
    body_bytes: AtomicUsize,
    last_access: AtomicU64,
}

struct Inner {
    slots: Mutex<HashMap<CacheKey, Arc<Slot>>>,
    capacity: usize,
    max_bytes: usize,
    retained_bytes: AtomicUsize,
    access_clock: AtomicU64,
    fresh_ttl: Duration,
    stale_ttl: Duration,
    query_timeout: Duration,
    query_limit: Arc<Semaphore>,
}

/// What a cache instance holds — only for the labels that tell two instances'
/// failures apart. The cache itself is body-agnostic.
pub(crate) trait CacheKind: 'static {
    /// The `ultros_clickhouse::queries` function the loader runs, so a
    /// timeout groups with that query's own failures in error reporting.
    const QUERY: &'static str;
    /// Short label for log lines.
    const LABEL: &'static str;
    /// Hard budget for retained response bodies, sized from this kind's own
    /// payload — the two families are an order of magnitude apart.
    const MAX_BYTES: usize;
}

/// `/api/v1/sale_stats` bodies.
pub(crate) struct SaleStatsKind;

impl CacheKind for SaleStatsKind {
    const QUERY: &'static str = "bulk_sale_stats";
    const LABEL: &'static str = "sale-stats";
    /// Measured in the roadmap's §1.4: 249 KB for a world, 481 KB for a
    /// datacenter, 578 KB for a region. 64 MiB holds every scope Ultros has
    /// several times over.
    const MAX_BYTES: usize = 64 * 1024 * 1024;
}

/// `/api/v1/listing_stats` bodies.
pub(crate) struct ListingStatsKind;

impl CacheKind for ListingStatsKind {
    const QUERY: &'static str = "bulk_listing_alive";
    const LABEL: &'static str = "listing-stats";
    /// Sized from the alive set, not copied from sale stats — this family is
    /// much heavier. Prod carries ≈11.4M alive listings over ~90 worlds
    /// (≈127k per world), aggregated to one row per `(item, hq)` with stock:
    /// on the order of 9k rows for a world, the same magnitude as
    /// `sale_stats_window`'s per-world row count. An `ItemListingStats` is
    /// eight numeric fields and serializes to ≈190 bytes, so a world body is
    /// ≈1.7 MB, a datacenter (the union over ~8 worlds, ≈25k rows) ≈4.8 MB,
    /// and a region ≈7 MB — roughly 7× a sale-stats body of the same scope.
    ///
    /// At sale stats' 64 MiB that is only ~39 world bodies, fewer than Ultros
    /// has worlds, so the budget would be evicting hot scopes continuously.
    /// 192 MiB holds every datacenter and region body plus ~50 world bodies.
    /// The other thing `max_bytes` gates is `finish_refresh` refusing to cache
    /// a single body larger than it — which would put that scope on a 2-second
    /// failure backoff, re-running the query on a 2-second floor forever. A
    /// region body is ~27× under this ceiling, so that path stays unreachable
    /// even if the market grows several-fold.
    const MAX_BYTES: usize = 192 * 1024 * 1024;
}

/// One cache per [`CacheKind`]: distinct types, so each is its own axum
/// `State` and a listing snapshot never competes with a sale snapshot for
/// slots or bytes.
pub(crate) struct StatsCache<K: CacheKind> {
    inner: Arc<Inner>,
    kind: PhantomData<fn() -> K>,
}

pub(crate) type SaleStatsCache = StatsCache<SaleStatsKind>;
pub(crate) type ListingStatsCache = StatsCache<ListingStatsKind>;

impl<K: CacheKind> Clone for StatsCache<K> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            kind: PhantomData,
        }
    }
}

impl<K: CacheKind> StatsCache<K> {
    pub fn new(capacity: usize, max_concurrent_queries: usize) -> Self {
        Self::with_config(
            capacity,
            max_concurrent_queries,
            Duration::from_secs(5 * 60),
            Duration::from_secs(30 * 60),
            Duration::from_secs(12),
            K::MAX_BYTES,
        )
    }

    fn with_config(
        capacity: usize,
        max_concurrent_queries: usize,
        fresh_ttl: Duration,
        stale_ttl: Duration,
        query_timeout: Duration,
        max_bytes: usize,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                slots: Mutex::new(HashMap::new()),
                capacity: capacity.max(1),
                max_bytes: max_bytes.max(1),
                retained_bytes: AtomicUsize::new(0),
                access_clock: AtomicU64::new(1),
                fresh_ttl,
                stale_ttl: stale_ttl.max(fresh_ttl),
                query_timeout,
                query_limit: Arc::new(Semaphore::new(max_concurrent_queries.max(1))),
            }),
            kind: PhantomData,
        }
    }

    fn slot(&self, key: CacheKey) -> Arc<Slot> {
        let Ok(mut slots) = self.inner.slots.lock() else {
            return Arc::new(Slot::default());
        };
        if let Some(slot) = slots.get(&key) {
            slot.last_access.store(
                self.inner.access_clock.fetch_add(1, Ordering::Relaxed),
                Ordering::Relaxed,
            );
            return slot.clone();
        }
        while slots.len() >= self.inner.capacity {
            let Some(victim) = slots.keys().next().copied() else {
                break;
            };
            if let Some(slot) = slots.remove(&victim) {
                slot.cached.store(false, Ordering::Release);
                self.subtract_retained(slot.body_bytes.swap(0, Ordering::AcqRel));
            }
        }
        let slot = Arc::new(Slot {
            cached: AtomicBool::new(true),
            last_access: AtomicU64::new(self.inner.access_clock.fetch_add(1, Ordering::Relaxed)),
            ..Default::default()
        });
        slots.insert(key, slot.clone());
        slot
    }

    fn subtract_retained(&self, bytes: usize) {
        let _ = self.inner.retained_bytes.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |current| Some(current.saturating_sub(bytes)),
        );
    }

    /// Evict least-recently-used slots until cached response bodies are back
    /// under the hard per-process byte budget. `Bytes` lets active responses
    /// share the allocation while their cache entry is being evicted.
    fn prune_to_byte_budget(&self, protected: &Arc<Slot>) {
        if self.inner.retained_bytes.load(Ordering::Acquire) <= self.inner.max_bytes {
            return;
        }
        let Ok(mut slots) = self.inner.slots.lock() else {
            return;
        };
        let mut victims = slots
            .iter()
            .filter(|(_, slot)| !Arc::ptr_eq(slot, protected))
            .map(|(key, slot)| {
                (
                    *key,
                    slot.last_access.load(Ordering::Relaxed),
                    slot.body_bytes.load(Ordering::Acquire),
                )
            })
            .filter(|(_, _, bytes)| *bytes > 0)
            .collect::<Vec<_>>();
        victims.sort_unstable_by_key(|(_, access, _)| *access);
        for (key, _, _) in victims {
            if self.inner.retained_bytes.load(Ordering::Acquire) <= self.inner.max_bytes {
                break;
            }
            if let Some(slot) = slots.remove(&key) {
                slot.cached.store(false, Ordering::Release);
                self.subtract_retained(slot.body_bytes.swap(0, Ordering::AcqRel));
            }
        }
    }

    /// Return a fresh value, serve stale and refresh in the background, or
    /// coalesce callers behind one cold load.
    pub async fn get_or_load<F, Fut>(
        &self,
        key: CacheKey,
        loader: F,
    ) -> Result<CacheValue, WebError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<Bytes, WebError>> + Send + 'static,
    {
        let slot = self.slot(key);
        let mut loader = Some(loader);

        loop {
            let mut state = slot.state.lock().await;
            let now = Instant::now();
            if state.fresh_until.is_some_and(|until| until > now)
                && let Some(body) = &state.body
            {
                return Ok(CacheValue {
                    body: body.clone(),
                    disposition: CacheDisposition::Fresh,
                });
            }

            if state.stale_until.is_some_and(|until| until > now)
                && let Some(body) = state.body.clone()
            {
                if !state.refreshing {
                    state.refreshing = true;
                    drop(state);
                    self.spawn_refresh(slot.clone(), loader.take().expect("loader used once"));
                }
                return Ok(CacheValue {
                    body,
                    disposition: CacheDisposition::Stale,
                });
            }

            // A failed cold load wakes every coalesced follower. Briefly
            // back those followers off instead of letting each one launch the
            // same doomed analytical query in sequence.
            if state.failed_until.is_some_and(|until| until > now) {
                return Err(WebError::TemporarilyUnavailable);
            }

            if state.refreshing {
                let changed = slot.changed.notified();
                drop(state);
                changed.await;
                continue;
            }

            state.refreshing = true;
            drop(state);
            let result = self
                .run_loader(loader.take().expect("loader used once"))
                .await;
            self.finish_refresh(&slot, result.as_ref().ok()).await;
            return result.map(|body| CacheValue {
                body,
                disposition: CacheDisposition::Loaded,
            });
        }
    }

    fn spawn_refresh<F, Fut>(&self, slot: Arc<Slot>, loader: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<Bytes, WebError>> + Send + 'static,
    {
        let cache = self.clone();
        tokio::spawn(async move {
            let result = cache.run_loader(loader).await;
            if let Err(error) = &result {
                tracing::warn!(
                    ?error,
                    cache = K::LABEL,
                    "stats background refresh failed; serving stale"
                );
            }
            cache.finish_refresh(&slot, result.as_ref().ok()).await;
        });
    }

    async fn run_loader<F, Fut>(&self, loader: F) -> Result<Bytes, WebError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Bytes, WebError>>,
    {
        let guarded = async {
            let _permit = self
                .inner
                .query_limit
                .acquire()
                .await
                .map_err(|_| anyhow::anyhow!("{} query limiter closed", K::LABEL))?;
            loader().await
        };
        match tokio::time::timeout(self.inner.query_timeout, guarded).await {
            Ok(result) => result,
            Err(_) => Err(ClickHouseQueryError::new(
                K::QUERY,
                ultros_clickhouse::ClickHouseError::Client(clickhouse::error::Error::TimedOut),
            )
            .into()),
        }
    }

    async fn finish_refresh(&self, slot: &Arc<Slot>, body: Option<&Bytes>) {
        let mut state = slot.state.lock().await;
        state.refreshing = false;
        if let Some(body) = body
            && body.len() <= self.inner.max_bytes
            && slot.cached.load(Ordering::Acquire)
        {
            let now = Instant::now();
            state.body = Some(body.clone());
            state.fresh_until = Some(now + self.inner.fresh_ttl);
            state.stale_until = Some(now + self.inner.stale_ttl);
            state.failed_until = None;
            let old_len = slot.body_bytes.swap(body.len(), Ordering::AcqRel);
            if body.len() >= old_len {
                self.inner
                    .retained_bytes
                    .fetch_add(body.len() - old_len, Ordering::AcqRel);
            } else {
                self.subtract_retained(old_len - body.len());
            }
        } else {
            state.failed_until = Some(Instant::now() + Duration::from_secs(2));
        }
        drop(state);
        slot.changed.notify_waiters();
        self.prune_to_byte_budget(slot);
    }
}

impl<K: CacheKind> Default for StatsCache<K> {
    fn default() -> Self {
        Self::new(512, 2)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn key(id: i32) -> CacheKey {
        CacheKey {
            selector: AnySelector::World(id),
            window_days: 7,
        }
    }

    #[tokio::test]
    async fn fresh_hits_do_not_reload() {
        let cache = SaleStatsCache::with_config(
            4,
            1,
            Duration::from_secs(60),
            Duration::from_secs(120),
            Duration::from_secs(1),
            64 * 1024 * 1024,
        );
        let calls = Arc::new(AtomicUsize::new(0));
        for expected in [CacheDisposition::Loaded, CacheDisposition::Fresh] {
            let calls = calls.clone();
            let value = cache
                .get_or_load(key(1), move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(Bytes::from_static(b"body"))
                })
                .await
                .unwrap();
            assert_eq!(value.disposition, expected);
            assert_eq!(value.body, "body");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cold_concurrent_misses_are_coalesced() {
        let cache = SaleStatsCache::with_config(
            4,
            1,
            Duration::from_secs(60),
            Duration::from_secs(120),
            Duration::from_secs(1),
            64 * 1024 * 1024,
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let load = |cache: SaleStatsCache, calls: Arc<AtomicUsize>| async move {
            cache
                .get_or_load(key(1), move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::task::yield_now().await;
                    Ok(Bytes::from_static(b"body"))
                })
                .await
                .unwrap()
                .body
        };
        let (a, b) = tokio::join!(
            load(cache.clone(), calls.clone()),
            load(cache, calls.clone())
        );
        assert_eq!(a, "body");
        assert_eq!(b, "body");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn stale_values_survive_a_failed_refresh() {
        let cache = SaleStatsCache::with_config(
            4,
            1,
            Duration::ZERO,
            Duration::from_secs(60),
            Duration::from_secs(1),
            64 * 1024 * 1024,
        );
        cache
            .get_or_load(key(1), || async { Ok(Bytes::from_static(b"body")) })
            .await
            .unwrap();
        let value = cache
            .get_or_load(key(1), || async { Err(WebError::BadRequest) })
            .await
            .unwrap();
        assert_eq!(value.disposition, CacheDisposition::Stale);
        assert_eq!(value.body, "body");
    }

    #[tokio::test]
    async fn failed_cold_loads_are_briefly_backed_off() {
        let cache = SaleStatsCache::with_config(
            4,
            1,
            Duration::from_secs(60),
            Duration::from_secs(120),
            Duration::from_secs(1),
            64 * 1024 * 1024,
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let first_calls = calls.clone();
        let first = cache
            .get_or_load(key(1), move || async move {
                first_calls.fetch_add(1, Ordering::SeqCst);
                Err(WebError::BadRequest)
            })
            .await;
        assert!(matches!(first, Err(WebError::BadRequest)));

        let second_calls = calls.clone();
        let second = cache
            .get_or_load(key(1), move || async move {
                second_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Bytes::from_static(b"unexpected"))
            })
            .await;
        assert!(matches!(second, Err(WebError::TemporarilyUnavailable)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn response_bodies_stay_under_the_byte_budget() {
        let cache = SaleStatsCache::with_config(
            4,
            1,
            Duration::from_secs(60),
            Duration::from_secs(120),
            Duration::from_secs(1),
            6,
        );
        for id in [1, 2] {
            cache
                .get_or_load(key(id), || async { Ok(Bytes::from_static(b"body")) })
                .await
                .unwrap();
        }
        assert!(cache.inner.retained_bytes.load(Ordering::Acquire) <= 6);
        assert_eq!(cache.inner.slots.lock().unwrap().len(), 1);
    }
}
