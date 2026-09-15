//! Regression test: reading a memo from several threads while one of its
//! sources changes must never observe the "hole" left while another thread
//! is recomputing it (`arc_memo.rs: called Option::unwrap() on a None value`).

use reactive_graph::{
    computed::ArcMemo,
    owner::Owner,
    signal::ArcRwSignal,
    traits::{Get, Set},
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[test]
fn a_panicking_computation_can_be_retried() {
    let owner = Owner::new();
    owner.set();
    let calls = Arc::new(AtomicUsize::new(0));
    let memo = ArcMemo::new(move |_| {
        assert_ne!(calls.fetch_add(1, Ordering::SeqCst), 0, "first call fails");
        7
    });
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || memo.get()
    ))
    .is_err());
    assert_eq!(memo.get(), 7);
}

#[test]
fn concurrent_reads_during_recompute_never_panic() {
    let owner = Owner::new();
    owner.set();

    let source = ArcRwSignal::new(0u64);
    let memo = ArcMemo::new({
        let source = source.clone();
        move |_| {
            let value = source.get();
            // Widen the window in which the cached value is taken out.
            let start = Instant::now();
            while start.elapsed() < Duration::from_micros(20) {}
            value
        }
    });

    let panics = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let readers = (0..4)
        .map(|_| {
            let memo = memo.clone();
            let stop = Arc::clone(&stop);
            let panics = Arc::clone(&panics);
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let read = std::panic::catch_unwind(
                        std::panic::AssertUnwindSafe(|| memo.get()),
                    );
                    if let Err(payload) = read {
                        let message = payload
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| {
                                payload
                                    .downcast_ref::<&str>()
                                    .map(|s| s.to_string())
                            })
                            .unwrap_or_else(|| "non-string panic".to_string());
                        panics.lock().unwrap().push(message);
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut writes = 0u64;
    while Instant::now() < deadline {
        writes += 1;
        source.set(writes);
        thread::sleep(Duration::from_micros(30));
    }
    stop.store(true, Ordering::Relaxed);
    for reader in readers {
        reader.join().unwrap();
    }

    let panics = panics.lock().unwrap();
    assert!(
        panics.is_empty(),
        "memo reads panicked during recompute: {panics:?}"
    );
    assert_eq!(memo.get(), writes);
}

#[test]
fn reentrant_read_is_reported_not_deadlocked() {
    use reactive_graph::traits::GetUntracked;

    let owner = Owner::new();
    owner.set();

    // A memo whose closure reads itself. Never supported; it must fail fast
    // (an `Option` from `try_get`, a panic from `get`) rather than hang.
    let slot: Arc<std::sync::Mutex<Option<ArcMemo<u32>>>> =
        Arc::new(std::sync::Mutex::new(None));
    let memo = ArcMemo::new({
        let slot = Arc::clone(&slot);
        move |_| {
            let me = slot.lock().unwrap().clone();
            me.and_then(|m| m.try_get_untracked()).unwrap_or(7)
        }
    });
    *slot.lock().unwrap() = Some(memo.clone());

    let done = Arc::new(AtomicBool::new(false));
    let worker = thread::spawn({
        let memo = memo.clone();
        let done = Arc::clone(&done);
        move || {
            let value = memo.get();
            done.store(true, Ordering::Relaxed);
            value
        }
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done.load(Ordering::Relaxed) {
        assert!(Instant::now() < deadline, "reentrant memo read deadlocked");
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(worker.join().unwrap(), 7);
}
