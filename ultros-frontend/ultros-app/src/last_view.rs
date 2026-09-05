//! Automatic, device-local analyzer preferences. Explicit links always win.
use leptos_router::{location::Url, params::ParamsMap};

const MAX_QUERY_BYTES: usize = 16_384;
#[cfg(any(feature = "hydrate", test))]
const MAX_COOKIE_BYTES: usize = 3_500;

pub fn analyzer(path: &str) -> Option<&'static str> {
    let first = path.strip_prefix('/')?.split('/').next()?;
    [
        "flip-finder",
        "recipe-analyzer",
        "leve-analyzer",
        "venture-analyzer",
        "vendor-resale",
        "scrip-sources",
        "fc-crafting-analyzer",
    ]
    .into_iter()
    .find(|tool| *tool == first)
}

fn parse(query: &str) -> ParamsMap {
    let mut map = ParamsMap::new();
    for pair in query
        .trim_start_matches('?')
        .split('&')
        .filter(|p| !p.is_empty())
    {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(Url::unescape(key), Url::unescape(value));
    }
    map
}

fn world_is_context(path: &str) -> bool {
    analyzer(path).is_some_and(|tool| tool != "flip-finder")
}

pub fn is_bare(path: &str, query: &str) -> bool {
    parse(query)
        .into_iter()
        .all(|(key, _)| key == "lang" || (key == "world" && world_is_context(path)))
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
        map.insert(key, value);
    }
    Some(format!("{path}{}", map.to_query_string()))
}

/// Shared with the HTTP middleware: resolve before rendering so SSR and
/// hydration both see the same filters, columns and resource keys.
pub fn cookie_redirect(path: &str, query: &str, header: &str) -> Option<String> {
    let tool = analyzer(path)?;
    let name = format!("ultros_last_{tool}");
    cookie::Cookie::split_parse_encoded(header)
        .filter_map(Result::ok)
        .find(|c| c.name() == name)
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
                    let (_, setter) = leptos_router::hooks::query_signal_with_options::<String>(
                        key,
                        leptos_router::NavigateOptions {
                            replace: true,
                            scroll: false,
                            ..Default::default()
                        },
                    );
                    setter.set(Some(value));
                }
                return;
            }
            if let Some(saved) = saved_query(&path, &query) {
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
}
