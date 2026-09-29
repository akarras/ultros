//! Detects when the wasm bundle running in this tab is older than the
//! server it is talking to.
//!
//! The server stamps every response with `x-ultros-commit` (see
//! `app_commit_header_layer` in `ultros/src/web.rs`). The client fetch
//! helpers in `crate::api` pass that header to [`observe_server_commit`],
//! which records the first mismatch in [`AppUpdate::pending`]. The state is
//! monotonic: once set it stays set until the page reloads, so a proxy
//! hiccup or a transient error page can never flip it back or nag twice.
//!
//! Spec: `docs/superpowers/specs/2026-09-05-app-update-system-design.md`.

use leptos::prelude::*;

// The decision logic is only reachable from the client-side observer and the
// unit tests; the SSR build would flag it as dead code otherwise.

#[cfg(any(not(feature = "ssr"), test))]
const DIRTY: &str = "dirty";

/// Whether the server's reported commit means this client is stale.
///
/// Conservative on purpose: a missing or empty header (older server, proxy
/// error page, connection dropped mid-deploy) and a `dirty` hash on either
/// side (local build without git) both mean "don't know", which is treated
/// as "not stale" so the app never nags without evidence.
#[cfg(any(not(feature = "ssr"), test))]
pub fn update_pending(client: &str, server: Option<&str>) -> bool {
    let Some(server) = server.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    if client == DIRTY || server == DIRTY {
        return false;
    }
    client != server
}

/// Whether a response's `Cache-Control` lets the browser store and replay it.
///
/// A replayed response carries the `x-ultros-commit` it was *stored* with, so
/// right after a deploy the fresh bundle reads the previous commit off it and
/// decides it is the stale one. Reloading replays the same entry, so the
/// banner comes straight back until the entry expires (up to an hour for the
/// price-history endpoints). Only responses the browser cannot have served
/// from its cache are trusted as evidence of the server's commit.
#[cfg(any(not(feature = "ssr"), test))]
pub fn may_be_cached_replay(cache_control: Option<&str>) -> bool {
    let Some(cache_control) = cache_control else {
        return false;
    };
    let mut cacheable = false;
    for directive in cache_control.split(',') {
        let directive = directive.trim().to_ascii_lowercase();
        let (name, value) = directive
            .split_once('=')
            .map_or((directive.as_str(), ""), |(n, v)| (n.trim(), v.trim()));
        match name {
            "no-store" => return false,
            "max-age" if value.trim_matches('"').parse::<u64>().is_ok_and(|s| s > 0) => {
                cacheable = true;
            }
            "stale-while-revalidate" | "stale-if-error" | "immutable" => cacheable = true,
            _ => {}
        }
    }
    cacheable
}

/// App-wide update state. Provided once in `AppInner`.
#[derive(Clone, Copy)]
pub struct AppUpdate {
    /// The first server commit seen that differs from the client commit supplied at initialization.
    /// Never cleared until the page reloads.
    pub pending: RwSignal<Option<String>>,
    /// The user closed the banner. Reload-on-navigation still applies.
    pub dismissed: RwSignal<bool>,
}

impl AppUpdate {
    pub fn banner_visible(&self) -> bool {
        self.pending.get().is_some() && !self.dismissed.get()
    }
}

// The fetch helpers run inside `spawn_local` with no guaranteed reactive
// owner, so the client keeps a thread-local handle to the signal. wasm is
// single-threaded, so this is exactly one slot per tab. It is compiled out of
// the SSR build on purpose: a thread-local on the server would leak state
// across requests sharing a worker thread.
#[cfg(not(feature = "ssr"))]
thread_local! {
    static PENDING: std::cell::RefCell<Option<(&'static str, RwSignal<Option<String>>)>> =
        const { std::cell::RefCell::new(None) };
}

pub fn provide_app_update_context(_client_commit: &'static str) -> AppUpdate {
    let update = AppUpdate {
        pending: RwSignal::new(None),
        dismissed: RwSignal::new(false),
    };
    provide_context(update);
    #[cfg(not(feature = "ssr"))]
    PENDING.with(|slot| *slot.borrow_mut() = Some((_client_commit, update.pending)));
    update
}

pub fn use_app_update() -> Option<AppUpdate> {
    use_context::<AppUpdate>()
}

/// Called by every client fetch helper with the raw `x-ultros-commit` header.
/// Records the first mismatch and ignores everything after that.
#[cfg(not(feature = "ssr"))]
pub fn observe_server_commit(header: Option<&str>) {
    let Some(server) = header else {
        return;
    };
    PENDING.with(|slot| {
        if let Some((client_commit, pending)) = slot.borrow().as_ref() {
            if !update_pending(client_commit, Some(server)) {
                return;
            }
            if pending.get_untracked().is_none() {
                pending.set(Some(server.trim().to_string()));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{may_be_cached_replay, update_pending};

    #[test]
    fn same_commit_is_not_pending() {
        assert!(!update_pending("283f84e5", Some("283f84e5")));
    }

    #[test]
    fn different_commit_is_pending() {
        assert!(update_pending("283f84e5", Some("fdf6a8cb")));
    }

    #[test]
    fn missing_or_empty_header_is_not_pending() {
        assert!(!update_pending("283f84e5", None));
        assert!(!update_pending("283f84e5", Some("")));
        assert!(!update_pending("283f84e5", Some("   ")));
    }

    #[test]
    fn dirty_on_either_side_is_not_pending() {
        assert!(!update_pending("dirty", Some("283f84e5")));
        assert!(!update_pending("283f84e5", Some("dirty")));
        assert!(!update_pending("dirty", Some("dirty")));
    }

    #[test]
    fn header_whitespace_is_ignored() {
        assert!(!update_pending("283f84e5", Some(" 283f84e5 ")));
    }

    #[test]
    fn uncacheable_responses_are_trusted() {
        assert!(!may_be_cached_replay(None));
        assert!(!may_be_cached_replay(Some("private, no-store")));
        assert!(!may_be_cached_replay(Some("no-cache")));
        assert!(!may_be_cached_replay(Some("max-age=0")));
        assert!(!may_be_cached_replay(Some(
            "public, max-age=3600, no-store"
        )));
    }

    #[test]
    fn cacheable_responses_are_ignored() {
        // The shapes the API actually sends (changelog, price history, bulk stats).
        assert!(may_be_cached_replay(Some("public, max-age=300")));
        assert!(may_be_cached_replay(Some("max-age=15")));
        assert!(may_be_cached_replay(Some(
            "public, max-age=300, s-maxage=300, stale-while-revalidate=1800"
        )));
        assert!(may_be_cached_replay(Some("Private, Max-Age=604800")));
    }
}
