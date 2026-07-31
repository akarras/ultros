//! Search overlay open/closed state.
//!
//! Deliberately **not** persisted: an overlay that restored itself as open
//! on page load would trap the user behind a modal on every navigation.
//! Contrast [`SideNavSettings::collapsed`](super::side_nav::SideNavSettings),
//! which is cookie-backed on purpose.

use leptos::prelude::*;

/// Shared open state for the search overlay. Every trigger — the sidebar
/// row, the mobile bar button, and the `Cmd`/`Ctrl`+K hotkey — flips this
/// one signal, so they can never disagree about whether the overlay is up.
#[derive(Clone, Copy)]
pub struct SearchOverlayState {
    pub open: RwSignal<bool>,
}

impl SearchOverlayState {
    fn new() -> Self {
        Self {
            open: RwSignal::new(false),
        }
    }

    /// Flip the overlay open or closed.
    pub fn toggle(&self) {
        self.open.update(|v| *v = !*v);
    }

    /// Force the overlay closed. Safe to call when already closed.
    pub fn close(&self) {
        self.open.set(false);
    }
}

/// Provide `SearchOverlayState` into context if absent, and return it.
pub fn provide_search_overlay_state() -> SearchOverlayState {
    if let Some(existing) = use_context::<SearchOverlayState>() {
        return existing;
    }
    let state = SearchOverlayState::new();
    provide_context(state);
    state
}

/// Retrieve `SearchOverlayState` from context. Panics if not provided.
pub fn use_search_overlay_state() -> SearchOverlayState {
    use_context::<SearchOverlayState>().expect("SearchOverlayState not provided")
}

#[cfg(test)]
mod tests {
    use super::SearchOverlayState;

    #[test]
    fn starts_closed() {
        let state = SearchOverlayState::new();
        assert!(!state.open.get_untracked());
    }

    #[test]
    fn toggle_flips_open() {
        let state = SearchOverlayState::new();
        state.toggle();
        assert!(state.open.get_untracked());
        state.toggle();
        assert!(!state.open.get_untracked());
    }

    #[test]
    fn close_is_idempotent() {
        let state = SearchOverlayState::new();
        state.close();
        assert!(!state.open.get_untracked());
        state.toggle();
        state.close();
        state.close();
        assert!(!state.open.get_untracked());
    }
}
