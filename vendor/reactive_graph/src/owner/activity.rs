//! Tracks effect bodies that are currently running somewhere in an owner
//! tree, so a forced teardown of the tree can wait for them.
//!
//! Effects run on spawned tasks. On a multi-threaded runtime an effect's body
//! can be mid-run on one worker while another thread tears the tree's root
//! down (leptos' `from_app` does that at the end of every response stream);
//! every arena read the body makes after that point panics with "already
//! been disposed", even though the body was started while the tree was
//! alive. The tracker is shared by every owner in a tree (cloned from the
//! parent like the arena) and counts bodies in flight; the root's forced
//! cleanup waits for the count to reach zero — bounded, and never for a body
//! running on the waiting thread itself.

use or_poisoned::OrPoisoned;
use std::{
    cell::RefCell,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

/// Longest a forced teardown waits for a running effect body. An effect body
/// is synchronous and short (a `<Suspense>`'s `dry_resolve` walk is the
/// heaviest in practice); anything past this is a stuck body, and tearing
/// down under it is the pre-existing behaviour.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(crate) struct Activity {
    running: Mutex<usize>,
    idle: Condvar,
}

thread_local! {
    /// The trackers whose effect bodies this thread is currently inside,
    /// so a teardown triggered from within a body never waits on itself.
    static ENTERED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

impl Activity {
    fn id(self: &Arc<Self>) -> usize {
        Arc::as_ptr(self) as usize
    }

    /// Marks an effect body as running until the returned guard drops.
    pub(crate) fn enter(self: &Arc<Self>) -> RunGuard {
        *self.running.lock().or_poisoned() += 1;
        ENTERED.with_borrow_mut(|entered| entered.push(self.id()));
        RunGuard(Arc::clone(self))
    }

    /// Blocks until no effect body of this tree is running, or until
    /// [`IDLE_TIMEOUT`] passes. Returns at once if the current thread is
    /// itself inside one of this tree's effect bodies.
    pub(crate) fn wait_idle(self: &Arc<Self>) {
        let id = self.id();
        if ENTERED.with_borrow(|entered| entered.contains(&id)) {
            return;
        }
        let running = self.running.lock().or_poisoned();
        if *running == 0 {
            return;
        }
        // A body that unwound has already released its guard, so a poisoned
        // lock carries nothing worth propagating; either way the wait ends.
        _ = self
            .idle
            .wait_timeout_while(running, IDLE_TIMEOUT, |running| *running > 0)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

/// Decrements the running count when the effect body it was taken for ends,
/// whether it returned or unwound.
pub(crate) struct RunGuard(Arc<Activity>);

impl Drop for RunGuard {
    fn drop(&mut self) {
        let id = self.0.id();
        ENTERED.with_borrow_mut(|entered| {
            if let Some(pos) = entered.iter().rposition(|e| *e == id) {
                entered.remove(pos);
            }
        });
        let mut running = self.0.running.lock().or_poisoned();
        *running -= 1;
        if *running == 0 {
            self.0.idle.notify_all();
        }
    }
}
