//! Automatic, device-local analyzer preferences. Explicit links always win.
use leptos_router::{location::Url, params::ParamsMap};

const MAX_QUERY_BYTES: usize = 16_384;
#[cfg(any(feature = "hydrate", test))]
const MAX_COOKIE_BYTES: usize = 3_500;

pub use ultros_ui_grid::view_policy::analyzer;

fn parse(query: &str) -> ParamsMap {
    ultros_ui_grid::view_policy::parse_query(query)
}

fn world_is_context(path: &str) -> bool {
    analyzer(path).is_some_and(|tool| tool != "flip-finder")
}

pub fn is_bare(path: &str, query: &str) -> bool {
    parse(query)
        .into_iter()
        .all(|(key, _)| key == "lang" || (key == "world" && world_is_context(path)))
}

/// A new bare entry has not chosen a view yet. Recording it before the route
/// mounts would create an empty preference that suppresses its landing seed.
/// In-place Clear is different: remember that deliberately empty view.
fn should_remember_view(path: &str, query: &str, entering: bool) -> bool {
    !entering || !is_bare(path, query)
}

fn saved_query(path: &str, query: &str) -> Option<String> {
    if query.len() > MAX_QUERY_BYTES {
        return None;
    }
    let mut map = parse(query);
    map.remove("lang");
    if world_is_context(path) {
        map.remove("world");
    }
    // An explicitly empty view is still a preference. This also prevents
    // landing defaults and the restore redirect from running a second time.
    map.remove("v");
    map.insert("v", "1".to_string());
    Some(map.to_query_string())
}

fn restore(path: &str, current: &str, saved: &str) -> Option<String> {
    if !is_bare(path, current) {
        return None;
    }
    let mut map = parse(&saved_query(path, saved)?);
    for (key, value) in parse(current) {
        map.insert(key, Url::escape(&value));
    }
    Some(format!("{path}{}", map.to_query_string()))
}

/// Shared with the HTTP middleware: resolve before rendering so SSR and
/// hydration both see the same filters, columns and resource keys.
pub fn cookie_redirect(path: &str, query: &str, header: &str) -> Option<String> {
    let tool = analyzer(path)?;
    // An explicitly chosen default wins over the most recently inspected
    // view. Match client-side restore order before SSR resources are fetched.
    let default_name = format!("ultros_default_{tool}");
    if let Some(default) = cookie::Cookie::split_parse_encoded(header)
        .filter_map(Result::ok)
        .find(|c| c.name() == default_name)
    {
        return (default.value() != ultros_ui_grid::view_policy::LOCAL_DEFAULT_COOKIE_VALUE)
            .then(|| restore(path, query, default.value()))
            .flatten();
    }
    let last_name = format!("ultros_last_{tool}");
    cookie::Cookie::split_parse_encoded(header)
        .filter_map(Result::ok)
        .find(|c| c.name() == last_name)
        .and_then(|c| restore(path, query, c.value()))
}

#[cfg(any(feature = "hydrate", test))]
fn preference_cookie(tool: &str, query: &str) -> cookie::Cookie<'static> {
    let mut c = cookie::Cookie::new(format!("ultros_last_{tool}"), query.to_string());
    c.set_path(format!("/{tool}"));
    c.set_same_site(cookie::SameSite::Lax);
    c.set_max_age(time::Duration::days(365));
    // Oversized views stay in localStorage. Expire the old cookie rather than
    // restoring a stale preference or exceeding browser cookie/header limits.
    if c.encoded().to_string().len() > MAX_COOKIE_BYTES {
        c.set_value("");
        c.set_max_age(time::Duration::ZERO);
    }
    c
}

#[cfg(feature = "hydrate")]
fn local_saved(tool: &str) -> Option<String> {
    if let Some(default) = ultros_ui_grid::view_policy::saved_default_query(tool) {
        return Some(default);
    }
    leptos::prelude::window()
        .local_storage()
        .ok()
        .flatten()?
        .get_item(&format!("ultros.last-view.{tool}"))
        .ok()
        .flatten()
        .filter(|q| q.len() <= MAX_QUERY_BYTES)
}

/// Used by landing defaults before their effects are queued.
pub fn has_restorable_view() -> bool {
    use leptos::prelude::*;
    let location = crate::components::app_link::use_location_or_default();
    let path = location.pathname.get_untracked();
    let query = location.query.get_untracked().to_query_string();
    if !is_bare(&path, &query) {
        return false;
    }
    #[cfg(feature = "hydrate")]
    if let Some(tool) = analyzer(&path) {
        return local_saved(tool).is_some();
    }
    let _ = path;
    false
}

/// Mounted once in the shell. Restore only on entry, never while clearing
/// filters on an already-open analyzer. Storage failures remain nonfatal.
pub fn track_last_view() {
    #[cfg(feature = "hydrate")]
    {
        use crate::query_defaults::query_signal_or_default;
        use leptos::prelude::*;
        use wasm_bindgen::JsCast;
        let location = leptos_router::hooks::use_location();
        let previous = StoredValue::new(String::new());
        Effect::new(move |_| {
            let path = location.pathname.get();
            let query = location.query.get().to_query_string();
            let entering = previous.get_value() != path;
            previous.set_value(path.clone());
            let Some(tool) = analyzer(&path) else {
                return;
            };
            if entering
                && let Some(saved) = local_saved(tool)
                && let Some(target) = restore(&path, &query, &saved)
            {
                let query = target.split_once('?').map(|(_, q)| q).unwrap_or_default();
                for (key, value) in parse(query) {
                    let (_, setter) = query_signal_or_default::<String>(
                        key,
                        leptos_router::NavigateOptions {
                            replace: true,
                            scroll: false,
                            ..Default::default()
                        },
                    );
                    // The router's query mutation applies ParamsMap::replace,
                    // which decodes its input just like ParamsMap::insert.
                    setter.set(Some(Url::escape(&value)));
                }
                return;
            }
            if should_remember_view(&path, &query, entering)
                && let Some(saved) = saved_query(&path, &query)
            {
                if let Ok(Some(storage)) = window().local_storage() {
                    let _ = storage.set_item(&format!("ultros.last-view.{tool}"), &saved);
                }
                if let Ok(document) = document().dyn_into::<web_sys::HtmlDocument>() {
                    let _ =
                        document.set_cookie(&preference_cookie(tool, &saved).encoded().to_string());
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_percent_literals_survive_cookie_restore_and_context_copying() {
        let query = "?name=%2520%20%2526";
        let saved = saved_query("/items", query).unwrap();
        let cookie = preference_cookie("items", &saved).encoded().to_string();
        let target =
            cookie_redirect("/items/category/1", "world=Zone%2520&lang=ja", &cookie).unwrap();
        let restored = parse(target.split_once('?').unwrap().1);
        assert_eq!(restored.get("name").as_deref(), Some("%20 %26"));
        assert_eq!(restored.get("world").as_deref(), Some("Zone%20"));
        assert_eq!(
            saved_query("/items", &saved).as_deref(),
            Some(saved.as_str())
        );
    }
    #[test]
    fn a_cold_bare_entry_cannot_become_an_empty_preference_before_seeding() {
        for path in [
            "/recipe-analyzer/Gilgamesh",
            "/currency-exchange",
            "/currency-exchange/28",
            "/items/category/1",
        ] {
            assert!(!should_remember_view(path, "", true), "{path}");
            assert!(
                !should_remember_view(path, "?world=Gilgamesh&lang=ja", true),
                "{path}"
            );
            assert!(
                should_remember_view(path, "?v=1", true),
                "explicit empty {path}"
            );
            assert!(
                should_remember_view(path, "?profit=1", true),
                "shared {path}"
            );
            assert!(should_remember_view(path, "", false), "Clear {path}");
        }
    }
    #[test]
    fn chosen_default_precedes_last_view_and_explicit_links_precede_both() {
        let default = cookie::Cookie::new("ultros_default_recipe-analyzer", "?profit=123&v=1")
            .encoded()
            .to_string();
        let last = preference_cookie("recipe-analyzer", "?profit=999")
            .encoded()
            .to_string();
        for header in [format!("{last}; {default}"), format!("{default}; {last}")] {
            let restored =
                cookie_redirect("/recipe-analyzer/Gilgamesh", "lang=ja", &header).unwrap();
            assert_eq!(
                parse(restored.split_once('?').unwrap().1)
                    .get("profit")
                    .as_deref(),
                Some("123")
            );
            assert!(cookie_redirect("/recipe-analyzer/Gilgamesh", "profit=1", &header).is_none());
            assert!(cookie_redirect("/recipe-analyzer/Gilgamesh", "v=1", &header).is_none());
        }
        let large = format!(
            "ultros_default_recipe-analyzer={}; {last}",
            ultros_ui_grid::view_policy::LOCAL_DEFAULT_COOKIE_VALUE
        );
        assert!(
            cookie_redirect("/recipe-analyzer/Gilgamesh", "", &large).is_none(),
            "a large local default must not be replaced by the last-view cookie"
        );
    }

    #[test]
    fn new_tool_preferences_restore_without_overwriting_current_market() {
        for tool in ["trends", "items", "currency-exchange"] {
            let cookie = preference_cookie(tool, "?world=Goblin&gf=filters")
                .encoded()
                .to_string();
            let path = format!("/{tool}");
            let restored = cookie_redirect(&path, "world=Gilgamesh&lang=ja", &cookie).unwrap();
            let params = parse(restored.split_once('?').unwrap().1);
            assert_eq!(params.get("world").as_deref(), Some("Gilgamesh"));
            assert_eq!(params.get("gf").as_deref(), Some("filters"));
            assert_eq!(params.get("lang").as_deref(), Some("ja"));
        }
    }
    #[test]
    fn shared_links_win_and_world_and_language_stay_current() {
        let cookie = preference_cookie("recipe-analyzer", "?profit=10&l=2~~profit.2s")
            .encoded()
            .to_string();
        let restored =
            cookie_redirect("/recipe-analyzer", "world=Gilgamesh&lang=ja", &cookie).unwrap();
        let q = parse(restored.split_once('?').unwrap().1);
        assert_eq!(q.get("profit").as_deref(), Some("10"));
        assert_eq!(q.get("world").as_deref(), Some("Gilgamesh"));
        assert_eq!(q.get("lang").as_deref(), Some("ja"));
        assert!(cookie_redirect("/recipe-analyzer", "profit=20", &cookie).is_none());
        assert!(cookie_redirect("/recipe-analyzer", "v=1", &cookie).is_none());
        assert!(cookie_redirect("/leve-analyzer", "", &cookie).is_none());
        assert!(cookie_redirect("/item/1/2", "", &cookie).is_none());
        assert!(cookie_redirect("//recipe-analyzer", "", &cookie).is_none());
        assert!(!is_bare("/flip-finder/Gilgamesh", "world=Goblin"));
        assert!(
            saved_query("/flip-finder/Gilgamesh", "world=Goblin")
                .unwrap()
                .contains("world=Goblin")
        );
    }
    #[test]
    fn empty_views_and_cookie_limits() {
        let once = saved_query("/recipe-analyzer", "?v=1&profit=20").unwrap();
        assert_eq!(
            saved_query("/recipe-analyzer", &once).as_deref(),
            Some(once.as_str())
        );
        assert_eq!(
            saved_query("/recipe-analyzer", "?v=1&v=1"),
            Some("?v=1".into())
        );
        assert_eq!(
            saved_query("/recipe-analyzer", "?world=A&lang=ja"),
            Some("?v=1".into())
        );
        let cookie = preference_cookie("flip-finder", &"界".repeat(1200));
        assert_eq!(cookie.max_age(), Some(time::Duration::ZERO));
        assert!(cookie.encoded().to_string().len() < MAX_COOKIE_BYTES);
        assert!(saved_query("/recipe-analyzer", &"x".repeat(MAX_QUERY_BYTES + 1)).is_none());
    }

    #[test]
    fn legacy_preferences_restore_filters_on_the_current_world_path() {
        for tool in [
            "recipe-analyzer",
            "venture-analyzer",
            "leve-analyzer",
            "scrip-sources",
        ] {
            let cookie = preference_cookie(tool, "?world=Cerberus&profit=20")
                .encoded()
                .to_string();
            let path = format!("/{tool}/Gilgamesh");
            let restored = cookie_redirect(&path, "lang=ja", &cookie).unwrap();
            assert!(restored.starts_with(&format!("{path}?")));
            let query = parse(restored.split_once('?').unwrap().1);
            assert_eq!(query.get("world"), None);
            assert_eq!(query.get("profit").as_deref(), Some("20"));
            assert_eq!(query.get("lang").as_deref(), Some("ja"));
        }
    }
}
