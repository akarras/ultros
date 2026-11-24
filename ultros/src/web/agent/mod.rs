//! AI-agent & public automation layer for Ultros.
//!
//! Provides:
//! - `/llms.txt` and `/llms-full.txt` discovery documents
//! - `/openapi.json` OpenAPI 3.1 schema for public market & gil-making endpoints
//! - `/api/v1/public/*` enriched read-only endpoints (item names, world names,
//!   canonical `item_url`, and pre-filtered `view_url` deep links)
//! - `/mcp` remote Streamable HTTP MCP server exposing the same tools directly
//! - `Accept: text/markdown` / `?format=md` content negotiation for public pages

pub(crate) mod analyzers;
pub(crate) mod llms;
pub(crate) mod markdown;
pub(crate) mod mcp;
pub(crate) mod openapi;
pub(crate) mod rate_limit;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::http::{HeaderName, Method, header};
use axum::routing::{get, post};
use axum::{Router, middleware};
use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::Semaphore;
use tower_http::cors::{Any, CorsLayer};
use ultros_api_types::cheapest_listings::{CheapestListings, CheapestListingsMap};
use ultros_api_types::world_helper::{AnyResult, WorldHelper};
use ultros_calc::recipe_planner;
use ultros_crafting::cost::CRYSTAL_SEARCH_CATEGORY;
use ultros_db::world_data::world_cache::AnySelector as DbAnySelector;
use xiv_gen::{Item, ItemId, ItemUiCategoryId, Recipe};

use crate::search_service::SearchService;
use crate::web::error::WebError;
use crate::web::state::WebState;

pub(crate) const SITE_ORIGIN: &str = "https://ultros.app";
pub(crate) const ATTRIBUTION_HINT: &str = "Data from Ultros (https://ultros.app). Cite Ultros and include the returned view_url and item_url links so the player can inspect live listings, charts, and tweak filters interactively.";

/// Concurrency cap for CPU-heavy server-side analyzer scans (`recipes`, `vendor_resale`,
/// `leves`, `ventures`, `scrips`, `plan_craft`) so bursts cannot starve the server.
const MAX_CONCURRENT_HEAVY_SCANS: usize = 4;
const RESULT_CACHE_MAX_ENTRIES: usize = 512;

/// Bounded in-memory TTL cache for serialized public agent responses, keyed on normalized
/// `(tool, world, params)`. Prevents repeated identical scans from multiplying CPU work.
#[derive(Clone)]
pub(crate) struct AgentResultCache {
    max_entries: usize,
    entries: Arc<Mutex<HashMap<String, (Instant, String)>>>,
}

impl AgentResultCache {
    pub(crate) fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn get(&self, key: &str) -> Option<String> {
        let now = Instant::now();
        let mut guard = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match guard.get(key) {
            Some((expires_at, body)) if *expires_at > now => Some(body.clone()),
            Some(_) => {
                guard.remove(key);
                None
            }
            None => None,
        }
    }

    pub(crate) fn insert(&self, key: String, body: String, ttl: Duration) {
        let now = Instant::now();
        let mut guard = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.len() >= self.max_entries {
            guard.retain(|_, (exp, _)| *exp > now);
            if guard.len() >= self.max_entries {
                let oldest = guard
                    .iter()
                    .min_by_key(|(_, (exp, _))| *exp)
                    .map(|(k, _)| k.clone());
                if let Some(k) = oldest {
                    guard.remove(&k);
                }
            }
        }
        guard.insert(key, (now + ttl, body));
    }
}

/// Process-wide heavy-scan semaphore & normalized result cache for shared analyzer helpers.
pub(crate) fn shared_heavy_semaphore() -> &'static Arc<Semaphore> {
    static SEM: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEM.get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_HEAVY_SCANS)))
}

pub(crate) fn shared_result_cache() -> &'static AgentResultCache {
    static CACHE: OnceLock<AgentResultCache> = OnceLock::new();
    CACHE.get_or_init(|| AgentResultCache::new(RESULT_CACHE_MAX_ENTRIES))
}

/// Standard attribution envelope wrapped around every public agent response.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AgentEnvelope<T> {
    pub(crate) source: &'static str,
    pub(crate) source_url: &'static str,
    pub(crate) view_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) world: Option<String>,
    pub(crate) as_of: DateTime<Utc>,
    pub(crate) attribution_hint: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) filters: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) total_matches: Option<usize>,
    pub(crate) data: T,
}

impl<T> AgentEnvelope<T> {
    pub(crate) fn new(view_url: String, world: Option<String>, data: T) -> Self {
        Self {
            source: "Ultros",
            source_url: SITE_ORIGIN,
            view_url,
            world,
            as_of: Utc::now(),
            attribution_hint: ATTRIBUTION_HINT,
            filters: None,
            total_matches: None,
            data,
        }
    }

    pub(crate) fn with_filters(mut self, filters: serde_json::Value) -> Self {
        self.filters = Some(filters);
        self
    }

    pub(crate) fn with_total_matches(mut self, total: usize) -> Self {
        self.total_matches = Some(total);
        self
    }
}

/// Public CORS policy for read-only agent endpoints, discovery docs, and MCP.
///
/// Permissive (`Allow-Origin: *`, no credentials) on purpose: every route behind this
/// layer is 100% public, read-only, unauthenticated market data (never reads cookies or
/// mutates user state), so cross-origin web/agent clients cannot perform privileged actions
/// via DNS rebinding or CSRF.
pub(crate) fn public_cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            header::ACCEPT,
            header::CONTENT_TYPE,
            HeaderName::from_static("mcp-session-id"),
            HeaderName::from_static("mcp-protocol-version"),
        ])
        .expose_headers([
            HeaderName::from_static("mcp-session-id"),
            HeaderName::from_static("x-ratelimit-limit"),
            HeaderName::from_static("x-ratelimit-remaining"),
            header::RETRY_AFTER,
        ])
}

/// Discovery routes (`/llms.txt`, `/llms-full.txt`, `/openapi.json`) — light static docs.
pub(crate) fn discovery_router() -> Router<WebState> {
    Router::new()
        .route("/llms.txt", get(llms::llms_txt))
        .route("/llms-full.txt", get(llms::llms_full_txt))
        .route("/openapi.json", get(openapi::get_openapi_json))
        .layer(public_cors_layer())
}

/// Rate-limited, CORS-enabled public agent REST API + remote Streamable HTTP MCP server.
/// Uses the shared `limiter` so REST, MCP, and Markdown negotiation enforce one unified budget.
pub(crate) fn agent_router(limiter: rate_limit::AgentRateLimiter) -> Router<WebState> {
    Router::new()
        .route("/api/v1/public/mcp", get(mcp::handle_mcp_discovery))
        .route("/api/v1/public/worlds", get(analyzers::http_worlds))
        .route("/api/v1/public/search", get(analyzers::http_search))
        .route(
            "/api/v1/public/item/{world}/{item}",
            get(analyzers::http_item_market),
        )
        .route("/api/v1/public/flips/{world}", get(analyzers::http_flips))
        .route(
            "/api/v1/public/recipes/{world}",
            get(analyzers::http_recipes),
        )
        .route(
            "/api/v1/public/vendor-resale/{world}",
            get(analyzers::http_vendor_resale),
        )
        .route("/api/v1/public/leves/{world}", get(analyzers::http_leves))
        .route(
            "/api/v1/public/ventures/{world}",
            get(analyzers::http_ventures),
        )
        .route("/api/v1/public/scrips/{world}", get(analyzers::http_scrips))
        .route("/api/v1/public/trends/{world}", get(analyzers::http_trends))
        .route(
            "/api/v1/public/plan-craft",
            post(analyzers::http_plan_craft),
        )
        .route(
            "/mcp",
            post(mcp::handle_mcp_post)
                .get(mcp::handle_mcp_get)
                .delete(mcp::handle_mcp_delete),
        )
        .route_layer(middleware::from_fn_with_state(
            limiter,
            rate_limit::rate_limit_middleware,
        ))
        .layer(public_cors_layer())
}

// --- Canonical URL helpers --------------------------------------------------

pub(crate) fn item_url(world: &str, item_id: i32) -> String {
    let mut u = url::Url::parse(SITE_ORIGIN).expect("valid static origin");
    u.set_path(&format!("/item/{world}/{item_id}"));
    u.to_string()
}

pub(crate) fn recipe_url(recipe_id: i32) -> String {
    format!("{SITE_ORIGIN}/recipe/{recipe_id}")
}

pub(crate) fn tool_url(tool: &str, world: &str, params: &[(&str, String)]) -> String {
    let mut u = url::Url::parse(SITE_ORIGIN).expect("valid static origin");
    u.set_path(&format!("/{tool}/{world}"));
    if !params.is_empty() {
        let mut pairs = u.query_pairs_mut();
        for (k, v) in params {
            if !v.is_empty() {
                pairs.append_pair(k, v);
            }
        }
    }
    u.to_string()
}

// --- Shared game-data lookups (initialized once per process) ----------------

/// Obtainable NPC gil-shop prices (`item_id -> unit gil price`), matching the frontend's
/// `vendor_price_map` so server-side recipe & vendor-resale numbers agree with the UI.
pub(crate) fn vendor_price_map() -> &'static HashMap<i32, i32> {
    static MAP: OnceLock<HashMap<i32, i32>> = OnceLock::new();
    MAP.get_or_init(|| {
        let data = xiv_gen_db::data();
        let mut map = HashMap::new();
        for items in data.gil_shop_items.values() {
            for shop_item in items {
                if !shop_item.availability.is_obtainable() {
                    continue;
                }
                if let Some(item) = data.items.get(&ItemId(shop_item.item))
                    && item.price_mid > 0
                {
                    map.insert(shop_item.item, item.price_mid as i32);
                }
            }
        }
        map
    })
}

/// Output `ItemId` -> list of recipes that craft it (sorted by recipe id for determinism).
pub(crate) fn recipes_by_output() -> &'static HashMap<ItemId, Vec<&'static Recipe>> {
    static MAP: OnceLock<HashMap<ItemId, Vec<&'static Recipe>>> = OnceLock::new();
    MAP.get_or_init(|| {
        let data = xiv_gen_db::data();
        let mut map: HashMap<ItemId, Vec<&'static Recipe>> = HashMap::new();
        for recipe in data.recipes.values() {
            if recipe.item_result > 0 {
                map.entry(ItemId(recipe.item_result))
                    .or_default()
                    .push(recipe);
            }
        }
        for list in map.values_mut() {
            list.sort_by_key(|r| r.key_id.0);
        }
        map
    })
}

/// Static recipe graph for `ultros_calc::recipe_planner`, initialized once per process.
pub(crate) struct StaticPlannerGraph {
    pub(crate) recipes: BTreeMap<i32, recipe_planner::Recipe>,
    pub(crate) default_choices: BTreeMap<i32, i32>,
    pub(crate) shard_ids: BTreeSet<i32>,
}

pub(crate) fn static_planner_graph() -> &'static StaticPlannerGraph {
    static GRAPH: OnceLock<StaticPlannerGraph> = OnceLock::new();
    GRAPH.get_or_init(|| {
        let data = xiv_gen_db::data();
        let mut recipes = BTreeMap::new();
        let mut default_choices = BTreeMap::new();

        // Walk sorted by recipe id so default_choices is deterministic across runs.
        let mut all_recipes: Vec<&Recipe> = data
            .recipes
            .values()
            .filter(|r| r.item_result > 0)
            .collect();
        all_recipes.sort_by_key(|r| r.key_id.0);

        for recipe in all_recipes {
            let ingredients: Vec<(i32, i64)> = recipe
                .ingredient
                .iter()
                .zip(recipe.amount_ingredient.iter())
                .filter_map(|(&id, &amt)| (id > 0 && amt > 0).then_some((id, amt as i64)))
                .collect();
            recipes.insert(
                recipe.key_id.0,
                recipe_planner::Recipe {
                    id: recipe.key_id.0,
                    output: recipe.item_result,
                    yield_amount: recipe.amount_result.max(1) as i64,
                    ingredients,
                },
            );
            default_choices
                .entry(recipe.item_result)
                .or_insert(recipe.key_id.0);
        }

        let shard_ids = data
            .items
            .iter()
            .filter(|(_, item)| item.item_search_category == CRYSTAL_SEARCH_CATEGORY)
            .map(|(id, _)| id.0)
            .collect();

        StaticPlannerGraph {
            recipes,
            default_choices,
            shard_ids,
        }
    })
}

/// Lowercased exact item name -> ItemId (preferring marketable items on collisions).
fn item_name_index() -> &'static HashMap<String, i32> {
    static MAP: OnceLock<HashMap<String, i32>> = OnceLock::new();
    MAP.get_or_init(|| {
        let data = xiv_gen_db::data();
        let mut map: HashMap<String, i32> = HashMap::new();
        for (id, item) in &data.items {
            if item.name.trim().is_empty() {
                continue;
            }
            let key = item.name.to_ascii_lowercase();
            let replace = match map.get(&key) {
                None => true,
                Some(existing_id) => {
                    let existing_marketable = data
                        .items
                        .get(&ItemId(*existing_id))
                        .is_some_and(|i| i.item_search_category > 0);
                    !existing_marketable && item.item_search_category > 0
                }
            };
            if replace {
                map.insert(key, id.0);
            }
        }
        map
    })
}

pub(crate) fn is_shard_item(item_id: ItemId) -> bool {
    static_planner_graph().shard_ids.contains(&item_id.0)
}

pub(crate) fn item_name(item_id: i32) -> &'static str {
    xiv_gen_db::data()
        .items
        .get(&ItemId(item_id))
        .map(|i| i.name.as_str())
        .unwrap_or("Unknown Item")
}

pub(crate) fn item_category_name(item: &Item) -> Option<&'static str> {
    xiv_gen_db::data()
        .item_ui_categorys
        .get(&ItemUiCategoryId(item.item_ui_category))
        .map(|c| c.name.as_str())
        .filter(|s| !s.is_empty())
}

pub(crate) fn world_name_by_id(world_helper: &WorldHelper, world_id: i32) -> String {
    world_helper
        .lookup_selector(ultros_api_types::world_helper::AnySelector::World(world_id))
        .map(|r| r.get_name().to_string())
        .unwrap_or_else(|| format!("World #{world_id}"))
}

/// Resolve an item argument that may be either an integer item ID (`"44178"`) or an item name.
pub(crate) fn resolve_item_arg(raw: &str, search: &SearchService) -> Option<i32> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(id) = trimmed.parse::<i32>() {
        return xiv_gen_db::data()
            .items
            .contains_key(&ItemId(id))
            .then_some(id);
    }
    if let Some(&id) = item_name_index().get(&trimmed.to_ascii_lowercase()) {
        return Some(id);
    }
    // Fall back to the full-text SearchService for partial/fuzzy item names.
    search.search(trimmed).into_iter().find_map(|res| {
        if !res.result_type.eq_ignore_ascii_case("item") {
            return None;
        }
        res.url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .and_then(|seg| seg.parse::<i32>().ok())
    })
}

/// Canonical `(acronym, full_name)` for a `Recipe::craft_type` index.
pub(crate) fn craft_type_info(craft_type: i32) -> Option<(&'static str, &'static str)> {
    Some(match craft_type {
        0 => ("CRP", "Carpenter"),
        1 => ("BSM", "Blacksmith"),
        2 => ("ARM", "Armorer"),
        3 => ("GSM", "Goldsmith"),
        4 => ("LTW", "Leatherworker"),
        5 => ("WVR", "Weaver"),
        6 => ("ALC", "Alchemist"),
        7 => ("CUL", "Culinarian"),
        _ => return None,
    })
}

/// Normalize a user/AI job filter (acronym or full name, case-insensitive) into `(acronym, full_name)`.
pub(crate) fn normalize_crafter_job(input: &str) -> Option<(&'static str, &'static str)> {
    match input.trim().to_ascii_lowercase().as_str() {
        "crp" | "carpenter" => Some(("CRP", "Carpenter")),
        "bsm" | "blacksmith" => Some(("BSM", "Blacksmith")),
        "arm" | "armorer" | "armourer" => Some(("ARM", "Armorer")),
        "gsm" | "goldsmith" => Some(("GSM", "Goldsmith")),
        "ltw" | "leatherworker" => Some(("LTW", "Leatherworker")),
        "wvr" | "weaver" => Some(("WVR", "Weaver")),
        "alc" | "alchemist" => Some(("ALC", "Alchemist")),
        "cul" | "culinarian" => Some(("CUL", "Culinarian")),
        _ => None,
    }
}

/// Convert `WorldHelper`'s `AnyResult` into the `ultros_db` selector that `AnalyzerService` expects.
pub(crate) fn db_selector(result: &AnyResult<'_>) -> DbAnySelector {
    match result {
        AnyResult::Region(r) => DbAnySelector::Region(r.id),
        AnyResult::Datacenter(d) => DbAnySelector::Datacenter(d.id),
        AnyResult::World(w) => DbAnySelector::World(w.id),
    }
}

/// Load a crafting price snapshot through the configured market client.
pub(crate) async fn snapshot_cheapest_map(
    state: &WebState,
    selector: &DbAnySelector,
) -> Result<CheapestListingsMap, WebError> {
    let scope = state.world_cache.lookup_selector(selector)?;
    let response = state
        .market_client
        .cheapest(scope.get_name(), None, &Default::default())
        .await
        .map_err(|_| WebError::TemporarilyUnavailable)?;
    Ok(decode_market_json::<CheapestListings>(response)
        .await?
        .into())
}

/// Every agent surface may cache a successful result. Propagate degraded market
/// responses as temporary failures rather than caching warm-up or fallback data.
async fn decode_market_json<T: serde::de::DeserializeOwned>(
    response: axum::response::Response,
) -> Result<T, WebError> {
    if !response.status().is_success()
        || response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case("no-store"))
            })
    {
        return Err(WebError::TemporarilyUnavailable);
    }
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .map_err(|_| WebError::TemporarilyUnavailable)?;
    serde_json::from_slice(&bytes).map_err(|_| WebError::TemporarilyUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn market_warmup_is_not_a_cacheable_empty_result() {
        use axum::{Json, response::IntoResponse};
        let mut warming = Json(Vec::<i32>::new()).into_response();
        warming.headers_mut().insert(
            header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        );
        assert!(matches!(
            decode_market_json::<Vec<i32>>(warming).await,
            Err(WebError::TemporarilyUnavailable)
        ));
        let ready = Json(Vec::<i32>::new()).into_response();
        assert!(
            decode_market_json::<Vec<i32>>(ready)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn canonical_urls_encode_world_and_query_params() {
        assert_eq!(
            item_url("Gilgamesh", 44178),
            "https://ultros.app/item/Gilgamesh/44178"
        );
        assert_eq!(recipe_url(35800), "https://ultros.app/recipe/35800");
        assert_eq!(
            tool_url(
                "recipe-analyzer",
                "Gilgamesh",
                &[
                    ("job", "CUL".into()),
                    ("subcrafts", "true".into()),
                    ("empty", String::new()),
                ],
            ),
            "https://ultros.app/recipe-analyzer/Gilgamesh?job=CUL&subcrafts=true"
        );
    }

    #[test]
    fn crafter_job_normalization_handles_acronyms_and_names() {
        assert_eq!(normalize_crafter_job("cul"), Some(("CUL", "Culinarian")));
        assert_eq!(
            normalize_crafter_job("Culinarian"),
            Some(("CUL", "Culinarian"))
        );
        assert_eq!(normalize_crafter_job("armourer"), Some(("ARM", "Armorer")));
        assert_eq!(normalize_crafter_job("unknown"), None);
    }

    #[test]
    fn static_game_data_caches_populate_deterministically() {
        assert!(!vendor_price_map().is_empty());
        assert!(!recipes_by_output().is_empty());
        let g = static_planner_graph();
        assert!(!g.recipes.is_empty());
        assert!(!g.default_choices.is_empty());
        assert!(!g.shard_ids.is_empty());
    }

    #[test]
    fn result_cache_respects_ttl_and_capacity() {
        let cache = AgentResultCache::new(2);
        cache.insert("k1".into(), "v1".into(), Duration::from_secs(30));
        cache.insert("k2".into(), "v2".into(), Duration::from_secs(60));
        assert_eq!(cache.get("k1").as_deref(), Some("v1"));
        cache.insert("k3".into(), "v3".into(), Duration::from_secs(60));
        // Evicted soonest-expiring entry (k1) when capacity reached.
        assert_eq!(cache.get("k1"), None);
        assert_eq!(cache.get("k2").as_deref(), Some("v2"));
        assert_eq!(cache.get("k3").as_deref(), Some("v3"));
    }
}
