//! Client platform detection for hotkey labels (`⌘K` vs `Ctrl K`).
//!
//! The server can't know the visitor's OS without rendering per-User-Agent
//! HTML, which would defeat caching and invite hydration mismatches. So SSR
//! always renders the non-Apple label, and a client Effect flips the signal
//! after hydration on Apple platforms. Effects never run during hydration,
//! so the swap can't desync the server HTML from the hydrating client.

use leptos::prelude::*;

/// Whether the client is an Apple platform (macOS/iOS/iPadOS), where the
/// search hotkey is `⌘K` rather than `Ctrl K`. Defaults to `false` on the
/// server and until detection runs on the client.
#[derive(Clone, Copy)]
pub struct PlatformHotkeys {
    pub apple: RwSignal<bool>,
}

impl PlatformHotkeys {
    fn new() -> Self {
        Self {
            apple: RwSignal::new(false),
        }
    }
}

/// `navigator.platform` is deprecated but still the most direct signal;
/// modern iPads report `MacIntel`, which lands on the right answer anyway.
/// The user-agent fallback must match `Macintosh`, never a bare `Mac` —
/// iPhone UAs contain the string `like Mac OS X`.
#[cfg(feature = "hydrate")]
fn is_apple_platform() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let navigator = window.navigator();
    if let Ok(platform) = navigator.platform()
        && !platform.is_empty()
    {
        return platform.starts_with("Mac") || platform.starts_with("iP");
    }
    navigator
        .user_agent()
        .is_ok_and(|ua| ua.contains("Macintosh") || ua.contains("iPhone") || ua.contains("iPad"))
}

/// Provide [`PlatformHotkeys`] into context if absent, and return it.
pub fn provide_platform_hotkeys() -> PlatformHotkeys {
    if let Some(existing) = use_context::<PlatformHotkeys>() {
        return existing;
    }
    let state = PlatformHotkeys::new();
    provide_context(state);
    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        if is_apple_platform() {
            state.apple.set(true);
        }
    });
    state
}

/// Retrieve [`PlatformHotkeys`] from context. Panics if not provided.
pub fn use_platform_hotkeys() -> PlatformHotkeys {
    use_context::<PlatformHotkeys>().expect("PlatformHotkeys not provided")
}
