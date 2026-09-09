//! The browser side of the local-first list document (spec section 3).
//!
//! `routes/list_view_sync.rs` is the call site: it opens a `handle`, renders
//! through `adapter`, runs `sync` against the realtime socket and installs
//! `undo`'s key bindings. Those call sites are all inside that page's
//! `#[cfg(feature = "hydrate")]` block, because a document only exists in a
//! browser tab — `ListDocHandle::open` builds `StoredValue::new_local`s that
//! must never be constructed on the SSR half (repo issue #1332). So on the
//! SSR build this module really has no callers, and only there is the
//! dead-code lint silenced. The unconditional `#![allow(dead_code)]` this
//! module carried while it was being built in isolation is gone: on the
//! hydrate target — the one that actually ships this code — dead items are
//! now errors again.
#![cfg_attr(not(feature = "hydrate"), allow(dead_code))]

pub mod adapter;
pub mod handle;
pub mod store;
pub mod sync;
pub mod undo;
