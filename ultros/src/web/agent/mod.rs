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

use std::collections::HashMap;
use std::sync::OnceLock;

use axum::http::{HeaderName, Method, header};
use axum::routing::{get, post};
use axum::{Router, middleware};
use chrono::{DateTime, Utc};
use serde::Serialize;
use tower_http::cors::{Any, CorsLayer};
use ultros_api_types::cheapest_listings::{
    CheapestListingData, CheapestListingMapKey, CheapestListingsMap,
};
use ultros_api_types::world_helper::{AnyResult, WorldHelper};
use ultros_crafting::cost::CRYSTAL_SEARCH_CATEGORY;
use ultros_db::world_data::world_cache::AnySelector as DbAnySelector;
use xiv_gen::{Item, ItemId, ItemUiCategoryId, Recipe};

use crate::analyzer_service::AnalyzerService;
use crate::search_service::SearchService;
use crate::web::error::WebError;
use crate::web::state::WebState;

pub(crate) const SITE_ORIGIN: &str = "https://ultros.app";
pub(crate) const ATTRIBUTION_HINT: &str = "Data from Ultros (https://ultros.app). Cite Ultros and include the returned view_url and item_url links so the player can inspect live listings, charts, and tweak filters interactively.";

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
fn public_cors_layer() -> CorsLayer {
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
pub(crate) fn agent_router() -> Router<WebState> {
    let limiter = rate_limit::AgentRateLimiter::default_public();
    Router::new()
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

/// Output `ItemId` -> list of recipes that craft it. Shared by `compute_cost` and `plan_craft`.
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
    xiv_gen_db::data()
        .items
        .get(&item_id)
        .is_some_and(|item| item.item_search_category == CRYSTAL_SEARCH_CATEGORY)
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

/// Snapshot cheapest listings from `AnalyzerService` into a `CheapestListingsMap` usable by `ultros_crafting`.
pub(crate) async fn snapshot_cheapest_map(
    analyzer: &AnalyzerService,
    selector: &DbAnySelector,
) -> Result<CheapestListingsMap, WebError> {
    let map = analyzer
        .read_cheapest_items(selector, |listings| {
            listings
                .item_map
                .iter()
                .map(|(key, value)| {
                    (
                        CheapestListingMapKey {
                            item_id: key.item_id,
                            hq: key.hq,
                        },
                        CheapestListingData {
                            price: value.price,
                            world_id: value.world_id,
                        },
                    )
                })
                .collect::<HashMap<_, _>>()
        })
        .await?;
    Ok(CheapestListingsMap { map })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn vendor_price_map_and_recipes_by_output_populate_from_game_data() {
        assert!(!vendor_price_map().is_empty());
        assert!(!recipes_by_output().is_empty());
    }
}
