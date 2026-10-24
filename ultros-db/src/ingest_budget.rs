//! All market writers share a connection budget, including metadata lookups.
//! Each admitted operation issues one query at a time and holds at most one
//! transaction, leaving at least half the pool available to ordinary requests.

use std::{sync::Arc, time::Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone, Debug)]
pub(crate) struct IngestBudget(Arc<Semaphore>);

impl IngestBudget {
    pub fn new(pool_max: u32) -> Self {
        // A one-connection pool cannot reserve headroom. Larger pools reserve
        // at least half; a high operator ceiling never means unlimited ingest.
        Self(Arc::new(Semaphore::new(
            (pool_max / 2).clamp(1, 16) as usize
        )))
    }

    pub async fn acquire(&self) -> anyhow::Result<OwnedSemaphorePermit> {
        let start = Instant::now();
        let permit = self.0.clone().acquire_owned().await?;
        metrics::histogram!("ultros_ingest_database_budget_wait_seconds").record(start.elapsed());
        Ok(permit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cloned_budgets_share_capacity_and_cancelled_waits_do_not_take_it() {
        let budget = IngestBudget::new(4);
        let clone = budget.clone();
        let first = budget.acquire().await.unwrap();
        let second = clone.acquire().await.unwrap();
        let waiting = budget.acquire();
        tokio::pin!(waiting);
        assert!(futures::poll!(&mut waiting).is_pending());
        let mut cancelled = Box::pin(clone.acquire());
        assert!(futures::poll!(&mut cancelled).is_pending());
        drop(cancelled);
        drop(first);
        let third = tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(budget.0.available_permits(), 0);
        drop((second, third));
        assert_eq!(budget.0.available_permits(), 2);
        assert_eq!(IngestBudget::new(1).0.available_permits(), 1);
        assert_eq!(IngestBudget::new(50).0.available_permits(), 16);
    }
}
