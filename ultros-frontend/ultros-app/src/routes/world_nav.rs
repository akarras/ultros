//! Shared URL builder for the tools that put the selected world in the path.
//!
//! Flip Finder, Vendor Resale and Market Trends all render a world picker
//! whose `Effect` navigates to `/<tool>/<world>` when the pick changes. That
//! effect also runs once on mount, so a navigation that forgets the query
//! string doesn't just lose filters on a world switch — it wipes them out of
//! a shared or bookmarked link the moment the page hydrates.
//! `leptos_router`'s navigate builds the next URL purely from the path it is
//! handed, so the query has to be carried across explicitly.

use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use leptos_router::{
    NavigateOptions,
    hooks::{use_location, use_navigate, use_params_map},
    location::Url,
    params::ParamsMap,
};
use ultros_api_types::{world::World, world_helper::WorldHelper};

use crate::global_state::{home_world::use_home_world, use_world_helper};

/// Path wins over legacy `?world=`, then the home-world cookie is the fallback.
/// An invalid explicit path never lets a conflicting query choose the market.
fn selected_analyzer_world(
    worlds: &WorldHelper,
    path: Option<&str>,
    query: Option<&str>,
    home: Option<World>,
) -> Option<World> {
    path.or(query)
        .and_then(|name| worlds.lookup_world_by_name(&Url::unescape(name)))
        .and_then(|world| world.as_world().cloned())
        .or(home)
}

/// Canonical analyzer destination, stripping only the obsolete world query.
fn analyzer_world_url(base: &str, world: &str, path: &str, query: &ParamsMap) -> Option<String> {
    let mut filters = query.clone();
    let legacy_world = filters.remove("world").is_some();
    world_nav_url(base, world, path, &filters)
        .or_else(|| legacy_world.then(|| format!("{path}{}", filters.to_query_string())))
}

/// Route-owned selection keeps reloads and Back/Forward in sync with all data
/// scopes. Compatibility/cookie navigation replaces history; picker edits push.
pub fn use_analyzer_world(
    base: &'static str,
) -> (Memo<Option<World>>, SignalSetter<Option<World>>) {
    let params = use_params_map();
    let location = use_location();
    let (home, _) = use_home_world();
    let worlds = use_world_helper().ok();
    let selected = Memo::new(move |_| {
        worlds.as_ref().and_then(|worlds| {
            selected_analyzer_world(
                worlds,
                params.get().get_str("world"),
                location.query.get().get_str("world"),
                home.get(),
            )
        })
    });
    let navigate = use_navigate();
    let navigate_to = move |world: World, replace: bool| {
        if let Some(url) = analyzer_world_url(
            base,
            &world.name,
            &location.pathname.get_untracked(),
            &location.query.get_untracked(),
        ) {
            navigate(
                &format!("{url}{}", location.hash.get_untracked()),
                NavigateOptions {
                    replace,
                    scroll: false,
                    ..Default::default()
                },
            );
        }
    };
    let canonicalize = navigate_to.clone();
    Effect::new(move |_| {
        // Track legacy query changes even when the path's world stays the same.
        location.query.with(|query| query.get("world"));
        if let Some(world) = selected.get() {
            let navigate = canonicalize.clone();
            // Let hydration's delayed storage and filter writes settle before
            // canonicalizing. The optional world route keeps the page owner
            // alive across bare/path URLs so queued storage events stay valid.
            request_animation_frame(move || {
                if selected.try_get_untracked().flatten().as_ref() == Some(&world) {
                    navigate(world, true);
                }
            });
        }
    });
    let setter = SignalSetter::map(move |world: Option<World>| {
        if let Some(world) = world {
            navigate_to(world, false);
        }
    });
    (selected, setter)
}

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
    let world = leptos_router::location::Url::escape(world);
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
    fn analyzer_compatibility_removes_world_without_losing_encoded_filters() {
        let mut query = ParamsMap::new();
        query.insert("world", "Cerberus".into());
        query.insert("filter", "ore & crystals=1 + HQ/材料".into());
        query.insert("probe", "one".into());
        query.insert("probe", "two".into());
        let mut expected = query.clone();
        expected.remove("world");
        for base in [
            "/recipe-analyzer",
            "/venture-analyzer",
            "/leve-analyzer",
            "/scrip-sources",
        ] {
            let canonical = format!("{base}/Gilgamesh");
            for path in [base, canonical.as_str()] {
                assert_eq!(
                    analyzer_world_url(base, "Gilgamesh", path, &query),
                    Some(format!("{canonical}{}", expected.to_query_string()))
                );
            }
            assert_eq!(
                analyzer_world_url(base, "Gilgamesh", &canonical, &expected),
                None
            );
        }
    }

    #[test]
    fn analyzer_selection_prefers_path_then_legacy_query_then_cookie() {
        use ultros_api_types::world::{Datacenter, Region, WorldData};
        let world = |id, name: &str| World {
            id,
            name: name.into(),
            datacenter_id: 1,
        };
        let home = world(3, "Goblin");
        let worlds = WorldHelper::new(WorldData {
            regions: vec![Region {
                id: 1,
                name: "test".into(),
                datacenters: vec![Datacenter {
                    id: 1,
                    region_id: 1,
                    name: "dc".into(),
                    worlds: vec![world(1, "Gilgamesh"), world(2, "红玉海"), home.clone()],
                }],
            }],
        });
        for (path, query, expected) in [
            (Some("Gilgamesh"), Some("红玉海"), "Gilgamesh"),
            (None, Some("Gilgamesh"), "Gilgamesh"),
            (Some("%E7%BA%A2%E7%8E%89%E6%B5%B7"), None, "红玉海"),
            (None, None, "Goblin"),
            (Some("invalid"), Some("Gilgamesh"), "Goblin"),
            (None, Some("invalid"), "Goblin"),
        ] {
            assert_eq!(
                selected_analyzer_world(&worlds, path, query, Some(home.clone()))
                    .unwrap()
                    .name,
                expected
            );
        }
        assert_eq!(selected_analyzer_world(&worlds, None, None, None), None);
    }

    #[test]
    fn encoded_world_and_query_values_survive_navigation() {
        let mut query = ParamsMap::new();
        query.insert("filter", "ore & crystals=1 + HQ/材料".into());
        let url = world_nav_url(
            "/fc-crafting-analyzer",
            "陆行鸟",
            "/fc-crafting-analyzer/Goblin",
            &query,
        )
        .unwrap();
        let (path, search) = url.split_once('?').unwrap();
        assert_eq!(path, "/fc-crafting-analyzer/%E9%99%86%E8%A1%8C%E9%B8%9F");
        assert_eq!(
            leptos_router::location::Url::unescape(search),
            "filter=ore & crystals=1 + HQ/材料"
        );
        assert_eq!(
            world_nav_url("/fc-crafting-analyzer", "陆行鸟", path, &query),
            None
        );
    }

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
