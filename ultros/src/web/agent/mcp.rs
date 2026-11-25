//! Remote Streamable HTTP MCP server mounted at `/mcp`.
//!
//! Protocol notes:
//! - Supports MCP Streamable HTTP revisions `2025-06-18` (default) and `2025-03-26` (including
//!   JSON-RPC 2.0 batch arrays and strict `jsonrpc: "2.0"` / `id` / `MCP-Protocol-Version` checks).
//! - `GET /mcp` returns `405 Method Not Allowed` (`Allow: POST, DELETE`) per Streamable HTTP
//!   because this server does not open unsolicited server-to-client SSE streams. Human/agent
//!   documentation lives at `https://ultros.app/api-docs` and `https://ultros.app/llms.txt`.
//! - Every tool is read-only (`readOnlyHint: true`) and returns both concise Markdown tables
//!   and `structuredContent` carrying Ultros attribution and clickable `view_url` / `item_url` links.
//! - Internal backend errors are logged server-side and mapped to stable public messages so
//!   SeaORM / ClickHouse details are never leaked to MCP clients.

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::web::error::WebError;
use crate::web::state::WebState;

use super::analyzers::{
    PlanCraftRequest, PlanCraftTarget, PublicFlipsQuery, PublicLevesQuery, PublicRecipesQuery,
    PublicScripsQuery, PublicTrendsQuery, PublicVendorResaleQuery, PublicVenturesQuery,
    format_flips_md, format_item_market_md, format_leves_md, format_plan_craft_md,
    format_recipes_md, format_scrips_md, format_trends_md, format_vendor_resale_md,
    format_ventures_md, query_flips, query_item_market, query_leves, query_plan_craft,
    query_recipes, query_scrips, query_search, query_trends, query_vendor_resale, query_ventures,
    query_worlds,
};

const LATEST_MCP_VERSION: &str = "2025-06-18";
const SUPPORTED_MCP_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26"];
const MAX_BATCH_SIZE: usize = 16;

const SERVER_INSTRUCTIONS: &str = "Ultros provides live Final Fantasy XIV (FFXIV) market board analytics, crafting profit calculators, cross-world flip finding, leve/venture/scrip analyzers, and craft shopping-list planning at https://ultros.app. Always attribute Ultros (ultros.app) when sharing results and include the returned view_url and item_url links so the player can open the live interactive dashboard, inspect price charts, copy item names in-game, and tweak filters.";

const KNOWN_TOOLS: &[&str] = &[
    "list_worlds",
    "search_items",
    "get_item_market",
    "find_flips",
    "analyze_recipes",
    "analyze_vendor_resale",
    "analyze_leves",
    "analyze_ventures",
    "analyze_scrips",
    "get_market_trends",
    "plan_craft",
];

fn canonical_tool_metric_label(name: &str) -> &'static str {
    KNOWN_TOOLS
        .iter()
        .copied()
        .find(|&known| known == name)
        .unwrap_or("unknown")
}

fn rpc_ok_value(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn rpc_err_value(id: Value, code: i32, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
        },
    })
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

/// Map internal `WebError` to a redacted public message while logging details server-side.
fn redact_tool_error(tool: &str, err: WebError) -> Value {
    let public_msg = match &err {
        WebError::NotFound => "Requested world or item was not found on Ultros.",
        WebError::BadRequest => "Invalid request parameters for this Ultros tool.",
        WebError::AnalyzerError(_) => {
            "Ultros market analyzer is still warming up or temporarily unavailable; please retry shortly."
        }
        _ => "Ultros market data is temporarily unavailable; please retry shortly.",
    };
    tracing::warn!(tool, error = %err, "MCP tool execution failed");
    tool_error(public_msg)
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
                    "min_sales": { "type": "integer", "description": "Minimum recent sales count in buffer" },
                    "min_daily_sales": { "type": "number", "description": "Minimum daily sales velocity" },
                    "sort": { "type": "string", "enum": ["profit", "roi", "profit_per_day", "daily_sales", "cost", "price"], "default": "profit" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 }
                }
            },
            "annotations": read_only_annotations("Analyze Profitable Crafting Recipes")
        }),
        json!({
            "name": "analyze_vendor_resale",
            "description": "Find NPC gil-shop items that resell profitably on a world's NQ market board. Returns vendor cost, NQ market price, post-tax profit, ROI, velocity, and a pre-filtered Vendor Resale view_url.",
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
            "description": "Rank FFXIV craft guildleve turn-ins by net gil profit (including expected item rewards), comparing market buy cost vs. whole-batch crafting cost.",
            "inputSchema": {
                "type": "object",
                "required": ["world"],
                "properties": {
                    "world": { "type": "string" },
                    "job": { "type": "string", "description": "Crafter job name or 3-letter acronym" },
                    "min_level": { "type": "integer", "minimum": 1, "maximum": 100 },
                    "max_level": { "type": "integer", "minimum": 1, "maximum": 100 },
                    "hq": { "type": "boolean", "default": false, "description": "Assume HQ turn-ins (2x gil reward only for can_be_hq items)" },
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
            "description": "Rank fixed-reward FFXIV retainer ventures by estimated net gil value on a world (NQ market basis).",
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
            "description": "Find the cheapest gil-per-scrip collectable turn-ins for Crafters' and Gatherers' scrips on a world (priced crafted turn-ins ranked ahead of unpriced gathered rows).",
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
            "description": "Expand one or more target crafts in one unified demand graph into a raw-material shopping list (with cheapest world/vendor prices) and bottom-up topological crafting order.",
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

// --- Strict MCP argument extractors (reject wrong types with -32602) --------

fn req_str_arg(args: &Value, key: &str) -> Result<String, String> {
    match opt_str_arg(args, key)? {
        Some(s) => Ok(s),
        None => Err(format!("Missing required string argument: {key}")),
    }
}

fn opt_str_arg(args: &Value, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok((!s.trim().is_empty()).then(|| s.clone())),
        Some(Value::Number(n)) if key == "item" => Ok(Some(n.to_string())),
        Some(_) => Err(format!("Argument '{key}' must be a string")),
    }
}

fn opt_i64_arg(args: &Value, key: &str) -> Result<Option<i64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_i64()
            .map(Some)
            .ok_or_else(|| format!("Argument '{key}' must be an integer")),
    }
}

fn opt_i32_arg(args: &Value, key: &str) -> Result<Option<i32>, String> {
    opt_i64_arg(args, key)?
        .map(|n| i32::try_from(n).map_err(|_| format!("Argument '{key}' is out of i32 range")))
        .transpose()
}

fn opt_usize_arg(args: &Value, key: &str) -> Result<Option<usize>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| format!("Argument '{key}' must be a non-negative integer")),
    }
}

fn opt_f32_arg(args: &Value, key: &str) -> Result<Option<f32>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|f| f.is_finite())
            .map(|n| Some(n as f32))
            .ok_or_else(|| format!("Argument '{key}' must be a finite number")),
    }
}

fn opt_bool_arg(args: &Value, key: &str) -> Result<Option<bool>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("Argument '{key}' must be a boolean")),
    }
}

fn parse_plan_craft_args(args: &Value) -> Result<PlanCraftRequest, String> {
    let world = req_str_arg(args, "world")?;
    let buy_scope = opt_str_arg(args, "buy_scope")?;
    let use_subcrafts = opt_bool_arg(args, "use_subcrafts")?;
    let exclude_shards = opt_bool_arg(args, "exclude_shards")?;
    let raw_items = args
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Missing required array argument: items".to_string())?;
    if raw_items.is_empty() || raw_items.len() > 20 {
        return Err("Argument 'items' must contain between 1 and 20 targets".to_string());
    }
    let mut items = Vec::with_capacity(raw_items.len());
    for entry in raw_items {
        let item = req_str_arg(entry, "item")?;
        let quantity = opt_i64_arg(entry, "quantity")?
            .filter(|&q| (1..=9999).contains(&q))
            .ok_or_else(|| "Each target requires integer 'quantity' in 1..=9999".to_string())?;
        items.push(PlanCraftTarget { item, quantity });
    }
    Ok(PlanCraftRequest {
        world,
        buy_scope,
        use_subcrafts,
        exclude_shards,
        items,
    })
}

/// Validate tool name & arguments before executing; returns `Err(msg)` for JSON-RPC `-32602`.
fn validate_known_tool(name: &str, args: &Value) -> Result<(), String> {
    if !KNOWN_TOOLS.contains(&name) {
        return Err(format!("Unknown tool: {name}"));
    }
    if !args.is_object() {
        return Err("Tool arguments must be a JSON object".to_string());
    }
    Ok(())
}

async fn dispatch_tool(state: &WebState, name: &str, args: &Value) -> Result<Value, String> {
    let metric_label = canonical_tool_metric_label(name);
    metrics::counter!(
        "ultros_agent_requests_total",
        "surface" => "mcp",
        "tool" => metric_label
    )
    .increment(1);

    validate_known_tool(name, args)?;

    let out = match name {
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
            let query = req_str_arg(args, "query")?;
            let limit = opt_usize_arg(args, "limit")?;
            let env = query_search(state, &query, limit);
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
            let world = req_str_arg(args, "world")?;
            let item = req_str_arg(args, "item")?;
            match query_item_market(state, &world, &item).await {
                Ok(env) => tool_success(
                    format_item_market_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => redact_tool_error(metric_label, e),
            }
        }
        "find_flips" => {
            let world = req_str_arg(args, "world")?;
            let min_buffer_sales = opt_i64_arg(args, "min_buffer_sales")?
                .map(|n| {
                    u8::try_from(n)
                        .map_err(|_| "Argument 'min_buffer_sales' must be 0..=255".to_string())
                })
                .transpose()?;
            let q = PublicFlipsQuery {
                min_profit: opt_i32_arg(args, "min_profit")?,
                filter_sale: opt_str_arg(args, "filter_sale")?,
                min_velocity: opt_f32_arg(args, "min_velocity")?,
                min_buffer_sales,
                max_roi: opt_f32_arg(args, "max_roi")?,
                show_suspicious: opt_bool_arg(args, "show_suspicious")?,
                limit: opt_usize_arg(args, "limit")?,
            };
            match query_flips(state, &world, q).await {
                Ok(env) => tool_success(
                    format_flips_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => redact_tool_error(metric_label, e),
            }
        }
        "analyze_recipes" => {
            let world = req_str_arg(args, "world")?;
            let min_level = opt_i64_arg(args, "min_level")?
                .map(|n| {
                    i8::try_from(n).map_err(|_| "Argument 'min_level' must be 1..=100".to_string())
                })
                .transpose()?;
            let max_level = opt_i64_arg(args, "max_level")?
                .map(|n| {
                    i8::try_from(n).map_err(|_| "Argument 'max_level' must be 1..=100".to_string())
                })
                .transpose()?;
            let q = PublicRecipesQuery {
                job: opt_str_arg(args, "job")?,
                min_level,
                max_level,
                buy_scope: opt_str_arg(args, "buy_scope")?,
                use_subcrafts: opt_bool_arg(args, "use_subcrafts")?,
                require_hq: opt_bool_arg(args, "require_hq")?,
                exclude_shards: opt_bool_arg(args, "exclude_shards")?,
                min_profit: opt_i32_arg(args, "min_profit")?,
                min_roi: opt_i32_arg(args, "min_roi")?,
                min_sales: opt_usize_arg(args, "min_sales")?,
                min_daily_sales: opt_f32_arg(args, "min_daily_sales")?,
                sort: opt_str_arg(args, "sort")?,
                limit: opt_usize_arg(args, "limit")?,
            };
            match query_recipes(state, &world, q).await {
                Ok(env) => tool_success(
                    format_recipes_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => redact_tool_error(metric_label, e),
            }
        }
        "analyze_vendor_resale" => {
            let world = req_str_arg(args, "world")?;
            let q = PublicVendorResaleQuery {
                min_profit: opt_i32_arg(args, "min_profit")?,
                min_roi: opt_i32_arg(args, "min_roi")?,
                min_sales: opt_usize_arg(args, "min_sales")?,
                tax: opt_bool_arg(args, "tax")?,
                show_suspicious: opt_bool_arg(args, "show_suspicious")?,
                sort: opt_str_arg(args, "sort")?,
                limit: opt_usize_arg(args, "limit")?,
            };
            match query_vendor_resale(state, &world, q).await {
                Ok(env) => tool_success(
                    format_vendor_resale_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => redact_tool_error(metric_label, e),
            }
        }
        "analyze_leves" => {
            let world = req_str_arg(args, "world")?;
            let min_level = opt_i64_arg(args, "min_level")?
                .map(|n| {
                    u16::try_from(n).map_err(|_| "Argument 'min_level' must be 1..=100".to_string())
                })
                .transpose()?;
            let max_level = opt_i64_arg(args, "max_level")?
                .map(|n| {
                    u16::try_from(n).map_err(|_| "Argument 'max_level' must be 1..=100".to_string())
                })
                .transpose()?;
            let q = PublicLevesQuery {
                job: opt_str_arg(args, "job")?,
                min_level,
                max_level,
                hq: opt_bool_arg(args, "hq")?,
                min_profit: opt_i32_arg(args, "min_profit")?,
                buy_scope: opt_str_arg(args, "buy_scope")?,
                sort: opt_str_arg(args, "sort")?,
                limit: opt_usize_arg(args, "limit")?,
            };
            match query_leves(state, &world, q).await {
                Ok(env) => tool_success(
                    format_leves_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => redact_tool_error(metric_label, e),
            }
        }
        "analyze_ventures" => {
            let world = req_str_arg(args, "world")?;
            let min_level = opt_i64_arg(args, "min_level")?
                .map(|n| {
                    u8::try_from(n).map_err(|_| "Argument 'min_level' must be 1..=100".to_string())
                })
                .transpose()?;
            let max_level = opt_i64_arg(args, "max_level")?
                .map(|n| {
                    u8::try_from(n).map_err(|_| "Argument 'max_level' must be 1..=100".to_string())
                })
                .transpose()?;
            let q = PublicVenturesQuery {
                min_level,
                max_level,
                min_profit: opt_i64_arg(args, "min_profit")?,
                sort: opt_str_arg(args, "sort")?,
                limit: opt_usize_arg(args, "limit")?,
            };
            match query_ventures(state, &world, q).await {
                Ok(env) => tool_success(
                    format_ventures_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => redact_tool_error(metric_label, e),
            }
        }
        "analyze_scrips" => {
            let world = req_str_arg(args, "world")?;
            let q = PublicScripsQuery {
                scrip: opt_str_arg(args, "scrip")?,
                job: opt_str_arg(args, "job")?,
                buy_scope: opt_str_arg(args, "buy_scope")?,
                use_subcrafts: opt_bool_arg(args, "use_subcrafts")?,
                limit: opt_usize_arg(args, "limit")?,
            };
            match query_scrips(state, &world, q).await {
                Ok(env) => tool_success(
                    format_scrips_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => redact_tool_error(metric_label, e),
            }
        }
        "get_market_trends" => {
            let world = req_str_arg(args, "world")?;
            let window = opt_i64_arg(args, "window")?
                .map(|n| {
                    u16::try_from(n)
                        .ok()
                        .filter(|w| matches!(w, 7 | 30 | 90))
                        .ok_or_else(|| "Argument 'window' must be 7, 30, or 90".to_string())
                })
                .transpose()?;
            let q = PublicTrendsQuery {
                window,
                show_suspicious: opt_bool_arg(args, "show_suspicious")?,
                sort: opt_str_arg(args, "sort")?,
                limit: opt_usize_arg(args, "limit")?,
            };
            match query_trends(state, &world, q).await {
                Ok(env) => tool_success(
                    format_trends_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => redact_tool_error(metric_label, e),
            }
        }
        "plan_craft" => {
            let req = parse_plan_craft_args(args)?;
            match query_plan_craft(state, req).await {
                Ok(env) => tool_success(
                    format_plan_craft_md(&env),
                    serde_json::to_value(&env).unwrap_or(Value::Null),
                ),
                Err(e) => redact_tool_error(metric_label, e),
            }
        }
        _ => return Err(format!("Unknown tool: {name}")),
    };
    Ok(out)
}

/// Process a single JSON-RPC 2.0 message object. Returns `None` for notifications and
/// client-originated JSON-RPC responses (which expect HTTP 202 Accepted with no body).
async fn process_single_message(state: Option<&WebState>, msg: Value) -> Option<Value> {
    let Some(obj) = msg.as_object() else {
        return Some(rpc_err_value(
            Value::Null,
            -32600,
            "Invalid Request: expected JSON object",
        ));
    };

    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        let id = obj.get("id").cloned().unwrap_or(Value::Null);
        return Some(rpc_err_value(
            id,
            -32600,
            "Invalid Request: jsonrpc must be \"2.0\"",
        ));
    }

    // Client-originated JSON-RPC response (has `result` or `error` and no `method`).
    let Some(method_val) = obj.get("method") else {
        if obj.contains_key("result") || obj.contains_key("error") {
            return None;
        }
        let id = obj.get("id").cloned().unwrap_or(Value::Null);
        return Some(rpc_err_value(id, -32600, "Invalid Request: missing method"));
    };

    let Some(method) = method_val.as_str() else {
        let id = obj.get("id").cloned().unwrap_or(Value::Null);
        return Some(rpc_err_value(
            id,
            -32600,
            "Invalid Request: method must be a string",
        ));
    };

    // Distinguish absent `id` (notification -> no response) from explicit `id: null` (invalid in MCP).
    let id = match obj.get("id") {
        None => return None,
        Some(v) if v.is_string() || v.is_number() => v.clone(),
        Some(_) => {
            return Some(rpc_err_value(
                Value::Null,
                -32600,
                "Invalid Request: id must be a string or number",
            ));
        }
    };

    let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "initialize" => {
            let client_version = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(LATEST_MCP_VERSION);
            let negotiated = if SUPPORTED_MCP_VERSIONS.contains(&client_version) {
                client_version
            } else {
                LATEST_MCP_VERSION
            };
            Some(rpc_ok_value(
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
            ))
        }
        "ping" => Some(rpc_ok_value(id, json!({}))),
        "tools/list" => Some(rpc_ok_value(id, json!({ "tools": tool_definitions() }))),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(rpc_err_value(id, -32602, "Missing required param: name"));
            };
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if let Err(msg) = validate_known_tool(name, &args) {
                metrics::counter!(
                    "ultros_agent_requests_total",
                    "surface" => "mcp",
                    "tool" => canonical_tool_metric_label(name)
                )
                .increment(1);
                return Some(rpc_err_value(id, -32602, msg));
            }
            let Some(state) = state else {
                return Some(rpc_err_value(id, -32603, "Server state unavailable"));
            };
            match dispatch_tool(state, name, &args).await {
                Ok(result) => Some(rpc_ok_value(id, result)),
                Err(msg) => Some(rpc_err_value(id, -32602, msg)),
            }
        }
        other => Some(rpc_err_value(
            id,
            -32601,
            format!("Method not found: {other}"),
        )),
    }
}

pub(crate) async fn handle_mcp_bytes(
    state: Option<&WebState>,
    headers: &HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(version) = headers
        .get("mcp-protocol-version")
        .and_then(|v| v.to_str().ok())
        && !SUPPORTED_MCP_VERSIONS.contains(&version.trim())
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(rpc_err_value(
                Value::Null,
                -32600,
                format!(
                    "Unsupported MCP-Protocol-Version '{version}'. Supported: {}",
                    SUPPORTED_MCP_VERSIONS.join(", ")
                ),
            )),
        )
            .into_response();
    }

    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::OK,
                Json(rpc_err_value(
                    Value::Null,
                    -32700,
                    "Parse error: invalid JSON",
                )),
            )
                .into_response();
        }
    };

    match parsed {
        Value::Array(items) => {
            if items.is_empty() || items.len() > MAX_BATCH_SIZE {
                return (
                    StatusCode::OK,
                    Json(rpc_err_value(
                        Value::Null,
                        -32600,
                        format!("Invalid Request: batch size must be 1..={MAX_BATCH_SIZE}"),
                    )),
                )
                    .into_response();
            }
            let mut responses = Vec::new();
            for item in items {
                if let Some(resp) = process_single_message(state, item).await {
                    responses.push(resp);
                }
            }
            if responses.is_empty() {
                StatusCode::ACCEPTED.into_response()
            } else {
                (StatusCode::OK, Json(Value::Array(responses))).into_response()
            }
        }
        single => match process_single_message(state, single).await {
            Some(resp) => (StatusCode::OK, Json(resp)).into_response(),
            None => StatusCode::ACCEPTED.into_response(),
        },
    }
}

pub(crate) async fn handle_mcp_post(
    State(state): State<WebState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_mcp_bytes(Some(&state), &headers, body).await
}

/// Per MCP Streamable HTTP spec, servers that do not offer an unsolicited GET SSE stream
/// MUST return `405 Method Not Allowed` on `GET /mcp`.
pub(crate) async fn handle_mcp_get() -> Response {
    let mut resp = (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({
            "error": "method_not_allowed",
            "message": "Ultros MCP uses Streamable HTTP via POST /mcp (no unsolicited GET SSE stream). See https://ultros.app/llms.txt or https://ultros.app/api-docs for documentation.",
            "docs_url": "https://ultros.app/api-docs",
            "openapi_url": "https://ultros.app/openapi.json",
        })),
    )
        .into_response();
    resp.headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("POST, DELETE"));
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

    async fn call_mcp(headers: HeaderMap, body: &str) -> (StatusCode, Vec<u8>) {
        let resp = handle_mcp_bytes(None, &headers, Bytes::from(body.to_owned())).await;
        let status = resp.status();
        let bytes = resp
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        (status, bytes)
    }

    #[test]
    fn all_mcp_tools_have_schemas_and_read_only_annotations() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), KNOWN_TOOLS.len());
        for tool in &tools {
            let name = tool["name"].as_str().unwrap();
            assert!(KNOWN_TOOLS.contains(&name));
            assert!(tool["description"].is_string());
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["annotations"]["readOnlyHint"], true);
            assert_eq!(tool["annotations"]["destructiveHint"], false);
        }
        assert_eq!(canonical_tool_metric_label("find_flips"), "find_flips");
        assert_eq!(
            canonical_tool_metric_label("attacker_random_12345"),
            "unknown"
        );
    }

    #[tokio::test]
    async fn mcp_get_returns_405_with_allow_header() {
        let get_resp = handle_mcp_get().await;
        assert_eq!(get_resp.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            get_resp.headers().get(header::ALLOW).unwrap(),
            "POST, DELETE"
        );
    }

    #[tokio::test]
    async fn mcp_post_handles_initialize_ping_notifications_batches_and_errors() {
        // Initialize negotiates newest supported revision.
        let (st, bytes) = call_mcp(
            HeaderMap::new(),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2099-01-01"}}"#,
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["result"]["protocolVersion"], LATEST_MCP_VERSION);

        // Notification (absent id) -> 202 Accepted with empty body.
        let (st, bytes) = call_mcp(
            HeaderMap::new(),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        )
        .await;
        assert_eq!(st, StatusCode::ACCEPTED);
        assert!(bytes.is_empty());

        // Explicit `id: null` is rejected with -32600, NOT treated as a notification.
        let (st, bytes) = call_mcp(
            HeaderMap::new(),
            r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], -32600);

        // Malformed JSON -> -32700.
        let (st, bytes) = call_mcp(HeaderMap::new(), "{not json").await;
        assert_eq!(st, StatusCode::OK);
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], -32700);

        // Unknown tool -> -32602.
        let (st, bytes) = call_mcp(
            HeaderMap::new(),
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"bogus_tool","arguments":{}}}"#,
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], -32602);

        // Batch array containing a ping and a notification returns 1-element JSON array.
        let (st, bytes) = call_mcp(
            HeaderMap::new(),
            r#"[{"jsonrpc":"2.0","id":3,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#,
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["id"], 3);

        // Unsupported MCP-Protocol-Version header -> 400 Bad Request.
        let mut bad_ver = HeaderMap::new();
        bad_ver.insert(
            "mcp-protocol-version",
            HeaderValue::from_static("1999-01-01"),
        );
        let (st, _) = call_mcp(bad_ver, r#"{"jsonrpc":"2.0","id":4,"method":"ping"}"#).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn mcp_delete_returns_no_content() {
        assert_eq!(handle_mcp_delete().await, StatusCode::NO_CONTENT);
    }
}
