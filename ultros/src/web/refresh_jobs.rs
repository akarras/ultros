//! Bounded, coalesced manual refreshes. A job belongs to this service, so a
//! cancelled HTTP request cannot abandon its followers or release its budget.

use std::{
    collections::HashMap,
    future::Future,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::FutureExt;
use tokio::sync::{Semaphore, watch};
use ultros_db::world_data::world_cache::AnySelector;

use super::error::WebError;

type Key = (AnySelector, i32);
type Status = watch::Receiver<Option<bool>>;

#[derive(Clone)]
pub(crate) struct RefreshJobs {
    jobs: Arc<Mutex<HashMap<Key, Status>>>,
    permits: Arc<Semaphore>,
    deadline: Duration,
}

impl Default for RefreshJobs {
    fn default() -> Self {
        Self::new(2, Duration::from_secs(30))
    }
}

impl RefreshJobs {
    fn new(limit: usize, deadline: Duration) -> Self {
        Self {
            jobs: Arc::default(),
            permits: Arc::new(Semaphore::new(limit.max(1))),
            deadline,
        }
    }

    pub async fn run<F, Fut>(&self, key: Key, loader: F) -> Result<(), WebError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let mut status = {
            let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(status) = jobs.get(&key) {
                status.clone()
            } else {
                // Acquire before spawning: the task set itself is bounded,
                // including when every upstream connection is stalled.
                let permit = self
                    .permits
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| WebError::TemporarilyUnavailable)?;
                let (changed, status) = watch::channel(None);
                jobs.insert(key, status.clone());
                let service = self.clone();
                tokio::spawn(async move {
                    // Invoke the closure inside the caught future as well.
                    let work = async { loader().await };
                    let result = AssertUnwindSafe(tokio::time::timeout(service.deadline, work))
                        .catch_unwind()
                        .await;
                    let success = match result {
                        Ok(Ok(Ok(()))) => true,
                        Ok(Ok(Err(error))) => {
                            tracing::error!(?error, "manual market refresh failed");
                            false
                        }
                        Ok(Err(_)) => {
                            tracing::warn!("manual market refresh timed out");
                            false
                        }
                        Err(_) => {
                            tracing::error!("manual market refresh panicked");
                            false
                        }
                    };
                    let mut jobs = service.jobs.lock().unwrap_or_else(|e| e.into_inner());
                    changed.send_replace(Some(success));
                    drop(permit);
                    jobs.remove(&key);
                });
                status
            }
        };
        loop {
            let completed = *status.borrow_and_update();
            if let Some(success) = completed {
                return if success {
                    Ok(())
                } else {
                    Err(WebError::TemporarilyUnavailable)
                };
            }
            status
                .changed()
                .await
                .map_err(|_| WebError::TemporarilyUnavailable)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(item: i32) -> Key {
        (AnySelector::World(1), item)
    }

    #[tokio::test]
    async fn cancelled_requests_leave_the_coalesced_job_and_budget_intact() {
        let jobs = RefreshJobs::new(1, Duration::from_secs(1));
        let (started, start) = tokio::sync::oneshot::channel();
        let (finish, finished) = tokio::sync::oneshot::channel();
        let service = jobs.clone();
        let leader = tokio::spawn(async move {
            service
                .run(key(1), || async move {
                    started.send(()).unwrap();
                    finished.await.unwrap();
                    Ok(())
                })
                .await
        });
        start.await.unwrap();
        let follower = jobs.run(key(1), || async { panic!("duplicate job") });
        tokio::pin!(follower);
        assert!(futures::poll!(&mut follower).is_pending());
        leader.abort();
        assert!(matches!(leader.await, Err(error) if error.is_cancelled()));
        assert!(matches!(
            jobs.run(key(2), || async { panic!("budget exceeded") })
                .await,
            Err(WebError::TemporarilyUnavailable)
        ));
        finish.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), follower)
            .await
            .unwrap()
            .unwrap();
        jobs.run(key(2), || async { Ok(()) }).await.unwrap();
    }

    #[tokio::test]
    async fn errors_timeouts_and_panics_release_the_job_for_retry() {
        let jobs = RefreshJobs::new(1, Duration::from_millis(10));
        for failure in [0, 1] {
            let result = jobs
                .run(key(1), move || async move {
                    if failure == 0 {
                        anyhow::bail!("refresh failed");
                    }
                    std::future::pending::<anyhow::Result<()>>().await
                })
                .await;
            assert!(matches!(result, Err(WebError::TemporarilyUnavailable)));
            jobs.run(key(1), || async { Ok(()) }).await.unwrap();
        }
        let result = jobs
            .run(key(1), || -> std::future::Ready<anyhow::Result<()>> {
                panic!("loader construction failed")
            })
            .await;
        assert!(matches!(result, Err(WebError::TemporarilyUnavailable)));
        jobs.run(key(1), || async { Ok(()) }).await.unwrap();
    }
}
