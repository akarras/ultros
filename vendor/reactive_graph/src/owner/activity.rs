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
//!
//! Waiting alone leaves a window: an effect task that has already taken its
//! wake-up but not yet entered its body is not counted, so the teardown sees
//! nothing running, clears the arena, and the body then starts against dead
//! signals. A root teardown therefore also *closes* the tracker, under the
//! same lock `enter` takes: a body either entered before the close (and is
//! waited for) or sees the tracker closed and does not run.

use or_poisoned::OrPoisoned;
use std::{
    cell::RefCell,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::Duration,
};

/// Longest a forced teardown waits for a running effect body. An effect body
/// is synchronous and short (a `<Suspense>`'s `dry_resolve` walk is the
/// heaviest in practice); anything past this is a stuck body, and tearing
/// down under it is the pre-existing behaviour.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(crate) struct Activity {
    state: Mutex<State>,
    idle: Condvar,
}

#[derive(Default)]
struct State {
    /// Effect bodies currently running.
    running: usize,
    /// Set once the tree's root is torn down; no body may start after it.
    closed: bool,
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

    /// Marks an effect body as running until the returned guard drops, or
    /// returns `None` if the tree has been torn down and the body must not
    /// run.
    pub(crate) fn enter(self: &Arc<Self>) -> Option<RunGuard> {
        {
            let mut state = self.state.lock().or_poisoned();
            if state.closed {
                return None;
            }
            state.running += 1;
        }
        ENTERED.with_borrow_mut(|entered| entered.push(self.id()));
        Some(RunGuard(Arc::clone(self)))
    }

    /// Blocks until no effect body of this tree is running, or until
    /// [`IDLE_TIMEOUT`] passes. Returns at once if the current thread is
    /// itself inside one of this tree's effect bodies.
    pub(crate) fn wait_idle(self: &Arc<Self>) {
        drop(self.idle_state());
    }

    /// [`Activity::wait_idle`], then closes the tracker so no effect body of
    /// the tree starts afterwards. For the teardown of a tree's root.
    pub(crate) fn close(self: &Arc<Self>) {
        self.idle_state().closed = true;
    }

    fn idle_state(self: &Arc<Self>) -> MutexGuard<'_, State> {
        let state = self.state.lock().or_poisoned();
        let id = self.id();
        if state.running == 0
            || ENTERED.with_borrow(|entered| entered.contains(&id))
        {
            return state;
        }
        // A body that unwound has already released its guard, so a poisoned
        // lock carries nothing worth propagating; either way the wait ends.
        self.idle
            .wait_timeout_while(state, IDLE_TIMEOUT, |state| state.running > 0)
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .0
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
        let mut state = self.0.state.lock().or_poisoned();
        state.running -= 1;
        if state.running == 0 {
            self.0.idle.notify_all();
        }
    }
}
