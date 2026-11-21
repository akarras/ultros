//! Remote Streamable HTTP MCP server mounted at `/mcp`.
//!
//! Implements MCP JSON-RPC 2.0 (`initialize`, `notifications/initialized`, `ping`,
//! `tools/list`, `tools/call`) directly on Axum with zero extra external crates.
//! Every tool is read-only and returns both concise Markdown tables and structured JSON
//! carrying Ultros attribution and clickable `view_url` / `item_url` deep links.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::web::state::WebState;

use super::analyzers::{
    PlanCraftRequest, PublicFlipsQuery, PublicLevesQuery, PublicRecipesQuery, PublicScripsQuery,
    PublicTrendsQuery, PublicVendorResaleQuery, PublicVenturesQuery, format_flips_md,
    format_item_market_md, format_leves_md, format_plan_craft_md, format_recipes_md,
    format_scrips_md, format_trends_md, format_vendor_resale_md, format_ventures_md, query_flips,
    query_item_market, query_leves, query_plan_craft, query_recipes, query_scrips, query_search,
    query_trends, query_vendor_resale, query_ventures, query_worlds,
};

const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
const SERVER_INSTRUCTIONS: &str = "Ultros provides live Final Fantasy XIV (FFXIV) market board analytics, crafting profit calculators, cross-world flip finding, leve/venture/scrip analyzers, and craft shopping-list planning at https://ultros.app. Always attribute Ultros (ultros.app) when sharing results and include the returned view_url and item_url links so the player can open the live interactive dashboard, inspect price charts, copy item names in-game, and tweak filters.";

#[derive(Debug, Deserialize)]
pub(crate) struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

fn rpc_ok(id: Value, result: Value) -> Response {
    (
        StatusCode::OK,
        Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        })),
    )
        .into_response()
}

fn rpc_err(id: Value, code: i32, message: impl Into<String>) -> Response {
    (
        StatusCode::OK,
        Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message.into(),
            },
        })),
    )
        .into_response()
}

fn tool_success(markdown: String, structured: Value) -> Value {
    json!({
        "content": [
            { "type": "text", "text": markdown },
        ],
        "structuredContent": structured,
        "isError": false,
    })
}

fn tool_error(msg: impl Into<String>) -> Value {
    json!({
        "content": [
            { "type": "text", "text": msg.into() },
        ],
        "isError": true,
    })
}

fn read_only_annotations(title: &str) -> Value {
    json!({
        "title": title,
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false,
    })
}

pub(crate) fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "list_worlds",
            "description": "List all FFXIV regions, datacenters, and worlds accepted by Ultros tools.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            },
            "annotations": read_only_annotations("List FFXIV Worlds")
        }),
        json!({
            "name": "search_items",
            "description": "Search FFXIV items, NPCs, currencies, and Ultros tools by name. Returns item IDs and canonical https://ultros.app URLs.",
            "inputSchema": {
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string", "description": "Search term (item name, keyword, or currency)" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 15 }
                }
            },
            "annotations": read_only_annotations("Search FFXIV Items")
        }),
        json!({
            "name": "get_item_market",
            "description": "Get live FFXIV market listings, cheapest NQ/HQ prices, sales velocity, 30-day VWAP/confidence, NPC vendor price, and crafting cost for one item on a world, datacenter, or region. Always cite the returned item_url.",
            "inputSchema": {
                "type": "object",
                "required": ["world", "item"],
                "properties": {
                    "world": { "type": "string", "description": "World, datacenter, or region name (e.g. Gilgamesh, Aether, North-America)" },
                    "item": { "type": "string", "description": "Item ID (e.g. '44178') or item name" }
                }
            },
            "annotations": read_only_annotations("Get Item Market Summary")
        }),
        json!({
            "name": "find_flips",
            "description": "Find profitable cross-world arbitrage flips to buy cheaply on another world in the region and resell on the target world. Returns post-tax profit, ROI, velocity, confidence, item_url, and a pre-filtered Ultros Flip Finder view_url.",
            "inputSchema": {
                "type": "object",
                "required": ["world"],
                "properties": {
                    "world": { "type": "string", "description": "Target sell world name (e.g. Gilgamesh)" },
                    "min_profit": { "type": "integer", "description": "Minimum post-tax profit in gil" },
                    "filter_sale": { "type": "string", "enum": ["Day", "Week", "Month"], "description": "Require at least one sale within this window" },
                    "min_velocity": { "type": "number", "description": "Minimum recent sales per day" },
                    "min_buffer_sales": { "type": "integer", "minimum": 0, "maximum": 255, "description": "Minimum recent sales count" },
                    "max_roi": { "type": "number", "description": "Maximum ROI % cap to filter extreme outliers" },
                    "show_suspicious": { "type": "boolean", "default": false, "description": "Include items flagged for launder/price manipulation" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 }
                }
            },
            "annotations": read_only_annotations("Find Cross-World Flips")
        }),
        json!({
            "name": "analyze_recipes",
            "description": "Run Ultros's crafting profit calculator (Recipe Analyzer) on a world. Tweak crafter job, level range, ingredient buy scope, subcrafting, HQ ingredients, shards, profit/ROI/velocity thresholds, and sort order. Returns ingredient breakdowns plus a pre-filtered Ultros Recipe Analyzer view_url.",
            "inputSchema": {
                "type": "object",
                "required": ["world"],
                "properties": {
                    "world": { "type": "string", "description": "Sell world name (e.g. Gilgamesh)" },
                    "job": { "type": "string", "description": "Crafter job code or name: CRP, BSM, ARM, GSM, LTW, WVR, ALC, CUL" },
                    "min_level": { "type": "integer", "minimum": 1, "maximum": 100 },
                    "max_level": { "type": "integer", "minimum": 1, "maximum": 100 },
                    "buy_scope": { "type": "string", "enum": ["world", "datacenter", "region"], "default": "datacenter" },
                    "use_subcrafts": { "type": "boolean", "default": false, "description": "Craft intermediate ingredients when cheaper than buying" },
                    "require_hq": { "type": "boolean", "default": false, "description": "Price ingredients using HQ listings when available" },
                    "exclude_shards": { "type": "boolean", "default": true, "description": "Exclude shard/crystal/cluster cost" },
                    "min_profit": { "type": "integer", "description": "Minimum post-tax profit per unit in gil" },
                    "min_roi": { "type": "integer", "description": "Minimum ROI percentage" },
                    "min_sales": { "type": "integer", "description": "Minimum recent sales count" },
                    "min_daily_sales": { "type": "number", "description": "Minimum daily sales velocity" },
                    "sort": { "type": "string", "enum": ["profit", "roi", "profit_per_day", "daily_sales", "cost", "price"], "default": "profit" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 }
                }
            },
            "annotations": read_only_annotations("Analyze Profitable Crafting Recipes")
        }),
        json!({
            "name": "analyze_vendor_resale",
            "description": "Find NPC gil-shop items that resell profitably on a world's market board. Returns vendor cost, market price, post-tax profit, ROI, velocity, and a pre-filtered Vendor Resale view_url.",
            "inputSchema": {
                "type": "object",
                "required": ["world"],
                "properties": {
                    "world": { "type": "string" },
                    "min_profit": { "type": "integer" },
                    "min_roi": { "type": "integer" },
                    "min_sales": { "type": "integer" },
                    "tax": { "type": "boolean", "default": true },
                    "show_suspicious": { "type": "boolean", "default": false },
                    "sort": { "type": "string", "enum": ["profit", "roi", "daily_sales"], "default": "profit" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 }
                }
            },
            "annotations": read_only_annotations("Analyze NPC Vendor Resale")
        }),
        json!({
            "name": "analyze_leves",
            "description": "Rank FFXIV craft guildleve turn-ins by net gil profit, comparing market buy cost vs. crafting cost against the gil reward.",
            "inputSchema": {
                "type": "object",
                "required": ["world"],
                "properties": {
                    "world": { "type": "string" },
                    "job": { "type": "string", "description": "Crafter job name or 3-letter acronym" },
                    "min_level": { "type": "integer", "minimum": 1, "maximum": 100 },
                    "max_level": { "type": "integer", "minimum": 1, "maximum": 100 },
                    "hq": { "type": "boolean", "default": true, "description": "Assume HQ turn-ins (2x gil reward)" },
                    "min_profit": { "type": "integer" },
                    "buy_scope": { "type": "string", "enum": ["world", "datacenter", "region"], "default": "world" },
                    "sort": { "type": "string", "enum": ["profit", "level", "cost"], "default": "profit" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 }
                }
            },
            "annotations": read_only_annotations("Analyze Craft Leve Profits")
        }),
        json!({
            "name": "analyze_ventures",
            "description": "Rank fixed-reward FFXIV retainer ventures by estimated net gil value on a world.",
            "inputSchema": {
                "type": "object",
                "required": ["world"],
                "properties": {
                    "world": { "type": "string" },
                    "min_level": { "type": "integer", "minimum": 1, "maximum": 100 },
                    "max_level": { "type": "integer", "minimum": 1, "maximum": 100 },
                    "min_profit": { "type": "integer" },
                    "sort": { "type": "string", "enum": ["profit", "daily_sales", "level"], "default": "profit" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 }
                }
            },
            "annotations": read_only_annotations("Analyze Retainer Ventures")
        }),
        json!({
            "name": "analyze_scrips",
            "description": "Find the cheapest gil-per-scrip collectable turn-ins for Crafters' and Gatherers' scrips on a world.",
            "inputSchema": {
                "type": "object",
                "required": ["world"],
                "properties": {
                    "world": { "type": "string" },
                    "scrip": { "type": "string", "enum": ["OrangeCrafters", "PurpleCrafters", "OrangeGatherers", "PurpleGatherers"] },
                    "job": { "type": "string" },
                    "buy_scope": { "type": "string", "enum": ["world", "datacenter", "region"], "default": "datacenter" },
                    "use_subcrafts": { "type": "boolean", "default": false },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 }
                }
            },
            "annotations": read_only_annotations("Analyze Collectable Scrip Sources")
        }),
        json!({
            "name": "get_market_trends",
            "description": "Get the highest-volume, fastest-selling, or biggest price-moving items on a world over 7, 30, or 90 days.",
            "inputSchema": {
                "type": "object",
                "required": ["world"],
                "properties": {
                    "world": { "type": "string" },
                    "window": { "type": "integer", "enum": [7, 30, 90], "default": 30 },
                    "show_suspicious": { "type": "boolean", "default": false },
                    "sort": { "type": "string", "enum": ["gil_volume", "sales_per_day", "pct_change", "unit_volume"], "default": "gil_volume" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 }
                }
            },
            "annotations": read_only_annotations("Get Market Trends")
        }),
        json!({
            "name": "plan_craft",
            "description": "Expand one or more target crafts into an aggregated raw-material shopping list (with cheapest world/vendor prices) and bottom-up crafting order.",
            "inputSchema": {
                "type": "object",
                "required": ["world", "items"],
                "properties": {
                    "world": { "type": "string" },
                    "buy_scope": { "type": "string", "enum": ["world", "datacenter", "region"], "default": "datacenter" },
                    "use_subcrafts": { "type": "boolean", "default": true },
                    "exclude_shards": { "type": "boolean", "default": true },
                    "items": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 20,
                        "items": {
                            "type": "object",
                            "required": ["item", "quantity"],
                            "properties": {
                                "item": { "type": "string", "description": "Item ID or item name" },
                                "quantity": { "type": "integer", "minimum": 1, "maximum": 9999 }
                            }
                        }
                    }
                }
            },
            "annotations": read_only_annotations("Plan Crafting Shopping List")
        }),
    ]
}

fn str_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .filter(|s| !s.trim().is_empty())
}

fn i32_arg(args: &Value, key: &str) -> Option<i32> {
    args.get(key)
        .and_then(|v| v.as_i64())
        .and_then(|n| i32::try_from(n).ok())
}

fn usize_arg(args: &Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(|v| v.as_u64())
        .and_then(|n| usize::try_from(n).ok())
}

fn f32_arg(args: &Value, key: &str) -> Option<f32> {
    args.get(key).and_then(|v| v.as_f64()).map(|n| n as f32)
}

fn bool_arg(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| v.as_bool())
}

async fn dispatch_tool(state: &WebState, name: &str, args: &Value) -> Value {
    metrics::counter!(
        "ultros_agent_requests_total",
        "surface" => "mcp",
        "tool" => name.to_string()
    )
    .increment(1);

    match name {
        "list_worlds" => {
            let env = query_worlds(&state.world_helper);
            let mut md = String::from(
                "# FFXIV Regions, Datacenters & Worlds\n\n> Source: **Ultros** (https://ultros.app)\n\n",
            );
            for r in &env.data {
                md.push_str(&format!("## {}\n", r.name));
                for dc in &r.datacenters {
                    let worlds: Vec<&str> = dc.worlds.iter().map(|w| w.name.as_str()).collect();
                    md.push_str(&format!("- **{}**: {}\n", dc.name, worlds.join(", ")));
                }
                md.push('\n');
            }
            tool_success(md, serde_json::to_value(&env).unwrap_or(Value::Null))
        }
        "search_items" => {
            let Some(query) = str_arg(args, "query") else {
                return tool_error("Missing required argument: query");
            };
            let env = query_search(state, &query, usize_arg(args, "limit"));
            let mut md = format!(
                "# Search results for \"{query}\"\n\n> Source: **Ultros** ({})\n\n",
                env.view_url
            );
            for item in &env.data {
                let id_part = item
                    .item_id
                    .map(|id| format!(" (ID: {id})"))
                    .unwrap_or_default();
                md.push_str(&format!(
                    "- [{}]({}){} — {}\n",
                    item.title, item.url, id_part, item.result_type
                ));
            }
            tool_success(md, serde_json::to_value(&env).unwrap_or(Value::Null))
        }
        "get_item_market" => {
            let Some(world) = str_arg(args, "world") else {
                return tool_error("Missing required argument: world");
            };
            let Some(item) = str_arg(args, "item") else {
                return tool_error("Missing required argument: item");
            };
            match query_item_market(state, &world, &item).await {
                Ok(env) => tool_success(
                    format_item_market_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => tool_error(format!(
                    "Could not fetch item market for '{item}' on '{world}': {e}"
                )),
            }
        }
        "find_flips" => {
            let Some(world) = str_arg(args, "world") else {
                return tool_error("Missing required argument: world");
            };
            let q = PublicFlipsQuery {
                min_profit: i32_arg(args, "min_profit"),
                filter_sale: str_arg(args, "filter_sale"),
                min_velocity: f32_arg(args, "min_velocity"),
                min_buffer_sales: args
                    .get("min_buffer_sales")
                    .and_then(|v| v.as_u64())
                    .and_then(|n| u8::try_from(n).ok()),
                max_roi: f32_arg(args, "max_roi"),
                show_suspicious: bool_arg(args, "show_suspicious"),
                limit: usize_arg(args, "limit"),
            };
            match query_flips(state, &world, q).await {
                Ok(env) => tool_success(
                    format_flips_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => tool_error(format!("Could not compute flips for '{world}': {e}")),
            }
        }
        "analyze_recipes" => {
            let Some(world) = str_arg(args, "world") else {
                return tool_error("Missing required argument: world");
            };
            let q = PublicRecipesQuery {
                job: str_arg(args, "job"),
                min_level: i32_arg(args, "min_level").and_then(|n| i8::try_from(n).ok()),
                max_level: i32_arg(args, "max_level").and_then(|n| i8::try_from(n).ok()),
                buy_scope: str_arg(args, "buy_scope"),
                use_subcrafts: bool_arg(args, "use_subcrafts"),
                require_hq: bool_arg(args, "require_hq"),
                exclude_shards: bool_arg(args, "exclude_shards"),
                min_profit: i32_arg(args, "min_profit"),
                min_roi: i32_arg(args, "min_roi"),
                min_sales: usize_arg(args, "min_sales"),
                min_daily_sales: f32_arg(args, "min_daily_sales"),
                sort: str_arg(args, "sort"),
                limit: usize_arg(args, "limit"),
            };
            match query_recipes(state, &world, q).await {
                Ok(env) => tool_success(
                    format_recipes_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => tool_error(format!("Could not analyze recipes for '{world}': {e}")),
            }
        }
        "analyze_vendor_resale" => {
            let Some(world) = str_arg(args, "world") else {
                return tool_error("Missing required argument: world");
            };
            let q = PublicVendorResaleQuery {
                min_profit: i32_arg(args, "min_profit"),
                min_roi: i32_arg(args, "min_roi"),
                min_sales: usize_arg(args, "min_sales"),
                tax: bool_arg(args, "tax"),
                show_suspicious: bool_arg(args, "show_suspicious"),
                sort: str_arg(args, "sort"),
                limit: usize_arg(args, "limit"),
            };
            match query_vendor_resale(state, &world, q).await {
                Ok(env) => tool_success(
                    format_vendor_resale_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => tool_error(format!(
                    "Could not analyze vendor resale for '{world}': {e}"
                )),
            }
        }
        "analyze_leves" => {
            let Some(world) = str_arg(args, "world") else {
                return tool_error("Missing required argument: world");
            };
            let q = PublicLevesQuery {
                job: str_arg(args, "job"),
                min_level: i32_arg(args, "min_level").and_then(|n| u16::try_from(n).ok()),
                max_level: i32_arg(args, "max_level").and_then(|n| u16::try_from(n).ok()),
                hq: bool_arg(args, "hq"),
                min_profit: i32_arg(args, "min_profit"),
                buy_scope: str_arg(args, "buy_scope"),
                sort: str_arg(args, "sort"),
                limit: usize_arg(args, "limit"),
            };
            match query_leves(state, &world, q).await {
                Ok(env) => tool_success(
                    format_leves_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => tool_error(format!("Could not analyze leves for '{world}': {e}")),
            }
        }
        "analyze_ventures" => {
            let Some(world) = str_arg(args, "world") else {
                return tool_error("Missing required argument: world");
            };
            let q = PublicVenturesQuery {
                min_level: i32_arg(args, "min_level").and_then(|n| u8::try_from(n).ok()),
                max_level: i32_arg(args, "max_level").and_then(|n| u8::try_from(n).ok()),
                min_profit: i32_arg(args, "min_profit"),
                sort: str_arg(args, "sort"),
                limit: usize_arg(args, "limit"),
            };
            match query_ventures(state, &world, q).await {
                Ok(env) => tool_success(
                    format_ventures_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => tool_error(format!("Could not analyze ventures for '{world}': {e}")),
            }
        }
        "analyze_scrips" => {
            let Some(world) = str_arg(args, "world") else {
                return tool_error("Missing required argument: world");
            };
            let q = PublicScripsQuery {
                scrip: str_arg(args, "scrip"),
                job: str_arg(args, "job"),
                buy_scope: str_arg(args, "buy_scope"),
                use_subcrafts: bool_arg(args, "use_subcrafts"),
                limit: usize_arg(args, "limit"),
            };
            match query_scrips(state, &world, q).await {
                Ok(env) => tool_success(
                    format_scrips_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => tool_error(format!("Could not analyze scrips for '{world}': {e}")),
            }
        }
        "get_market_trends" => {
            let Some(world) = str_arg(args, "world") else {
                return tool_error("Missing required argument: world");
            };
            let q = PublicTrendsQuery {
                window: i32_arg(args, "window").and_then(|n| u16::try_from(n).ok()),
                show_suspicious: bool_arg(args, "show_suspicious"),
                sort: str_arg(args, "sort"),
                limit: usize_arg(args, "limit"),
            };
            match query_trends(state, &world, q).await {
                Ok(env) => tool_success(
                    format_trends_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => tool_error(format!("Could not fetch market trends for '{world}': {e}")),
            }
        }
        "plan_craft" => {
            let req: PlanCraftRequest = match serde_json::from_value(args.clone()) {
                Ok(r) => r,
                Err(e) => return tool_error(format!("Invalid plan_craft arguments: {e}")),
            };
            match query_plan_craft(state, req).await {
                Ok(env) => tool_success(
                    format_plan_craft_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => tool_error(format!("Could not build craft plan: {e}")),
            }
        }
        other => tool_error(format!("Unknown Ultros MCP tool: {other}")),
    }
}

pub(crate) async fn handle_mcp_post(
    State(state): State<WebState>,
    Json(req): Json<JsonRpcRequest>,
) -> Response {
    // Notifications carry no `id` and expect 202 Accepted with an empty body.
    let Some(id) = req.id else {
        return StatusCode::ACCEPTED.into_response();
    };

    match req.method.as_str() {
        "initialize" => {
            let client_version = req
                .params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or(MCP_PROTOCOL_VERSION);
            let negotiated = match client_version {
                "2024-11-05" | "2025-03-26" | "2025-06-18" => client_version,
                _ => MCP_PROTOCOL_VERSION,
            };
            rpc_ok(
                id,
                json!({
                    "protocolVersion": negotiated,
                    "capabilities": {
                        "tools": { "listChanged": false }
                    },
                    "serverInfo": {
                        "name": "ultros",
                        "version": env!("CARGO_PKG_VERSION"),
                        "title": "Ultros FFXIV Market Board & Gil-Making Tools",
                        "websiteUrl": "https://ultros.app",
                    },
                    "instructions": SERVER_INSTRUCTIONS,
                }),
            )
        }
        "ping" => rpc_ok(id, json!({})),
        "tools/list" => rpc_ok(id, json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let Some(name) = req.params.get("name").and_then(|v| v.as_str()) else {
                return rpc_err(id, -32602, "Missing required param: name");
            };
            let args = req
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let result = dispatch_tool(&state, name, &args).await;
            rpc_ok(id, result)
        }
        other => rpc_err(id, -32601, format!("Method not found: {other}")),
    }
}

/// `GET /mcp` — human/agent discovery summary (Streamable HTTP clients POST JSON-RPC here).
pub(crate) async fn handle_mcp_get() -> Response {
    let body = json!({
        "name": "ultros",
        "title": "Ultros FFXIV Market Board & Gil-Making MCP Server",
        "transport": "streamable-http",
        "endpoint": "https://ultros.app/mcp",
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "docs": "https://ultros.app/llms.txt",
        "openapi": "https://ultros.app/openapi.json",
        "help": "https://ultros.app/help/ai-agents",
        "tools": tool_definitions()
            .into_iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(ToOwned::to_owned))
            .collect::<Vec<_>>(),
    });
    let mut resp = (StatusCode::OK, Json(body)).into_response();
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    resp
}

/// Stateless server: `DELETE /mcp` acknowledges session teardown cleanly.
pub(crate) async fn handle_mcp_delete() -> StatusCode {
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[test]
    fn all_mcp_tools_have_schemas_and_read_only_annotations() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 11);
        for tool in &tools {
            assert!(tool["name"].is_string());
            assert!(tool["description"].is_string());
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["annotations"]["readOnlyHint"], true);
            assert_eq!(tool["annotations"]["destructiveHint"], false);
        }
    }

    #[tokio::test]
    async fn mcp_get_returns_discovery_metadata() {
        let resp = handle_mcp_get().await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["name"], "ultros");
        assert_eq!(body["transport"], "streamable-http");
        assert_eq!(body["tools"].as_array().unwrap().len(), 11);
    }

    #[tokio::test]
    async fn mcp_delete_returns_no_content() {
        assert_eq!(handle_mcp_delete().await, StatusCode::NO_CONTENT);
    }
}
