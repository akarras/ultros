//! Shared JSON bodies with bounded retained bytes and service-owned miss jobs.
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

use super::error::{SharedWebError, WebError};

type ResultBody = Result<Bytes, Arc<WebError>>;
type Status = watch::Receiver<Option<ResultBody>>;

struct State<K> {
    entries: HashMap<K, (Instant, Bytes)>,
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
        let (until, value) = state.entries.get(key)?;
        (*until > Instant::now()).then(|| value.clone())
    }

    pub fn insert(&self, key: K, body: impl Into<Bytes>, ttl: Duration) {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        self.store(&mut state, key, body.into(), ttl);
    }

    fn store(&self, state: &mut State<K>, key: K, body: Bytes, ttl: Duration) {
        if let Some((_, previous)) = state.entries.remove(&key) {
            state.bytes -= previous.len();
        }
        let now = Instant::now();
        state.entries.retain(|_, (until, value)| {
            if *until <= now {
                state.bytes -= value.len();
                false
            } else {
                true
            }
        });
        if self.inner.capacity == 0 || body.len() > self.inner.max_bytes || ttl.is_zero() {
            return;
        }
        while state.entries.len() >= self.inner.capacity
            || state.bytes + body.len() > self.inner.max_bytes
        {
            let Some(victim) = state.entries.keys().next().cloned() else {
                break;
            };
            let (_, removed) = state.entries.remove(&victim).unwrap();
            state.bytes -= removed.len();
        }
        state.bytes += body.len();
        state.entries.insert(key, (now + ttl, body));
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
        let mut status = {
            let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((until, body)) = state.entries.get(&key)
                && *until > Instant::now()
            {
                return Ok(body.clone());
            }
            if let Some(status) = state.jobs.get(&key) {
                status.clone()
            } else {
                if state.jobs.len() >= self.inner.max_jobs {
                    return Err(WebError::TemporarilyUnavailable.into());
                }
                let (changed, status) = watch::channel(None);
                state.jobs.insert(key.clone(), status.clone());
                let cache = self.clone();
                tokio::spawn(async move {
                    let work = async { loader().await };
                    let result = AssertUnwindSafe(tokio::time::timeout(cache.inner.deadline, work))
                        .catch_unwind()
                        .await;
                    let result = match result {
                        Ok(Ok(Ok(body))) => Ok(Bytes::from(body)),
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
                    if let Ok(body) = &result {
                        cache.store(&mut state, key.clone(), body.clone(), ttl);
                    }
                    changed.send_replace(Some(result));
                    state.jobs.remove(&key);
                });
                status
            }
        };
        loop {
            let completed = status.borrow_and_update().clone();
            if let Some(result) = completed {
                return result.map_err(SharedWebError);
            }
            status
                .changed()
                .await
                .map_err(|_| SharedWebError::from(WebError::TemporarilyUnavailable))?;
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.state.lock().unwrap().entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            state.entries.values().map(|(_, b)| b.len()).sum::<usize>()
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
}
