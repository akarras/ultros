//! Shared JSON bodies with bounded retained bytes, stale-while-revalidate,
//! pre-compressed [`EncodedBody`] slabs, and service-owned miss jobs.
//! Cancellation of an HTTP request never cancels its coalesced loader.

use std::{
    borrow::Borrow,
    collections::HashMap,
    future::Future,
    hash::Hash,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::body::Bytes;
use futures::FutureExt;
use tokio::sync::watch;

use super::{
    encoded_body::EncodedBody,
    error::{SharedWebError, WebError},
};

/// Result of a stale-while-revalidate loader call.
///
/// Setting `cacheable: false` returns the `200 OK` payload to waiting callers
/// without populating or overwriting the in-process cache (used for degraded
/// fallbacks such as ClickHouse/sparkline soft-fails that must also emit
/// `Cache-Control: no-store`).
pub(crate) struct SwrLoad {
    pub body: Bytes,
    pub cacheable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SwrDisposition {
    Fresh,
    Loaded,
    Stale,
}

impl SwrDisposition {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Loaded => "loaded",
            Self::Stale => "stale",
        }
    }
}

#[derive(Clone)]
pub(crate) struct SwrValue {
    pub body: Arc<EncodedBody>,
    pub cacheable: bool,
    pub disposition: SwrDisposition,
}

type JobOutput = Result<(Arc<EncodedBody>, bool), Arc<WebError>>;
type Status = watch::Receiver<Option<JobOutput>>;

struct CacheEntry {
    body: Arc<EncodedBody>,
    fresh_until: Instant,
    stale_until: Instant,
}

struct State<K> {
    entries: HashMap<K, CacheEntry>,
    bytes: usize,
    jobs: HashMap<K, Status>,
}

struct Inner<K> {
    state: Mutex<State<K>>,
    capacity: usize,
    max_bytes: usize,
    max_jobs: usize,
    deadline: Duration,
}

pub(crate) struct JsonCache<K> {
    inner: Arc<Inner<K>>,
}

impl<K> Clone for JsonCache<K> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<K: Clone + Eq + Hash + Send + 'static> Default for JsonCache<K> {
    fn default() -> Self {
        Self::new(512)
    }
}

impl<K: Clone + Eq + Hash + Send + 'static> JsonCache<K> {
    pub fn new(capacity: usize) -> Self {
        Self::with_config(capacity, 32 * 1024 * 1024, 4, Duration::from_secs(12))
    }

    pub(crate) fn with_budget(capacity: usize, max_bytes: usize, max_jobs: usize) -> Self {
        Self::with_config(capacity, max_bytes, max_jobs, Duration::from_secs(12))
    }

    fn with_config(capacity: usize, max_bytes: usize, max_jobs: usize, deadline: Duration) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    entries: HashMap::new(),
                    bytes: 0,
                    jobs: HashMap::new(),
                }),
                capacity,
                max_bytes,
                max_jobs,
                deadline,
            }),
        }
    }

    pub fn get<Q: Eq + Hash + ?Sized>(&self, key: &Q) -> Option<Bytes>
    where
        K: Borrow<Q>,
    {
        let state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        let entry = state.entries.get(key)?;
        (entry.fresh_until > Instant::now()).then(|| entry.body.raw().clone())
    }

    pub fn insert(&self, key: K, body: impl Into<Bytes>, ttl: Duration) {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        self.store(&mut state, key, EncodedBody::new(body), ttl, ttl);
    }

    fn store(
        &self,
        state: &mut State<K>,
        key: K,
        body: Arc<EncodedBody>,
        fresh_ttl: Duration,
        stale_ttl: Duration,
    ) {
        if let Some(previous) = state.entries.remove(&key) {
            state.bytes -= previous.body.raw_len();
        }
        let now = Instant::now();
        state.entries.retain(|_, entry| {
            if entry.stale_until <= now {
                state.bytes -= entry.body.raw_len();
                false
            } else {
                true
            }
        });
        let stale_ttl = stale_ttl.max(fresh_ttl);
        if self.inner.capacity == 0 || body.raw_len() > self.inner.max_bytes || stale_ttl.is_zero()
        {
            return;
        }
        while state.entries.len() >= self.inner.capacity
            || state.bytes + body.raw_len() > self.inner.max_bytes
        {
            let Some(victim) = state.entries.keys().next().cloned() else {
                break;
            };
            let removed = state.entries.remove(&victim).unwrap();
            state.bytes -= removed.body.raw_len();
        }
        state.bytes += body.raw_len();
        state.entries.insert(
            key,
            CacheEntry {
                body,
                fresh_until: now + fresh_ttl,
                stale_until: now + stale_ttl,
            },
        );
    }

    fn spawn_job<F, Fut>(
        &self,
        state: &mut State<K>,
        key: K,
        fresh_ttl: Duration,
        stale_ttl: Duration,
        loader: F,
    ) -> Option<Status>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<SwrLoad, WebError>> + Send + 'static,
    {
        if state.jobs.len() >= self.inner.max_jobs {
            return None;
        }
        let (changed, status) = watch::channel(None);
        state.jobs.insert(key.clone(), status.clone());
        let cache = self.clone();
        tokio::spawn(async move {
            let work = async { loader().await };
            let result = AssertUnwindSafe(tokio::time::timeout(cache.inner.deadline, work))
                .catch_unwind()
                .await;
            let result: JobOutput = match result {
                Ok(Ok(Ok(loaded))) => Ok((EncodedBody::new(loaded.body), loaded.cacheable)),
                Ok(Ok(Err(error))) => Err(Arc::new(error)),
                Ok(Err(_)) => {
                    tracing::warn!("cached JSON loader timed out");
                    Err(Arc::new(WebError::TemporarilyUnavailable))
                }
                Err(_) => {
                    tracing::error!("cached JSON loader panicked");
                    Err(Arc::new(WebError::TemporarilyUnavailable))
                }
            };
            let mut state = cache.inner.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok((body, true)) = &result {
                cache.store(&mut state, key.clone(), body.clone(), fresh_ttl, stale_ttl);
            }
            changed.send_replace(Some(result));
            state.jobs.remove(&key);
        });
        Some(status)
    }

    /// Return a fresh value, serve stale immediately while triggering at most
    /// one coalesced background refresh, or coalesce callers behind one cold
    /// load.
    pub async fn get_or_load_swr<F, Fut>(
        &self,
        key: K,
        fresh_ttl: Duration,
        stale_ttl: Duration,
        loader: F,
    ) -> Result<SwrValue, SharedWebError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<SwrLoad, WebError>> + Send + 'static,
    {
        let mut status = {
            let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            if let Some(entry) = state.entries.get(&key) {
                if entry.fresh_until > now {
                    return Ok(SwrValue {
                        body: entry.body.clone(),
                        cacheable: true,
                        disposition: SwrDisposition::Fresh,
                    });
                }
                if entry.stale_until > now {
                    let body = entry.body.clone();
                    if !state.jobs.contains_key(&key) {
                        let _ = self.spawn_job(&mut state, key, fresh_ttl, stale_ttl, loader);
                    }
                    return Ok(SwrValue {
                        body,
                        cacheable: true,
                        disposition: SwrDisposition::Stale,
                    });
                }
            }
            if let Some(status) = state.jobs.get(&key) {
                status.clone()
            } else {
                let Some(status) = self.spawn_job(&mut state, key, fresh_ttl, stale_ttl, loader)
                else {
                    return Err(WebError::TemporarilyUnavailable.into());
                };
                status
            }
        };
        loop {
            let completed = status.borrow_and_update().clone();
            if let Some(result) = completed {
                return result
                    .map(|(body, cacheable)| SwrValue {
                        body,
                        cacheable,
                        disposition: SwrDisposition::Loaded,
                    })
                    .map_err(SharedWebError);
            }
            status
                .changed()
                .await
                .map_err(|_| SharedWebError::from(WebError::TemporarilyUnavailable))?;
        }
    }

    pub async fn get_or_load<F, Fut>(
        &self,
        key: K,
        ttl: Duration,
        loader: F,
    ) -> Result<Bytes, SharedWebError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<String, WebError>> + Send + 'static,
    {
        let value = self
            .get_or_load_swr(key, ttl, ttl, move || async move {
                let body = loader().await?;
                Ok(SwrLoad {
                    body: Bytes::from(body),
                    cacheable: true,
                })
            })
            .await?;
        Ok(value.body.raw().clone())
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.state.lock().unwrap().entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn byte_budget_covers_replacement_expiry_and_oversized_values() {
        let cache = JsonCache::with_config(4, 6, 1, Duration::from_secs(1));
        let ttl = Duration::from_secs(60);
        cache.insert(1, "abcd", ttl);
        let shared = cache.get(&1).unwrap();
        assert_eq!(shared.as_ptr(), cache.get(&1).unwrap().as_ptr());
        cache.insert(1, "ab", ttl);
        cache.insert(2, "cdef", ttl);
        assert_eq!(cache.inner.state.lock().unwrap().bytes, 6);
        cache.insert(3, "xyz", ttl);
        assert!(cache.inner.state.lock().unwrap().bytes <= 6);
        cache.insert(3, "oversized", ttl);
        assert!(cache.get(&3).is_none());
        cache.insert(4, "abc", Duration::ZERO);
        assert!(cache.get(&4).is_none());
        let state = cache.inner.state.lock().unwrap();
        assert_eq!(
            state.bytes,
            state
                .entries
                .values()
                .map(|e| e.body.raw_len())
                .sum::<usize>()
        );
    }

    #[tokio::test]
    async fn cancelled_leader_keeps_one_loader_and_bounded_job_capacity() {
        let cache = JsonCache::with_config(4, 100, 1, Duration::from_secs(1));
        let (started, start) = tokio::sync::oneshot::channel();
        let (finish, finished) = tokio::sync::oneshot::channel();
        let clone = cache.clone();
        let leader = tokio::spawn(async move {
            clone
                .get_or_load(1, Duration::from_secs(60), || async move {
                    started.send(()).unwrap();
                    finished.await.unwrap();
                    Ok("answer".into())
                })
                .await
        });
        start.await.unwrap();
        let follower =
            cache.get_or_load(1, Duration::from_secs(60), || async { panic!("duplicate") });
        tokio::pin!(follower);
        assert!(futures::poll!(&mut follower).is_pending());
        leader.abort();
        assert!(leader.await.unwrap_err().is_cancelled());
        assert!(
            cache
                .get_or_load(2, Duration::from_secs(1), || async { panic!("overload") })
                .await
                .is_err()
        );
        finish.send(()).unwrap();
        assert_eq!(follower.await.unwrap(), "answer");
        assert_eq!(
            cache
                .get_or_load(1, Duration::from_secs(60), || async {
                    panic!("cache miss")
                })
                .await
                .unwrap(),
            "answer"
        );
    }

    #[tokio::test]
    async fn failed_timed_out_and_panicking_loaders_can_retry() {
        let cache = JsonCache::with_config(4, 100, 1, Duration::from_millis(10));
        for mode in 0..3 {
            assert!(
                cache
                    .get_or_load(1, Duration::ZERO, move || async move {
                        match mode {
                            0 => Err(WebError::BadRequest),
                            1 => std::future::pending().await,
                            _ => panic!("loader panic"),
                        }
                    })
                    .await
                    .is_err()
            );
            assert_eq!(
                cache
                    .get_or_load(1, Duration::ZERO, || async { Ok("retry".into()) })
                    .await
                    .unwrap(),
                "retry"
            );
        }
        let result = cache
            .get_or_load(
                1,
                Duration::ZERO,
                || -> std::future::Ready<Result<String, WebError>> {
                    panic!("loader construction panic")
                },
            )
            .await;
        assert!(result.is_err());
        assert_eq!(
            cache
                .get_or_load(1, Duration::ZERO, || async { Ok("retry".into()) })
                .await
                .unwrap(),
            "retry"
        );
    }

    #[tokio::test]
    async fn swr_serves_stale_immediately_and_refreshes_once_in_background() {
        let cache = JsonCache::with_config(4, 1024, 2, Duration::from_secs(1));
        // Seed entry with fresh_ttl = 0 so it is immediately stale, stale_ttl = 60s.
        let loaded = cache
            .get_or_load_swr(1, Duration::ZERO, Duration::from_secs(60), || async {
                Ok(SwrLoad {
                    body: Bytes::from_static(b"v1"),
                    cacheable: true,
                })
            })
            .await
            .unwrap();
        assert_eq!(loaded.disposition, SwrDisposition::Loaded);
        assert_eq!(loaded.body.raw().as_ref(), b"v1");

        let calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();

        // First stale hit triggers background refresh and returns v1 immediately.
        let c1 = calls.clone();
        let stale1 = cache
            .get_or_load_swr(
                1,
                Duration::from_secs(60),
                Duration::from_secs(120),
                move || async move {
                    c1.fetch_add(1, Ordering::SeqCst);
                    let _ = started_tx.send(());
                    let _ = finish_rx.await;
                    Ok(SwrLoad {
                        body: Bytes::from_static(b"v2"),
                        cacheable: true,
                    })
                },
            )
            .await
            .unwrap();
        assert_eq!(stale1.disposition, SwrDisposition::Stale);
        assert_eq!(stale1.body.raw().as_ref(), b"v1");

        started_rx.await.unwrap();

        // Second stale hit while refresh is in flight also returns v1 immediately
        // without spawning a duplicate refresh.
        let c2 = calls.clone();
        let stale2 = cache
            .get_or_load_swr(
                1,
                Duration::from_secs(60),
                Duration::from_secs(120),
                move || async move {
                    c2.fetch_add(1, Ordering::SeqCst);
                    Ok(SwrLoad {
                        body: Bytes::from_static(b"duplicate"),
                        cacheable: true,
                    })
                },
            )
            .await
            .unwrap();
        assert_eq!(stale2.disposition, SwrDisposition::Stale);
        assert_eq!(stale2.body.raw().as_ref(), b"v1");

        // Complete the background refresh; subsequent hit gets fresh v2.
        let _ = finish_tx.send(());
        for _ in 0..20 {
            if cache.get(&1).as_deref() == Some(b"v2".as_slice()) {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let fresh = cache
            .get_or_load_swr(
                1,
                Duration::from_secs(60),
                Duration::from_secs(120),
                || async { panic!("should hit fresh v2") },
            )
            .await
            .unwrap();
        assert_eq!(fresh.disposition, SwrDisposition::Fresh);
        assert_eq!(fresh.body.raw().as_ref(), b"v2");
    }

    #[tokio::test]
    async fn swr_degraded_load_never_populates_or_poisons_cache() {
        let cache = JsonCache::with_config(4, 1024, 2, Duration::from_secs(1));

        // Cold degraded load returns body with cacheable = false and skips cache.
        let cold = cache
            .get_or_load_swr(
                1,
                Duration::from_secs(60),
                Duration::from_secs(120),
                || async {
                    Ok(SwrLoad {
                        body: Bytes::from_static(b"degraded"),
                        cacheable: false,
                    })
                },
            )
            .await
            .unwrap();
        assert!(!cold.cacheable);
        assert_eq!(cold.body.raw().as_ref(), b"degraded");
        assert!(cache.get(&1).is_none());

        // Populate healthy stale entry, then have background refresh degrade:
        // healthy stale entry must remain intact!
        cache
            .get_or_load_swr(2, Duration::ZERO, Duration::from_secs(60), || async {
                Ok(SwrLoad {
                    body: Bytes::from_static(b"healthy"),
                    cacheable: true,
                })
            })
            .await
            .unwrap();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let stale = cache
            .get_or_load_swr(
                2,
                Duration::from_secs(60),
                Duration::from_secs(120),
                || async move {
                    let _ = done_tx.send(());
                    Ok(SwrLoad {
                        body: Bytes::from_static(b"degraded-refresh"),
                        cacheable: false,
                    })
                },
            )
            .await
            .unwrap();
        assert!(stale.cacheable);
        assert_eq!(stale.body.raw().as_ref(), b"healthy");
        done_rx.await.unwrap();
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        // Entry is still "healthy", never overwritten with "degraded-refresh".
        let after = cache
            .get_or_load_swr(
                2,
                Duration::from_secs(60),
                Duration::from_secs(120),
                || async {
                    Ok(SwrLoad {
                        body: Bytes::from_static(b"v2"),
                        cacheable: true,
                    })
                },
            )
            .await
            .unwrap();
        assert_eq!(after.body.raw().as_ref(), b"healthy");
    }
}
