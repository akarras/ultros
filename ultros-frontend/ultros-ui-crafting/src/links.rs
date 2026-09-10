pub fn recipe_href(id: i32, world: &str, query: &leptos_router::params::ParamsMap) -> String {
    format!("/recipe/{id}{}", market_query(world, query))
}

pub fn market_query(world: &str, query: &leptos_router::params::ParamsMap) -> String {
    use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
    let mut url = format!("?world={}", utf8_percent_encode(world, NON_ALPHANUMERIC));
    for key in [
        "buy-scope",
        "require-hq",
        "subcrafts",
        "shards-exclude",
        "lang",
    ] {
        if let Some(value) = query.get(key) {
            url.push_str(&format!(
                "&{key}={}",
                utf8_percent_encode(&value, NON_ALPHANUMERIC)
            ));
        }
    }
    url
}

/// Return from a recipe detail to the analyzer using its canonical world path.
pub fn recipe_analyzer_href(world: &str, query: &leptos_router::params::ParamsMap) -> String {
    use leptos_router::{location::Url, params::ParamsMap};
    let mut filters = ParamsMap::new();
    for key in [
        "buy-scope",
        "require-hq",
        "subcrafts",
        "shards-exclude",
        "lang",
    ] {
        if let Some(value) = query.get(key) {
            filters.insert(key, value);
        }
    }
    format!(
        "/recipe-analyzer/{}{}",
        Url::escape(world),
        filters.to_query_string()
    )
}
