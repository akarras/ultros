//! Regression test for the SSR panics that survived akarras/ultros#1520
//! (GlitchTip #7388 `cookies.rs:174`, and the `leptos_i18n context.rs:213`
//! tail of #7382): an effect whose owner is cleaned up before its spawned
//! first run gets polled must not run at all.
//!
//! `Effect::new_isomorphic` (and `Effect::new`) queue their first run as a
//! detached task. That task keeps its own `Arc` to the effect and to the
//! child owner, so nothing in it notices when the request's root owner is
//! torn down — the body runs anyway and reads a signal whose arena node is
//! gone: "you tried to access a reactive value … but it has already been
//! disposed". On the server this happens whenever a render finishes before
//! the tokio scheduler gets to the effect task (leptos' `<Suspense>` effect
//! that `dry_resolve`s the boundary's children, `leptos_i18n`'s locale
//! effect); on the client it is the "effect outlives its page" shape.
//!
//! Run with `--features effects` for the `Effect::new` case: without it
//! `Effect::new` is a no-op and that test trivially passes.

use any_spawner::Executor;
use reactive_graph::{
    effect::Effect,
    owner::Owner,
    signal::RwSignal,
    traits::{Get, Set},
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Barrier,
    },
    time::Duration,
};

async fn settle() {
    for _ in 0..8 {
        Executor::tick().await;
    }
}

#[tokio::test]
async fn isomorphic_effect_does_not_run_after_its_owner_is_cleaned_up() {
    _ = Executor::init_tokio();
    let owner = Owner::new();
    owner.set();

    let source = RwSignal::new(0);
    let runs = Arc::new(AtomicUsize::new(0));

    Effect::new_isomorphic({
        let runs = Arc::clone(&runs);
        move |_| {
            runs.fetch_add(1, Ordering::SeqCst);
            // The read that panics in production: the node is gone.
            source.get();
        }
    });

    // The render is over before the effect task was ever polled.
    owner.unset_with_forced_cleanup();
    settle().await;

    assert_eq!(
        runs.load(Ordering::SeqCst),
        0,
        "an effect must not run once its owner has been cleaned up"
    );
}

#[tokio::test]
async fn isomorphic_effect_stops_rerunning_after_its_owner_is_cleaned_up() {
    _ = Executor::init_tokio();
    let owner = Owner::new();
    owner.set();

    let source = RwSignal::new(0);
    let survivor = RwSignal::new(0);
    let runs = Arc::new(AtomicUsize::new(0));

    Effect::new_isomorphic({
        let runs = Arc::clone(&runs);
        move |_| {
            runs.fetch_add(1, Ordering::SeqCst);
            source.get();
        }
    });
    settle().await;
    assert_eq!(runs.load(Ordering::SeqCst), 1, "first run happens normally");

    // Queue a rerun, then tear the owner down before the task gets to it.
    source.set(1);
    owner.unset_with_forced_cleanup();
    settle().await;
    // Unrelated activity must not resurrect it either.
    survivor.set(1);
    settle().await;

    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "a queued rerun must be dropped once the owner is cleaned up"
    );
}

#[cfg(feature = "effects")]
#[tokio::test]
async fn local_effect_does_not_run_after_its_owner_is_cleaned_up() {
    _ = Executor::init_tokio();
    let owner = Owner::new();
    owner.set();

    tokio::task::LocalSet::new()
        .run_until(async {
            let source = RwSignal::new(0);
            let runs = Arc::new(AtomicUsize::new(0));

            Effect::new({
                let runs = Arc::clone(&runs);
                move |_| {
                    runs.fetch_add(1, Ordering::SeqCst);
                    source.get();
                }
            });

            owner.unset_with_forced_cleanup();
            settle().await;

            assert_eq!(
                runs.load(Ordering::SeqCst),
                0,
                "a local effect must not run once its owner has been cleaned \
                 up"
            );
        })
        .await;
}

/// The production shape: the effect body is *mid-run* on one worker when
/// the request's root owner is force-cleaned on another (leptos' from_app
/// does that at the end of the response stream). Every read after that
/// point used to panic with "already been disposed"; the forced cleanup
/// must wait for the running body instead.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn forced_cleanup_waits_for_a_running_effect_body() {
    _ = Executor::init_tokio();
    let owner = Owner::new();
    owner.set();

    let first = RwSignal::new(1);
    let second = RwSignal::new(2);
    let completed = Arc::new(AtomicBool::new(false));
    let started = Arc::new(Barrier::new(2));

    Effect::new_isomorphic({
        let completed = Arc::clone(&completed);
        let started = Arc::clone(&started);
        move |_| {
            first.get();
            // Hand control to the test thread and give it time to run the
            // cleanup while this body is still going.
            started.wait();
            std::thread::sleep(Duration::from_millis(100));
            second.get();
            completed.store(true, Ordering::SeqCst);
        }
    });

    let cleanup = tokio::task::spawn_blocking(move || {
        started.wait();
        owner.unset_with_forced_cleanup();
    });
    cleanup.await.unwrap();
    // Let the effect task finish (or die) before asserting.
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(
        completed.load(Ordering::SeqCst),
        "the effect body must finish before its tree is torn down"
    );
}

/// Same race through the other teardown path: the root `Owner` is simply
/// dropped (an abandoned render past leptos' drain cap) while a body runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn root_drop_waits_for_a_running_effect_body() {
    _ = Executor::init_tokio();
    let owner = Owner::new();
    owner.set();

    let first = RwSignal::new(1);
    let second = RwSignal::new(2);
    let completed = Arc::new(AtomicBool::new(false));
    let started = Arc::new(Barrier::new(2));

    Effect::new_isomorphic({
        let completed = Arc::clone(&completed);
        let started = Arc::clone(&started);
        move |_| {
            first.get();
            started.wait();
            std::thread::sleep(Duration::from_millis(100));
            second.get();
            completed.store(true, Ordering::SeqCst);
        }
    });

    let teardown = tokio::task::spawn_blocking(move || {
        started.wait();
        // `set()` only keeps a weak handle, so this is the last strong one.
        drop(owner);
    });
    teardown.await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(
        completed.load(Ordering::SeqCst),
        "the effect body must finish before its tree is dropped"
    );
}

/// A body that tears down its own tree must not wait on itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_tearing_down_its_own_tree_does_not_deadlock() {
    _ = Executor::init_tokio();
    let owner = Owner::new();
    owner.set();

    let done = Arc::new(AtomicBool::new(false));
    let handle = owner.clone();
    Effect::new_isomorphic({
        let done = Arc::clone(&done);
        move |_| {
            handle.clone().unset_with_forced_cleanup();
            done.store(true, Ordering::SeqCst);
        }
    });

    let finished = tokio::time::timeout(Duration::from_secs(2), async {
        while !done.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(finished.is_ok(), "the body blocked on its own teardown");
}
