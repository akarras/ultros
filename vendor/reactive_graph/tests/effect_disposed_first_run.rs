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

/// The window the in-flight guard alone left open (the `context.rs:213`
/// tail of GlitchTip #7382 that kept firing after the guard shipped): the
/// effect task has already taken its wake-up off the channel, and the
/// teardown checks the tracker, finds nothing running and clears the arena
/// before the task gets to enter its body. The body must not run then.
///
/// This races the effect task's first poll against the teardown on real
/// threads, so a single iteration rarely lands in the window; enough of
/// them do.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_effect_body_never_starts_after_its_tree_is_torn_down() {
    _ = Executor::init_tokio();
    let ran_disposed = Arc::new(AtomicUsize::new(0));

    for i in 0..20_000u32 {
        let owner = Owner::new();
        let signal = owner.with(|| {
            let signal = RwSignal::new(1);
            Effect::new_isomorphic({
                let ran_disposed = Arc::clone(&ran_disposed);
                move |_| {
                    if signal.try_get().is_none() {
                        ran_disposed.fetch_add(1, Ordering::SeqCst);
                    }
                }
            });
            signal
        });
        // Stagger the teardown against the effect task's first poll.
        for _ in 0..(i % 64) {
            std::hint::spin_loop();
        }
        owner.unset_with_forced_cleanup();
        drop(signal);
        if i % 256 == 0 {
            tokio::task::yield_now().await;
        }
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(
        ran_disposed.load(Ordering::SeqCst),
        0,
        "an effect body started after its tree was torn down"
    );
}

#[cfg(feature = "sandboxed-arenas")]
#[tokio::test]
async fn completed_resource_and_effect_release_arena() {
    use reactive_graph::{computed::AsyncDerived, traits::Read};
    _ = Executor::init_tokio();
    let owner = Owner::new();
    owner.set();
    let resource = AsyncDerived::new(|| async { 42 });
    Effect::new_isomorphic(move |_| {
        let _ = resource.read();
    });
    settle().await;
    owner.cleanup();
    settle().await;
    assert_eq!(owner.arena_len(), 0);
    let arena = owner.arena_weak_reference();
    drop(owner);
    assert!(arena.upgrade().is_none());
}

/// SSR LocalResource uses a mock derived resource that stays pending forever.
/// Its suspense waiter must end at teardown even though readiness never fires.
#[cfg(feature = "sandboxed-arenas")]
#[tokio::test]
async fn mock_resource_waiter_releases_arena_after_cleanup() {
    use reactive_graph::{
        computed::{suspense::SuspenseContext, ArcAsyncDerived},
        owner::{provide_context, StoredValue},
        signal::ArcRwSignal,
        traits::ReadUntracked,
    };
    _ = Executor::init_tokio();
    for cycle in 0..32 {
        let owner = Owner::new();
        owner.set();
        // Give this request a real backing buffer, as item-page signals do.
        for value in 0..256 {
            let _ = StoredValue::new(value);
        }
        let suspense = SuspenseContext {
            tasks: ArcRwSignal::new(Default::default()),
        };
        provide_context(suspense.clone());
        let resource = ArcAsyncDerived::<i32>::new_mock(std::future::pending);
        let _ = resource.read_untracked();
        // Cover cancellation both before the task is first polled and
        // after it has registered its own waker.
        if cycle % 2 != 0 {
            settle().await;
        }
        assert_eq!(suspense.tasks.read_untracked().len(), 1);
        drop(resource);
        owner.cleanup();
        settle().await;
        assert_eq!(suspense.tasks.read_untracked().len(), 0);
        assert_eq!(owner.arena_len(), 0);
        let arena = owner.arena_weak_reference();
        drop(owner);
        assert!(
            arena.upgrade().is_none(),
            "a waiter for a mock resource retained the arena"
        );
    }
}

#[cfg(feature = "sandboxed-arenas")]
#[tokio::test]
async fn resolved_resource_waiter_releases_arena_before_cleanup() {
    use reactive_graph::{
        computed::{suspense::SuspenseContext, ArcAsyncDerived},
        owner::provide_context,
        signal::ArcRwSignal,
        traits::ReadUntracked,
    };
    _ = Executor::init_tokio();
    let owner = Owner::new();
    owner.set();
    let suspense = SuspenseContext {
        tasks: ArcRwSignal::new(Default::default()),
    };
    provide_context(suspense.clone());
    let (tx, rx) = futures::channel::oneshot::channel::<i32>();
    let rx = Arc::new(std::sync::Mutex::new(Some(rx)));
    let resource = ArcAsyncDerived::new(move || {
        let rx = rx.lock().unwrap().take().unwrap();
        async move { rx.await.unwrap() }
    });
    let _ = resource.read_untracked();
    settle().await;
    assert!(!suspense.tasks.read_untracked().is_empty());
    tx.send(42).unwrap();
    settle().await;
    assert_eq!(*resource.read_untracked(), Some(42));
    assert_eq!(suspense.tasks.read_untracked().len(), 0);
    drop(resource);
    owner.cleanup();
    settle().await;
    let arena = owner.arena_weak_reference();
    drop(owner);
    assert!(arena.upgrade().is_none());
}
#[cfg(feature = "sandboxed-arenas")]
#[tokio::test]
async fn reader_cleanup_releases_waiter_arena_without_cancelling_resource() {
    use reactive_graph::{
        computed::{suspense::SuspenseContext, ArcAsyncDerived},
        owner::provide_context,
        signal::ArcRwSignal,
        traits::ReadUntracked,
    };
    _ = Executor::init_tokio();
    let root = Owner::new();
    root.set();
    let (tx, rx) = futures::channel::oneshot::channel::<i32>();
    let rx = Arc::new(std::sync::Mutex::new(Some(rx)));
    let resource = ArcAsyncDerived::new(move || {
        let rx = rx.lock().unwrap().take().unwrap();
        async move { rx.await.unwrap() }
    });
    settle().await;
    let reader = root.child();
    let suspense = SuspenseContext {
        tasks: ArcRwSignal::new(Default::default()),
    };
    reader.with(|| {
        provide_context(suspense.clone());
        let _ = resource.read_untracked();
    });
    settle().await;
    assert_eq!(suspense.tasks.read_untracked().len(), 1);
    reader.cleanup();
    settle().await;
    assert!(suspense.tasks.read_untracked().is_empty());
    tx.send(42).unwrap();
    settle().await;
    assert_eq!(*resource.read_untracked(), Some(42));
    drop(resource);
    drop(reader);
    root.cleanup();
    settle().await;
    let arena = root.arena_weak_reference();
    drop(root);
    assert!(arena.upgrade().is_none());
}
#[cfg(feature = "sandboxed-arenas")]
#[test]
fn cleanup_restores_outer_arena_before_spawning_waiters() {
    use reactive_graph::owner::{on_cleanup, Sandboxed};
    let cleaned = Owner::new();
    cleaned.set();
    on_cleanup(|| {});
    let arena = cleaned.arena_weak_reference();
    let outer = Owner::default();
    let future = outer.with(|| {
        cleaned.cleanup();
        Sandboxed::new(std::future::pending::<()>())
    });
    drop(cleaned);
    assert!(
        arena.upgrade().is_none(),
        "the next task captured the cleaned owner's arena"
    );
    drop(future);
}
#[cfg(feature = "sandboxed-arenas")]
#[test]
fn local_resource_notifier_cleanup_releases_waiting_arena() {
    use futures::{
        task::{waker, ArcWake},
        FutureExt,
    };
    use reactive_graph::{
        computed::suspense::LocalResourceNotifier, owner::provide_context,
    };
    use std::{future::Future, pin::Pin, task::Context};
    struct RenderWaker(Owner);
    impl ArcWake for RenderWaker {
        fn wake_by_ref(this: &Arc<Self>) {
            let _ = this.0.debug_id();
        }
    }
    let owner = Owner::default();
    owner.set();
    let arena = owner.arena_weak_reference();
    let (tx, mut rx) = futures::channel::oneshot::channel();
    provide_context(LocalResourceNotifier::from(tx));
    // Model a pending SSR stream whose waker keeps its scoped owner alive.
    let waker = waker(Arc::new(RenderWaker(owner.clone())));
    assert!(Pin::new(&mut rx)
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    drop(waker);
    owner.cleanup();
    drop(owner);
    assert!(
        arena.upgrade().is_none(),
        "the notifier context retained the render owner through its \
         receiver's waker"
    );
    assert!(matches!(rx.now_or_never(), Some(Err(_))));
}
#[cfg(all(feature = "sandboxed-arenas", feature = "hydration"))]
#[test]
fn root_teardown_releases_late_hydration_resource_arena() {
    use hydration_context::{
        SerializedDataId, SharedContext, SsrSharedContext,
    };
    for forced in [false, true] {
        let shared = Arc::new(SsrSharedContext::new());
        let root = Owner::new_root(Some(shared.clone()));
        // A stream or one of its wakers can retain the buffers independently
        // of the SsrSharedContext Arc. Teardown must empty those buffers.
        let stream = shared.pending_data().unwrap();
        let arena = root.arena_weak_reference();
        let child = root.with(Owner::new);
        // A late resource serializer can remain buffered after the data stream
        // ends. Its resource/fetcher owns a child owner with the same context.
        shared.write_async(
            SerializedDataId::new(0),
            Box::pin(async move {
                std::future::pending::<()>().await;
                drop(child);
                String::new()
            }),
        );
        drop(shared);
        if forced {
            root.unset_with_forced_cleanup();
        } else {
            drop(root);
        }
        assert!(
            arena.upgrade().is_none(),
            "the hydration buffer and child owner retained each other after \
             root teardown"
        );
        drop(stream);
    }
}
#[cfg(all(feature = "sandboxed-arenas", feature = "hydration"))]
#[test]
fn ordinary_cleanup_keeps_shared_context_for_live_arena() {
    use hydration_context::{SharedContext, SsrSharedContext};
    let shared: Arc<dyn SharedContext + Send + Sync> =
        Arc::new(SsrSharedContext::new());
    let root = Owner::new_root(Some(shared.clone()));
    let child = root.with(Owner::new);
    root.cleanup();
    child.clone().unset_with_forced_cleanup();
    assert!(Arc::ptr_eq(&root.shared_context().unwrap(), &shared));
    assert!(Arc::ptr_eq(&child.shared_context().unwrap(), &shared));
    let survivor = root.clone();
    root.unset_with_forced_cleanup();
    assert!(Arc::ptr_eq(&survivor.shared_context().unwrap(), &shared));
    assert!(Arc::ptr_eq(&child.shared_context().unwrap(), &shared));
}
#[cfg(feature = "sandboxed-arenas")]
#[test]
fn sandbox_poll_keeps_the_captured_owner_and_arena_together() {
    use reactive_graph::owner::Sandboxed;
    use std::{
        future::{poll_fn, Future},
        pin::Pin,
        task::{Context, Poll},
    };
    let a = Owner::default();
    let b = Owner::default();
    let mut task = a.with(|| {
        Sandboxed::new(poll_fn(|_| {
            Poll::Ready(Owner::current().unwrap().debug_id())
        }))
    });
    let result = b.with(|| {
        let result = Pin::new(&mut task)
            .poll(&mut Context::from_waker(futures::task::noop_waker_ref()));
        assert_eq!(Owner::current().unwrap().debug_id(), b.debug_id());
        result
    });
    assert_eq!(result, Poll::Ready(a.debug_id()));
}

#[cfg(feature = "sandboxed-arenas")]
#[test]
fn late_cleanup_cannot_create_a_cycle_in_a_closed_arena() {
    use reactive_graph::owner::on_cleanup;
    let root = Owner::default();
    root.set();
    let survivor = root.clone();
    let arena = root.arena_weak_reference();
    root.unset_with_forced_cleanup();
    let ran = Arc::new(AtomicBool::new(false));
    survivor.with(|| {
        let captured_owner = survivor.clone();
        let ran = ran.clone();
        on_cleanup(move || {
            drop(captured_owner);
            ran.store(true, Ordering::SeqCst);
        });
    });
    assert!(
        ran.load(Ordering::SeqCst),
        "cleanup registered after teardown must run immediately"
    );
    drop(survivor);
    assert!(arena.upgrade().is_none());
}
#[cfg(feature = "sandboxed-arenas")]
#[test]
fn sandbox_panic_restores_the_callers_owner_and_arena() {
    use reactive_graph::owner::Sandboxed;
    use std::{
        future::{poll_fn, Future},
        pin::Pin,
        task::{Context, Poll},
    };
    let a = Owner::default();
    let b = Owner::default();
    let _a_value = a.with(|| RwSignal::new(11));
    let b_value = b.with(|| RwSignal::new(22));
    let mut task = a.with(|| {
        Sandboxed::new(poll_fn(|_| -> Poll<()> { panic!("sandbox probe") }))
    });
    b.with(|| {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Pin::new(&mut task).poll(&mut Context::from_waker(
                    futures::task::noop_waker_ref(),
                ))
            }));
        assert!(result.is_err());
        assert_eq!(Owner::current().unwrap().debug_id(), b.debug_id());
        assert_eq!(b_value.get(), 22);
    });
}
