//! Shared URL builder for the tools that put the selected world in the path.
//!
//! Flip Finder, Vendor Resale and Market Trends all render a world picker
//! whose `Effect` navigates to `/<tool>/<world>` when the pick changes. That
//! effect also runs once on mount, so a navigation that forgets the query
//! string doesn't just lose filters on a world switch — it wipes them out of
//! a shared or bookmarked link the moment the page hydrates.
//! `leptos_router`'s navigate builds the next URL purely from the path it is
//! handed, so the query has to be carried across explicitly.

use leptos_router::params::ParamsMap;

/// Where a world picker should navigate, or `None` if it is already there.
///
/// Returning `None` for the no-op case matters as much as building the URL
/// correctly: the effect runs on mount, and `query_signal`'s navigation is
/// deferred to an animation frame. A redundant navigate on mount pushes a
/// duplicate history entry (so Back appears dead) and re-sets the router's
/// URL underneath any filter write that is still in flight.
///
/// `ParamsMap::to_query_string` already emits the leading `?` for a non-empty
/// map (and `""` when empty), so this must not add one — `/trends/World??cat=8`
/// parses the key as `?cat` and silently drops the filter on reload.
pub fn world_nav_url(
    base: &str,
    world: &str,
    current_path: &str,
    query: &ParamsMap,
) -> Option<String> {
    let path = format!("{base}/{world}");
    if path == current_path {
        return None;
    }
    Some(format!("{path}{}", query.to_query_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_world_keeps_a_bare_path_when_there_are_no_filters() {
        let query = ParamsMap::new();
        assert_eq!(
            world_nav_url("/trends", "Gilgamesh", "/trends/Adamantoise", &query).as_deref(),
            Some("/trends/Gilgamesh")
        );
    }

    /// The regression behind issue #1053: filters live in the query string and
    /// the trends navigator dropped them, so a shared link lost its filters and
    /// a world switch reset them.
    #[test]
    fn filters_survive_a_world_switch() {
        let mut query = ParamsMap::new();
        query.insert("category", "10".to_string());
        assert_eq!(
            world_nav_url("/trends", "Gilgamesh", "/trends/Adamantoise", &query).as_deref(),
            Some("/trends/Gilgamesh?category=10")
        );
    }

    /// The other half of #1053: this effect also runs on mount, where the world
    /// is already the one in the path. Navigating there again wiped the query.
    #[test]
    fn already_on_the_world_is_not_a_navigation() {
        let mut query = ParamsMap::new();
        query.insert("category", "10".to_string());
        assert_eq!(
            world_nav_url("/trends", "Gilgamesh", "/trends/Gilgamesh", &query),
            None
        );
    }

    /// A path that differs only in case is still a navigation — it canonicalizes
    /// the world name the user typed.
    #[test]
    fn differing_case_still_navigates() {
        let query = ParamsMap::new();
        assert!(world_nav_url("/trends", "Gilgamesh", "/trends/gilgamesh", &query).is_some());
    }

    /// A doubled `?` is the failure mode a hand-written `format!("…?{query}")`
    /// falls into once `to_query_string` supplies its own.
    ///
    /// Keys here are deliberately alphanumeric: `Url::escape` percent-encodes
    /// with `NON_ALPHANUMERIC` under `ssr` but uses `encodeURIComponent` on
    /// wasm, so a key like `min_price` is spelled `min%5Fprice` in this test
    /// binary and `min_price` in the browser. Asserting on either spelling
    /// would be asserting on which half of that `cfg` got compiled.
    #[test]
    fn never_emits_a_double_question_mark() {
        let mut query = ParamsMap::new();
        query.insert("category", "10".to_string());
        query.insert("sort", "vwap".to_string());
        let url = world_nav_url(
            "/vendor-resale",
            "Adamantoise",
            "/vendor-resale/Cerberus",
            &query,
        )
        .expect("a different world is a navigation");
        assert_eq!(url.matches('?').count(), 1);
        assert!(url.starts_with("/vendor-resale/Adamantoise?"));
        assert!(url.contains("category=10"), "{url}");
        assert!(url.contains("sort=vwap"), "{url}");
    }
}
