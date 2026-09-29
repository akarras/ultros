//! Sidebar collapse + mobile drawer state.
//!
//! Mirrors the `ThemeSettings` pattern in [`super::theme`]: an RwSignal
//! persisted to a cookie via the [`Cookies`](super::cookies::Cookies) helper.

use leptos::prelude::*;

use crate::global_state::cookies::Cookies;

const COOKIE_NAME: &str = "side_nav_collapsed";

fn parse_collapsed(s: &str) -> Option<bool> {
    match s {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Sidebar state.
///
/// - `collapsed`: desktop icon-only mode. Persisted to the `side_nav_collapsed`
///   cookie via the `Effect` in `SideNavSettings::new`.
/// - `drawer_open`: mobile overlay drawer. Intentionally **not** persisted —
///   the drawer should reset to closed on every page load.
#[derive(Clone, Copy)]
pub struct SideNavSettings {
    pub collapsed: RwSignal<bool>,
    pub drawer_open: RwSignal<bool>,
}

impl SideNavSettings {
    fn new() -> Self {
        let initial_collapsed = load_collapsed_from_cookie().unwrap_or(false);
        let collapsed = RwSignal::new(initial_collapsed);
        let drawer_open = RwSignal::new(false);

        let settings = SideNavSettings {
            collapsed,
            drawer_open,
        };

        // Persist collapse changes to cookie.
        Effect::new(move |_| {
            let value = collapsed.get();
            if let Some(cookies) = use_context::<Cookies>() {
                let (_, set_cookie) = cookies.use_cookie_typed::<_, String>(COOKIE_NAME);
                set_cookie(Some(if value { "true".into() } else { "false".into() }));
            }
        });

        settings
    }
}

fn load_collapsed_from_cookie() -> Option<bool> {
    let cookies = use_context::<Cookies>()?;
    let (sig, _setter) = cookies.use_cookie_typed::<_, String>(COOKIE_NAME);
    let value = sig.get_untracked()?;
    parse_collapsed(&value)
}

/// Provide `SideNavSettings` into context if not already present, and return it.
pub fn provide_side_nav_settings() -> SideNavSettings {
    if let Some(existing) = use_context::<SideNavSettings>() {
        return existing;
    }
    let settings = SideNavSettings::new();
    provide_context(settings);
    settings
}

/// Retrieve `SideNavSettings` from context. Panics if not provided.
pub fn use_side_nav_settings() -> SideNavSettings {
    use_context::<SideNavSettings>().expect("SideNavSettings not provided")
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_collapsed_true() {
        assert_eq!(super::parse_collapsed("true"), Some(true));
    }

    #[test]
    fn parse_collapsed_false() {
        assert_eq!(super::parse_collapsed("false"), Some(false));
    }

    #[test]
    fn parse_collapsed_garbage() {
        assert_eq!(super::parse_collapsed("yes"), None);
        assert_eq!(super::parse_collapsed(""), None);
    }
}
