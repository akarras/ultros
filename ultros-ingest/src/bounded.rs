//! Bound work before spawning it, leaving excess events in the producer's
//! bounded channel. A semaphore acquired inside a spawned task only moves the
//! unbounded queue into tasks waiting for permits.

use std::{future::Future, num::NonZeroUsize};

use tokio::{sync::mpsc::Receiver, task::JoinSet};
use tokio_util::sync::CancellationToken;

/// Stop dequeuing on cancellation or channel closure, then finish every
/// accepted event. Reap completions as we go so even finished tasks cannot
/// accumulate in the set. The caller must keep downstream consumers alive
/// until this drain completes.
pub async fn run<T, F, Fut>(
    receiver: &mut Receiver<T>,
    max_in_flight: NonZeroUsize,
    shutdown: CancellationToken,
    mut process: F,
) where
    F: FnMut(T) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let mut tasks = JoinSet::new();
    loop {
        metrics::gauge!("ultros_websocket_in_flight").set(tasks.len() as f64);
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            result = tasks.join_next(), if !tasks.is_empty() => {
                report_completion(result);
            }
            message = receiver.recv(), if tasks.len() < max_in_flight.get() => {
                let Some(message) = message else { break };
                tasks.spawn(process(message));
            }
        }
    }
    while let Some(result) = tasks.join_next().await {
        metrics::gauge!("ultros_websocket_in_flight").set(tasks.len() as f64);
        report_completion(Some(result));
    }
    metrics::gauge!("ultros_websocket_in_flight").set(0.0);
}

fn report_completion(result: Option<Result<(), tokio::task::JoinError>>) {
    if let Some(Err(error)) = result {
        tracing::error!(?error, "websocket ingest task failed");
        metrics::counter!("ultros_websocket_task_failures_total").increment(1);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use tokio::sync::{Semaphore, mpsc};

    use super::*;

    #[tokio::test]
    async fn saturation_backpressures_the_producer_and_shutdown_drains_accepted_work() {
        let (tx, mut rx) = mpsc::channel(2);
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let completed = Arc::new(AtomicUsize::new(0));
        let finish = Arc::new(Semaphore::new(0));
        let shutdown = CancellationToken::new();
        let worker = tokio::spawn({
            let shutdown = shutdown.clone();
            let finish = finish.clone();
            let completed = completed.clone();
            async move {
                run(
                    &mut rx,
                    NonZeroUsize::new(2).unwrap(),
                    shutdown,
                    move |item| {
                        started_tx.send(item).unwrap();
                        let finish = finish.clone();
                        let completed = completed.clone();
                        async move {
                            finish.acquire().await.unwrap().forget();
                            completed.fetch_add(1, Ordering::SeqCst);
                        }
                    },
                )
                .await;
            }
        });
        tx.send(0).await.unwrap();
        tx.send(1).await.unwrap();
        assert_eq!(started_rx.recv().await, Some(0));
        assert_eq!(started_rx.recv().await, Some(1));
        tx.try_send(2).unwrap();
        tx.try_send(3).unwrap();
        assert!(matches!(
            tx.try_send(4),
            Err(mpsc::error::TrySendError::Full(4))
        ));

        // One completion admits exactly one queued event, never another task
        // waiting for capacity. Both accepted tasks still finish after cancel.
        finish.add_permits(1);
        assert_eq!(started_rx.recv().await, Some(2));
        shutdown.cancel();
        tokio::task::yield_now().await;
        assert!(!worker.is_finished());
        finish.add_permits(2);
        worker.await.unwrap();
        assert_eq!(completed.load(Ordering::SeqCst), 3);
        assert_eq!(started_rx.recv().await, None);
    }

    #[tokio::test]
    async fn closed_channel_finishes_every_message_even_after_a_task_panics() {
        let (tx, mut rx) = mpsc::channel(4);
        for item in 0..4 {
            tx.send(item).await.unwrap();
        }
        drop(tx);
        let completed = Arc::new(AtomicUsize::new(0));
        run(
            &mut rx,
            NonZeroUsize::new(1).unwrap(),
            CancellationToken::new(),
            {
                let completed = completed.clone();
                move |item| {
                    let completed = completed.clone();
                    async move {
                        if item == 1 {
                            panic!("failed write");
                        }
                        completed.fetch_add(1, Ordering::SeqCst);
                    }
                }
            },
        )
        .await;
        assert_eq!(completed.load(Ordering::SeqCst), 3);
    }
}
