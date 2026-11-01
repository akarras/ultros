use or_poisoned::OrPoisoned;
use slotmap::{new_key_type, SlotMap};
#[cfg(feature = "sandboxed-arenas")]
use std::cell::RefCell;
#[cfg(not(feature = "sandboxed-arenas"))]
use std::sync::OnceLock;
#[cfg(feature = "sandboxed-arenas")]
use std::sync::Weak;
use std::{
    any::Any,
    hash::Hash,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
};

/// Diagnostic counters for arena bookkeeping, used to chase SSR arena
/// retention (see `ULTROS_PATCH.md`). Cheap relaxed atomics; read them with
/// [`arena_diagnostics`].
pub(crate) static ARENA_INSERTS: AtomicU64 = AtomicU64::new(0);
pub(crate) static ARENA_INSERTS_UNREGISTERED: AtomicU64 = AtomicU64::new(0);
pub(crate) static ARENA_CLEANUP_REMOVES: AtomicU64 = AtomicU64::new(0);
pub(crate) static ARENA_CLEANUP_REMOVE_MISSES: AtomicU64 = AtomicU64::new(0);
pub(crate) static ARENA_EXPLICIT_REMOVES: AtomicU64 = AtomicU64::new(0);

/// Snapshot of the arena diagnostic counters: `(inserts,
/// inserts_without_an_owner, cleanup_removes, cleanup_remove_misses,
/// explicit_removes)`. A cleanup remove miss means the node was not in
/// the cleaning owner's arena: either it had already been removed
/// explicitly (`dispose`/`take`, counted in `explicit_removes`) or it was
/// inserted into a different arena than the one its registering owner
/// cleans up, in which case it stays there. Retained nodes over a window
/// are `inserts - cleanup_removes - explicit_removes`.
#[doc(hidden)]
pub fn arena_diagnostics() -> (u64, u64, u64, u64, u64) {
    (
        ARENA_INSERTS.load(Ordering::Relaxed),
        ARENA_INSERTS_UNREGISTERED.load(Ordering::Relaxed),
        ARENA_CLEANUP_REMOVES.load(Ordering::Relaxed),
        ARENA_CLEANUP_REMOVE_MISSES.load(Ordering::Relaxed),
        ARENA_EXPLICIT_REMOVES.load(Ordering::Relaxed),
    )
}

new_key_type! {
    /// Unique identifier for an item stored in the arena.
    pub struct NodeId;
}

pub struct Arena;

pub type ArenaMap = SlotMap<NodeId, Box<dyn Any + Send + Sync>>;

#[cfg(not(feature = "sandboxed-arenas"))]
static MAP: OnceLock<RwLock<ArenaMap>> = OnceLock::new();
#[cfg(feature = "sandboxed-arenas")]
thread_local! {
    pub(crate) static MAP: RefCell<Option<Weak<RwLock<ArenaMap>>>> = RefCell::new(Some(Default::default()));
}

impl Arena {
    #[inline(always)]
    #[allow(unused)]
    pub fn set(arena: &Arc<RwLock<ArenaMap>>) {
        #[cfg(feature = "sandboxed-arenas")]
        {
            let new_arena = Arc::downgrade(arena);
            MAP.with_borrow_mut(|arena| {
                *arena = Some(new_arena);
            })
        }
    }

    /// Sets the current arena, returning the previously-active one so the
    /// caller can restore it with [`Arena::restore`]. Scoped setters
    /// (`Owner::with`, `Sandboxed` polls) use this so an arena activated
    /// for one scope does not leak into whatever runs next on the thread:
    /// a leaked arena/owner pair from *different* scopes is how a value
    /// ends up inserted into one request's arena while registered for
    /// cleanup against another's (which then can never remove it).
    #[cfg(feature = "sandboxed-arenas")]
    pub(crate) fn replace(
        arena: &Arc<RwLock<ArenaMap>>,
    ) -> Option<Weak<RwLock<ArenaMap>>> {
        MAP.with_borrow_mut(|prev| {
            std::mem::replace(prev, Some(Arc::downgrade(arena)))
        })
    }

    /// Restores an arena previously returned by [`Arena::replace`].
    ///
    /// Only a still-live previous arena is restored. If it is gone (the
    /// common case at a root scope, where the previous value is the
    /// thread's dead default), the current arena stays active: existing
    /// code reads values after `Owner::with` returns (e.g. leptos_meta
    /// title closures evaluated during meta injection), and those reads
    /// resolve through the thread-local arena. Clearing it would turn
    /// their graceful disposed-value handling into "no Arena is active"
    /// panics. The cross-request pairing this guards against happens
    /// between *nested live* scopes, where the previous arena is alive
    /// and is restored.
    #[cfg(feature = "sandboxed-arenas")]
    pub(crate) fn restore(prev: Option<Weak<RwLock<ArenaMap>>>) {
        if let Some(prev) = prev {
            if prev.upgrade().is_some() {
                MAP.with_borrow_mut(|arena| *arena = Some(prev));
            }
        }
    }

    #[track_caller]
    pub fn with<U>(fun: impl FnOnce(&ArenaMap) -> U) -> U {
        #[cfg(not(feature = "sandboxed-arenas"))]
        {
            fun(&MAP.get_or_init(Default::default).read().or_poisoned())
        }
        #[cfg(feature = "sandboxed-arenas")]
        {
            Arena::try_with(fun).unwrap_or_else(|| {
                panic!(
                    "at {}, the `sandboxed-arenas` feature is active, but no \
                     Arena is active",
                    std::panic::Location::caller()
                )
            })
        }
    }

    #[track_caller]
    pub fn try_with<U>(fun: impl FnOnce(&ArenaMap) -> U) -> Option<U> {
        #[cfg(not(feature = "sandboxed-arenas"))]
        {
            Some(fun(&MAP.get_or_init(Default::default).read().or_poisoned()))
        }
        #[cfg(feature = "sandboxed-arenas")]
        {
            MAP.with_borrow(|arena| {
                arena
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .map(|n| fun(&n.read().or_poisoned()))
            })
        }
    }

    #[track_caller]
    pub fn with_mut<U>(fun: impl FnOnce(&mut ArenaMap) -> U) -> U {
        #[cfg(not(feature = "sandboxed-arenas"))]
        {
            fun(&mut MAP.get_or_init(Default::default).write().or_poisoned())
        }
        #[cfg(feature = "sandboxed-arenas")]
        {
            Arena::try_with_mut(fun).unwrap_or_else(|| {
                panic!(
                    "at {}, the `sandboxed-arenas` feature is active, but no \
                     Arena is active",
                    std::panic::Location::caller()
                )
            })
        }
    }

    #[track_caller]
    pub fn try_with_mut<U>(fun: impl FnOnce(&mut ArenaMap) -> U) -> Option<U> {
        #[cfg(not(feature = "sandboxed-arenas"))]
        {
            Some(fun(&mut MAP
                .get_or_init(Default::default)
                .write()
                .or_poisoned()))
        }
        #[cfg(feature = "sandboxed-arenas")]
        {
            MAP.with_borrow(|arena| {
                arena
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .map(|n| fun(&mut n.write().or_poisoned()))
            })
        }
    }
}

#[cfg(feature = "sandboxed-arenas")]
pub mod sandboxed {
    use super::{Arena, ArenaMap, MAP};
    use futures::Stream;
    use pin_project_lite::pin_project;
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, RwLock, Weak},
        task::{Context, Poll},
    };

    pin_project! {
        /// A [`Future`] that restores its associated arena as the current arena whenever it is
        /// polled.
        ///
        /// Sandboxed arenas are used to ensure that data created in response to e.g., different
        /// HTTP requests can be handled separately, while providing stable identifiers for their
        /// stored values. Wrapping a `Future` in `Sandboxed` ensures that it will always use the
        /// same arena that it was created under.
        pub struct Sandboxed<T> {
            arena: Option<Arc<RwLock<ArenaMap>>>,
            #[pin]
            inner: T,
        }
    }

    impl<T> Sandboxed<T> {
        /// Wraps the given [`Future`], ensuring that any [`ArenaItem`][item] created while it is
        /// being polled will be associated with the same arena that was active when this was
        /// called.
        ///
        /// [item]:[crate::owner::ArenaItem]
        #[track_caller]
        pub fn new(inner: T) -> Self {
            let arena = MAP.with_borrow(|n| n.as_ref().and_then(Weak::upgrade));
            Self { arena, inner }
        }
    }

    impl<Fut> Future for Sandboxed<Fut>
    where
        Fut: Future,
    {
        type Output = Fut::Output;

        fn poll(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Self::Output> {
            let prev = self.arena.as_ref().map(|arena| Arena::replace(arena));
            let this = self.project();
            let result = this.inner.poll(cx);
            if let Some(prev) = prev {
                Arena::restore(prev);
            }
            result
        }
    }

    impl<T> Stream for Sandboxed<T>
    where
        T: Stream,
    {
        type Item = T::Item;

        fn poll_next(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Self::Item>> {
            let prev = self.arena.as_ref().map(|arena| Arena::replace(arena));
            let this = self.project();
            let result = this.inner.poll_next(cx);
            if let Some(prev) = prev {
                Arena::restore(prev);
            }
            result
        }
    }
}
