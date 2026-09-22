//! Shared analyzer landing views and portable, explicit view URLs.
use leptos_router::{location::Url, params::ParamsMap};

pub const MAX_QUERY_BYTES: usize = 16_384;
pub const FLIP_RECOMMENDED_QUERY: &str = "?min-buy=5000&last-sold=1d&roi=30&sort=profit-per-day";
/// Large defaults restore after hydration; SSR must not substitute last view.
pub const LOCAL_DEFAULT_COOKIE_VALUE: &str = "local-storage";

pub fn analyzer(path: &str) -> Option<&'static str> {
    let first = path.strip_prefix('/')?.split('/').next()?;
    [
        "flip-finder",
        "recipe-analyzer",
        "leve-analyzer",
        "venture-analyzer",
        "vendor-resale",
        "vendor-sell",
        "scrip-sources",
        "fc-crafting-analyzer",
        "trends",
        "items",
        "currency-exchange",
    ]
    .into_iter()
    .find(|tool| *tool == first)
}

pub fn parse_query(query: &str) -> ParamsMap {
    let mut map = ParamsMap::new();
    for pair in query
        .trim_start_matches('?')
        .split('&')
        .filter(|p| !p.is_empty())
    {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        // ParamsMap::insert decodes the value itself. Decoding here as well
        // would turn a saved literal `%20` into a space.
        map.insert(Url::unescape(key), value.to_string());
    }
    map
}

pub fn is_context(tool: &str, key: &str) -> bool {
    key == "lang" || (key == "world" && tool != "flip-finder")
}

/// `v=1` records an intentional view, including an intentionally empty view.
/// Without this marker opening a saved empty view would run landing defaults.
pub fn explicit_query(tool: &str, query: &str) -> String {
    let mut params = parse_query(query);
    params.remove("lang");
    if tool != "flip-finder" {
        params.remove("world");
    }
    params.remove("v");
    params.insert("v", "1".to_string());
    params.to_query_string()
}

pub fn view_href(path: &str, query: &str, current: &ParamsMap) -> String {
    let tool = analyzer(path).unwrap_or_default();
    let mut params = parse_query(&explicit_query(tool, query));
    let path_has_world = tool != "items"
        && tool != "currency-exchange"
        && path.trim_matches('/').split('/').count() > 1;
    for (key, value) in current.clone() {
        if is_context(tool, &key) && !(key == "world" && path_has_world) {
            // This value is already decoded; insert expects encoded input.
            params.insert(key, Url::escape(&value));
        }
    }
    format!("{path}{}", params.to_query_string())
}

/// Each default expresses the job the tool performs; sparse crafts and fixed
/// NPC payouts must not inherit a one-sale-per-day resale threshold.
pub fn recommended_query(tool: &str) -> String {
    let query = match tool {
        "flip-finder" => FLIP_RECOMMENDED_QUERY,
        "recipe-analyzer" => "?min-sales=1&profit=1&hide-suspicious=true",
        "venture-analyzer" => "?profit=1&filter-outliers=true",
        "leve-analyzer" => "?profit=1&filter-outliers=true",
        "fc-crafting-analyzer" => "?profit=1&complete-prices=true",
        "vendor-sell" => "?profit=1",
        "vendor-resale" => "?profit=1&next-sale=1d&show-suspicious=false",
        "scrip-sources" => "?complete-prices=true&sort=efficiency",
        "currency-exchange" => "?price_per_item_min=1",
        "trends" => "?min_sales=3&show_suspicious=false",
        "items" => "",
        _ => return "?v=1".into(),
    };
    let mut params = parse_query(query);
    if matches!(
        tool,
        "venture-analyzer" | "fc-crafting-analyzer" | "currency-exchange" | "trends" | "items"
    ) {
        params.insert(
            "gf",
            r#"{"market-listing-assessment":{"op":"ne","value":"suspicious"}}"#.to_string(),
        );
    }
    explicit_query(tool, &params.to_query_string())
}

pub fn default_storage_key(tool: &str) -> String {
    // Preserve the original Flip Finder preference without migration.
    if tool == "flip-finder" {
        "ultros.flipfinder.default_view".into()
    } else {
        format!("ultros.grid.{tool}-grid.default_view")
    }
}

pub fn saved_default_query(tool: &str) -> Option<String> {
    #[cfg(feature = "hydrate")]
    {
        leptos::prelude::window()
            .local_storage()
            .ok()
            .flatten()?
            .get_item(&default_storage_key(tool))
            .ok()
            .flatten()
            .filter(|q| q.len() <= MAX_QUERY_BYTES)
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = tool;
        None
    }
}

/// Save immediately to both storage and the SSR preference cookie. A named
/// default takes precedence over the automatically remembered last view.
pub fn save_default_query(tool: &str, query: &str) -> bool {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        let query = explicit_query(tool, query);
        if query.len() > MAX_QUERY_BYTES {
            return false;
        }
        let Ok(Some(storage)) = leptos::prelude::window().local_storage() else {
            return false;
        };
        if storage
            .set_item(&default_storage_key(tool), &query)
            .is_err()
        {
            return false;
        }
        if let Ok(document) = leptos::prelude::document().dyn_into::<web_sys::HtmlDocument>() {
            let cookie = format!(
                "ultros_default_{tool}={}; Path=/{tool}; SameSite=Lax; Max-Age=31536000",
                Url::escape(&query)
            );
            let cookie = if cookie.len() <= 3500 {
                cookie
            } else {
                format!(
                    "ultros_default_{tool}={LOCAL_DEFAULT_COOKIE_VALUE}; Path=/{tool}; SameSite=Lax; Max-Age=31536000"
                )
            };
            let _ = document.set_cookie(&cookie);
        }
        true
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = (tool, query);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percent_sequences_in_filters_survive_saving_and_reopening_views() {
        let query = "?q=%2520%20%2526&gf=%7B%22item%22%3A%7B%22op%22%3A%22contains%22%2C%22value%22%3A%22%2520%22%7D%7D";
        let saved = explicit_query("items", query);
        assert_eq!(explicit_query("items", &saved), saved);
        let href = view_href(
            "/items/category/1",
            &saved,
            &parse_query("?world=Zone%2520&lang=ja"),
        );
        let restored = parse_query(href.split_once('?').unwrap().1);
        assert_eq!(restored.get("q").as_deref(), Some("%20 %26"));
        assert_eq!(
            restored.get("gf").as_deref(),
            Some(r#"{"item":{"op":"contains","value":"%20"}}"#)
        );
        assert_eq!(restored.get("world").as_deref(), Some("Zone%20"));
    }
    #[test]
    fn defaults_match_the_tools_work_instead_of_a_universal_sales_floor() {
        let recipe = parse_query(&recommended_query("recipe-analyzer"));
        assert_eq!(recipe.get("hide-suspicious").as_deref(), Some("true"));
        assert_eq!(recipe.get("min-sales").as_deref(), Some("1"));
        for tool in [
            "leve-analyzer",
            "fc-crafting-analyzer",
            "vendor-sell",
            "scrip-sources",
        ] {
            let query = parse_query(&recommended_query(tool));
            assert!(query.get("min-sales").is_none(), "{tool}");
            assert!(query.get("next-sale").is_none(), "{tool}");
        }
    }
    #[test]
    fn suspicious_guards_are_explicit_only_in_recommended_views() {
        for (tool, key) in [
            ("vendor-resale", "show-suspicious"),
            ("trends", "show_suspicious"),
        ] {
            assert_eq!(
                parse_query(&recommended_query(tool)).get(key).as_deref(),
                Some("false")
            );
            assert!(parse_query(&explicit_query(tool, "")).get(key).is_none());
        }
    }

    #[test]
    fn empty_views_are_explicit_and_flip_buy_world_survives() {
        assert_eq!(explicit_query("items", "?world=Gilgamesh&lang=ja"), "?v=1");
        assert!(explicit_query("flip-finder", "?world=Goblin").contains("world=Goblin"));
        assert_eq!(explicit_query("items", "?v=1&v=1"), "?v=1");
    }
    #[test]
    fn applying_a_view_preserves_the_live_scope_and_language() {
        let current = ParamsMap::from_iter([("world", "Gilgamesh"), ("lang", "ja")]);
        let href = view_href("/items", "?world=Goblin", &current);
        assert!(href.contains("world=Gilgamesh"));
        assert!(href.contains("lang=ja"));
        assert!(href.contains("v=1"));
        assert!(
            !view_href("/recipe-analyzer/Gilgamesh", "?world=Goblin", &current).contains("world=")
        );
    }
}
