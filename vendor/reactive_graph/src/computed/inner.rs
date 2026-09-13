use crate::{
    graph::{
        AnySource, AnySubscriber, Observer, ReactiveNode, ReactiveNodeState,
        Source, SourceSet, Subscriber, SubscriberSet, WithObserver,
    },
    owner::{Owner, Storage, StorageAccess},
};
use or_poisoned::OrPoisoned;
use std::{
    fmt::Debug,
    sync::{Arc, Condvar, Mutex, RwLock, RwLockWriteGuard},
    thread::{self, ThreadId},
};

pub struct MemoInner<T, S>
where
    S: Storage<T>,
{
    /// Must always be acquired *after* the reactivity lock
    pub(crate) value: Arc<RwLock<Option<S::Wrapped>>>,
    #[allow(clippy::type_complexity)]
    pub(crate) fun: Arc<dyn Fn(Option<T>) -> (T, bool) + Send + Sync>,
    pub(crate) owner: Owner,
    pub(crate) reactivity: RwLock<MemoInnerReactivity>,
    /// Serializes recomputation across threads. See [`MemoInner::update_if_necessary`].
    pub(crate) compute: ComputeLock,
}

/// Tracks which thread (if any) is currently running a memo's closure.
///
/// `update_if_necessary` `take()`s the cached value out of `value` while the
/// closure runs and only puts the new value back afterwards. Without this
/// lock two threads could both observe the memo as needing an update and race
/// through that window, or one thread could read the memo *between* the other
/// thread's `take()` and its store and find nothing there -- which is what
/// `ArcMemo::try_read_untracked` used to `unwrap()` on. On a multi-threaded
/// server (tokio + streaming SSR, where `Suspense` spawns effects onto worker
/// threads) that race is hit routinely.
///
/// The owning thread id is recorded so a reentrant read from inside the
/// memo's own closure is still detected (and reported) rather than deadlocking.
#[derive(Default)]
pub(crate) struct ComputeLock {
    owner: Mutex<Option<ThreadId>>,
    released: Condvar,
}

impl ComputeLock {
    /// Blocks until no *other* thread is computing, then claims the lock.
    ///
    /// Returns `None` when the current thread already holds it (a reentrant
    /// recompute), in which case the caller proceeds without a guard.
    fn acquire(&self) -> Option<ComputeGuard<'_>> {
        let me = thread::current().id();
        let mut owner = self.owner.lock().or_poisoned();
        loop {
            match *owner {
                Some(id) if id == me => return None,
                // Single-threaded targets (wasm32-unknown-unknown) can never
                // reach this arm, so `Condvar::wait` being unsupported there
                // is not a concern.
                Some(_) => owner = self.released.wait(owner).or_poisoned(),
                None => {
                    *owner = Some(me);
                    return Some(ComputeGuard { lock: self });
                }
            }
        }
    }

    /// Whether *another* thread currently holds the lock.
    pub(crate) fn held_by_other_thread(&self) -> bool {
        matches!(*self.owner.lock().or_poisoned(), Some(id) if id != thread::current().id())
    }

    /// Whether the current thread holds the lock (i.e. we are inside this
    /// memo's own closure).
    pub(crate) fn held_by_current_thread(&self) -> bool {
        matches!(*self.owner.lock().or_poisoned(), Some(id) if id == thread::current().id())
    }

    /// Blocks until whichever other thread holds the lock releases it.
    pub(crate) fn wait_for_release(&self) {
        let me = thread::current().id();
        let mut owner = self.owner.lock().or_poisoned();
        while matches!(*owner, Some(id) if id != me) {
            owner = self.released.wait(owner).or_poisoned();
        }
    }
}

/// Releases the [`ComputeLock`] on drop, including during a panic unwind, so
/// a closure that panics never leaves other threads waiting forever.
struct ComputeGuard<'a> {
    lock: &'a ComputeLock,
}

impl Drop for ComputeGuard<'_> {
    fn drop(&mut self) {
        *self.lock.owner.lock().or_poisoned() = None;
        self.lock.released.notify_all();
    }
}

pub(crate) struct MemoInnerReactivity {
    pub(crate) state: ReactiveNodeState,
    pub(crate) sources: SourceSet,
    pub(crate) subscribers: SubscriberSet,
    pub(crate) any_subscriber: AnySubscriber,
}

impl<T, S> Debug for MemoInner<T, S>
where
    S: Storage<T>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoInner").finish_non_exhaustive()
    }
}

impl<T: 'static, S> MemoInner<T, S>
where
    S: Storage<T>,
{
    #[allow(clippy::type_complexity)]
    pub fn new(
        fun: Arc<dyn Fn(Option<T>) -> (T, bool) + Send + Sync>,
        any_subscriber: AnySubscriber,
    ) -> Self {
        Self {
            value: Arc::new(RwLock::new(None)),
            fun,
            owner: Owner::new(),
            reactivity: RwLock::new(MemoInnerReactivity {
                state: ReactiveNodeState::Dirty,
                sources: Default::default(),
                subscribers: SubscriberSet::new(),
                any_subscriber,
            }),
            compute: ComputeLock::default(),
        }
    }
}

impl<T: 'static, S> ReactiveNode for MemoInner<T, S>
where
    S: Storage<T>,
{
    fn mark_dirty(&self) {
        self.reactivity.write().or_poisoned().state = ReactiveNodeState::Dirty;
        self.mark_subscribers_check();
    }

    fn mark_check(&self) {
        /// codegen optimisation:
        fn inner(reactivity: &RwLock<MemoInnerReactivity>) {
            {
                let mut lock = reactivity.write().or_poisoned();
                if lock.state != ReactiveNodeState::Dirty {
                    lock.state = ReactiveNodeState::Check;
                }
            }
            for sub in
                (&reactivity.read().or_poisoned().subscribers).into_iter()
            {
                sub.mark_check();
            }
        }
        inner(&self.reactivity);
    }

    fn mark_subscribers_check(&self) {
        let lock = self.reactivity.read().or_poisoned();
        for sub in (&lock.subscribers).into_iter() {
            sub.mark_check();
        }
    }

    fn update_if_necessary(&self) -> bool {
        /// codegen optimisation:
        fn needs_update(reactivity: &RwLock<MemoInnerReactivity>) -> bool {
            let (state, sources) = {
                let inner = reactivity.read().or_poisoned();
                (inner.state, inner.sources.clone())
            };
            match state {
                ReactiveNodeState::Clean => false,
                ReactiveNodeState::Dirty => true,
                ReactiveNodeState::Check => {
                    (&sources).into_iter().any(|source| {
                        source.update_if_necessary()
                            || reactivity.read().or_poisoned().state
                                == ReactiveNodeState::Dirty
                    })
                }
            }
        }

        if needs_update(&self.reactivity) {
            // Another thread may already be recomputing this memo. Wait for
            // it rather than racing it: once it finishes, the memo is `Clean`
            // and the re-check below skips the redundant (and, with the value
            // already taken, `None`-seeded) second run.
            //
            // `None` means *this* thread is already inside the closure -- a
            // memo reading itself. That was never supported (it used to
            // recurse until the stack overflowed); now the nested read finds
            // no value and fails the way a disposed memo does.
            let Some(compute_guard) = self.compute.acquire() else {
                return false;
            };
            if !needs_update(&self.reactivity) {
                let mut lock = self.reactivity.write().or_poisoned();
                if lock.state == ReactiveNodeState::Check {
                    lock.state = ReactiveNodeState::Clean;
                }
                return false;
            }

            // No deadlock risk, because we only hold the value lock.
            let value = self.value.write().or_poisoned().take();
            // Mark `Clean` *before* running the closure rather than after
            // it: a source that changes on another thread while the closure
            // is running marks this memo `Dirty` again, and stamping `Clean`
            // afterwards would silently discard that update and serve the
            // stale value until the next change.
            self.reactivity.write().or_poisoned().state =
                ReactiveNodeState::Clean;

            /// codegen optimisation:
            fn inner_1(
                reactivity: &RwLock<MemoInnerReactivity>,
            ) -> AnySubscriber {
                let any_subscriber =
                    reactivity.read().or_poisoned().any_subscriber.clone();
                any_subscriber.clear_sources(&any_subscriber);
                any_subscriber
            }
            let any_subscriber = inner_1(&self.reactivity);

            let (new_value, changed) = self.owner.with_cleanup(|| {
                any_subscriber.with_observer(|| {
                    (self.fun)(value.map(StorageAccess::into_taken))
                })
            });

            // Two locks are acquired, so order matters.
            let reactivity_lock = self.reactivity.write().or_poisoned();
            {
                // Safety: Can block endlessly if the user is has a ReadGuard on the value
                let mut value_lock = self.value.write().or_poisoned();
                *value_lock = Some(S::wrap(new_value));
            }

            /// codegen optimisation:
            fn inner_2(
                changed: bool,
                reactivity_lock: RwLockWriteGuard<'_, MemoInnerReactivity>,
            ) {
                // `state` was set to `Clean` before the closure ran and is
                // deliberately left alone here: see above.
                if changed {
                    let subs = reactivity_lock.subscribers.clone();
                    drop(reactivity_lock);
                    for sub in subs {
                        // don't trigger reruns of effects/memos
                        // basically: if one of the observers has triggered this memo to
                        // run, it doesn't need to be re-triggered because of this change
                        if !Observer::is(&sub) {
                            sub.mark_dirty();
                        }
                    }
                } else {
                    drop(reactivity_lock);
                }
            }
            inner_2(changed, reactivity_lock);
            // Only now -- with the value stored and the state `Clean` -- may
            // waiting readers proceed.
            drop(compute_guard);

            changed
        } else {
            /// codegen optimisation:
            fn inner(reactivity: &RwLock<MemoInnerReactivity>) -> bool {
                let mut lock = reactivity.write().or_poisoned();
                // Only settle a `Check` we just walked. Overwriting a `Dirty`
                // that another thread stamped in the meantime would drop that
                // update on the floor.
                if lock.state == ReactiveNodeState::Check {
                    lock.state = ReactiveNodeState::Clean;
                }
                false
            }
            inner(&self.reactivity)
        }
    }
}

impl<T: 'static, S> Source for MemoInner<T, S>
where
    S: Storage<T>,
{
    fn add_subscriber(&self, subscriber: AnySubscriber) {
        let mut lock = self.reactivity.write().or_poisoned();
        lock.subscribers.subscribe(subscriber);
    }

    fn remove_subscriber(&self, subscriber: &AnySubscriber) {
        self.reactivity
            .write()
            .or_poisoned()
            .subscribers
            .unsubscribe(subscriber);
    }

    fn clear_subscribers(&self) {
        self.reactivity.write().or_poisoned().subscribers.take();
    }
}

impl<T: 'static, S> Subscriber for MemoInner<T, S>
where
    S: Storage<T>,
{
    fn add_source(&self, source: AnySource) {
        self.reactivity.write().or_poisoned().sources.insert(source);
    }

    fn clear_sources(&self, subscriber: &AnySubscriber) {
        self.reactivity
            .write()
            .or_poisoned()
            .sources
            .clear_sources(subscriber);
    }
}
