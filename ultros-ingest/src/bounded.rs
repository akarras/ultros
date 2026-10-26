//! Bound work before spawning it, leaving excess events in the producer's
//! bounded channel. A semaphore acquired inside a spawned task only moves the
//! unbounded queue into tasks waiting for permits.

use std::{collections::HashMap, future::Future, hash::Hash, num::NonZeroUsize};

use tokio::{
    sync::{mpsc::Receiver, oneshot},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

/// Stop dequeuing on cancellation or channel closure, then finish every
/// accepted event. Reap completions as we go so even finished tasks cannot
/// accumulate in the set. The caller must keep downstream consumers alive
/// until this drain completes.
pub async fn run<T, F, Fut>(
    receiver: &mut Receiver<T>,
    max_in_flight: NonZeroUsize,
    shutdown: CancellationToken,
    process: F,
) where
    F: FnMut(T) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let mut sequence = 0_u64;
    run_ordered(
        receiver,
        max_in_flight,
        shutdown,
        move |_| {
            sequence = sequence.wrapping_add(1);
            sequence
        },
        process,
    )
    .await;
}

/// Preserve receive order for each key, including database writes and event
/// publication, while unrelated keys can run concurrently. Waiting successors
/// count toward the same admission limit; they cannot form an unbounded queue
/// of spawned tasks. A failed predecessor still releases its successors.
pub async fn run_ordered<T, K, Key, F, Fut>(
    receiver: &mut Receiver<T>,
    max_in_flight: NonZeroUsize,
    shutdown: CancellationToken,
    mut key: Key,
    mut process: F,
) where
    K: Eq + Hash,
    Key: FnMut(&T) -> K,
    F: FnMut(T) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let mut tasks = JoinSet::new();
    let mut tails: HashMap<K, oneshot::Receiver<()>> = HashMap::new();
    loop {
        // Retain only keys with accepted work. `try_recv` consumes completion
        // only for the last task in a chain, after all its predecessors ended.
        tails.retain(|_, tail| matches!(tail.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
        metrics::gauge!("ultros_websocket_in_flight").set(tasks.len() as f64);
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            result = tasks.join_next(), if !tasks.is_empty() => {
                report_completion(result);
            }
            message = receiver.recv(), if tasks.len() < max_in_flight.get() => {
                let Some(message) = message else { break };
                let (finished, tail) = oneshot::channel();
                let predecessor = tails.insert(key(&message), tail);
                let future = process(message);
                tasks.spawn(async move {
                    if let Some(predecessor) = predecessor {
                        let _ = predecessor.await;
                    }
                    future.await;
                    let _ = finished.send(());
                });
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
    async fn one_boards_events_stay_ordered_without_blocking_other_boards() {
        let (tx, mut rx) = mpsc::channel(4);
        for message in [(1, 0), (1, 1), (2, 0)] {
            tx.send(message).await.unwrap();
        }
        drop(tx);
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let release = Arc::new(Semaphore::new(0));
        let worker = tokio::spawn({
            let release = release.clone();
            async move {
                run_ordered(
                    &mut rx,
                    NonZeroUsize::new(3).unwrap(),
                    CancellationToken::new(),
                    |(board, _)| *board,
                    move |message| {
                        let release = release.clone();
                        let started_tx = started_tx.clone();
                        async move {
                            started_tx.send(message).unwrap();
                            if message == (1, 0) {
                                release.acquire().await.unwrap().forget();
                            }
                        }
                    },
                )
                .await;
            }
        });
        let mut first = [
            tokio::time::timeout(std::time::Duration::from_secs(5), started_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            tokio::time::timeout(std::time::Duration::from_secs(5), started_rx.recv())
                .await
                .unwrap()
                .unwrap(),
        ];
        first.sort_unstable();
        assert_eq!(first, [(1, 0), (2, 0)]);
        assert!(started_rx.try_recv().is_err());
        release.add_permits(1);
        tokio::time::timeout(std::time::Duration::from_secs(5), worker)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(started_rx.recv().await, Some((1, 1)));
    }

    #[tokio::test]
    async fn a_panicking_predecessor_releases_the_next_event_for_its_board() {
        let (tx, mut rx) = mpsc::channel(3);
        for item in 0..3 {
            tx.send(item).await.unwrap();
        }
        drop(tx);
        let completed = Arc::new(AtomicUsize::new(0));
        let future = run_ordered(
            &mut rx,
            NonZeroUsize::new(3).unwrap(),
            CancellationToken::new(),
            |_| 1,
            {
                let completed = completed.clone();
                move |item| {
                    let completed = completed.clone();
                    async move {
                        if item == 0 {
                            panic!("failed predecessor");
                        }
                        completed.fetch_add(1, Ordering::SeqCst);
                    }
                }
            },
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), future)
            .await
            .unwrap();
        assert_eq!(completed.load(Ordering::SeqCst), 2);
    }

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
