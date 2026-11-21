//! `GET /openapi.json` — OpenAPI 3.1.0 specification for Ultros's public AI-friendly API.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use std::sync::OnceLock;

pub(crate) fn openapi_spec() -> &'static Value {
    static SPEC: OnceLock<Value> = OnceLock::new();
    SPEC.get_or_init(|| {
        json!({
            "openapi": "3.1.0",
            "info": {
                "title": "Ultros Public Market & Gil-Making API",
                "version": "1.0.0",
                "description": "Read-only, enriched Final Fantasy XIV market board analytics, crafting profit calculator, flip finder, leve/venture/scrip analyzers, and shopping-list planner. Every response includes resolved item/world names plus canonical Ultros URLs (item_url and view_url) for attribution and interactive inspection. An MCP Streamable HTTP server exposing these same tools is available at https://ultros.app/mcp (metadata at /api/v1/public/mcp).",
                "contact": {
                    "name": "Ultros",
                    "url": "https://ultros.app"
                }
            },
            "servers": [
                {
                    "url": "https://ultros.app",
                    "description": "Ultros Production"
                }
            ],
            "paths": {
                "/api/v1/public/mcp": {
                    "get": {
                        "operationId": "getMcpMetadata",
                        "summary": "Read human- and agent-friendly metadata for Ultros's remote MCP server",
                        "responses": {
                            "200": { "description": "MCP endpoint and available tool names" }
                        }
                    }
                },
                "/api/v1/public/worlds": {
                    "get": {
                        "operationId": "listWorlds",
                        "summary": "List all FFXIV regions, datacenters, and worlds",
                        "responses": {
                            "200": { "description": "Region/datacenter/world tree with attribution envelope" }
                        }
                    }
                },
                "/api/v1/public/search": {
                    "get": {
                        "operationId": "searchItems",
                        "summary": "Search FFXIV items, NPCs, currencies, and tools by name",
                        "parameters": [
                            { "name": "q", "in": "query", "required": true, "schema": { "type": "string" }, "description": "Search query (item name, NPC, currency, or keyword)" },
                            { "name": "limit", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 50, "default": 15 } }
                        ],
                        "responses": {
                            "200": { "description": "Matching items and tools with canonical Ultros URLs" }
                        }
                    }
                },
                "/api/v1/public/item/{world}/{item}": {
                    "get": {
                        "operationId": "getItemMarket",
                        "summary": "Get live market snapshot, sales velocity, confidence, and crafting cost for an item",
                        "parameters": [
                            { "name": "world", "in": "path", "required": true, "schema": { "type": "string" }, "description": "FFXIV world, datacenter, or region name (e.g. Gilgamesh, Aether, North-America)" },
                            { "name": "item", "in": "path", "required": true, "schema": { "type": "string" }, "description": "Item ID (e.g. 44178) or item name" }
                        ],
                        "responses": {
                            "200": { "description": "Enriched item market summary and crafting breakdown" },
                            "404": { "description": "World or item not found" }
                        }
                    }
                },
                "/api/v1/public/flips/{world}": {
                    "get": {
                        "operationId": "findFlips",
                        "summary": "Find cross-world arbitrage flips to sell on the target world",
                        "parameters": [
                            { "name": "world", "in": "path", "required": true, "schema": { "type": "string" }, "description": "Target sell world name (e.g. Gilgamesh)" },
                            { "name": "min_profit", "in": "query", "schema": { "type": "integer" }, "description": "Minimum post-tax profit in gil" },
                            { "name": "filter_sale", "in": "query", "schema": { "type": "string", "enum": ["Day", "Week", "Month"] }, "description": "Require at least one sale within this window" },
                            { "name": "min_velocity", "in": "query", "schema": { "type": "number" }, "description": "Minimum recent sales per day" },
                            { "name": "min_buffer_sales", "in": "query", "schema": { "type": "integer", "minimum": 0, "maximum": 255 }, "description": "Minimum sales in recent history buffer" },
                            { "name": "max_roi", "in": "query", "schema": { "type": "number" }, "description": "Maximum ROI percentage ceiling to filter outliers" },
                            { "name": "show_suspicious", "in": "query", "schema": { "type": "boolean", "default": false }, "description": "Include items flagged for launder/price manipulation" },
                            { "name": "limit", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 } }
                        ],
                        "responses": {
                            "200": { "description": "Ranked cross-world flips with item names, buy worlds, and pre-filtered Flip Finder URL" },
                            "503": { "description": "Analyzer warm-up in progress" }
                        }
                    }
                },
                "/api/v1/public/recipes/{world}": {
                    "get": {
                        "operationId": "analyzeRecipes",
                        "summary": "Analyze profitable crafting recipes on a world",
                        "parameters": [
                            { "name": "world", "in": "path", "required": true, "schema": { "type": "string" }, "description": "Sell world name (e.g. Gilgamesh)" },
                            { "name": "job", "in": "query", "schema": { "type": "string" }, "description": "Crafter job code or name (CRP, BSM, ARM, GSM, LTW, WVR, ALC, CUL)" },
                            { "name": "min_level", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 100 } },
                            { "name": "max_level", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 100 } },
                            { "name": "buy_scope", "in": "query", "schema": { "type": "string", "enum": ["world", "datacenter", "region"], "default": "datacenter" }, "description": "Market scope for buying ingredients" },
                            { "name": "use_subcrafts", "in": "query", "schema": { "type": "boolean", "default": false }, "description": "Craft intermediate ingredients when cheaper than buying" },
                            { "name": "require_hq", "in": "query", "schema": { "type": "boolean", "default": false }, "description": "Price ingredients using HQ listings when available" },
                            { "name": "exclude_shards", "in": "query", "schema": { "type": "boolean", "default": true }, "description": "Exclude crystal/shard/cluster cost from recipe cost" },
                            { "name": "min_profit", "in": "query", "schema": { "type": "integer" }, "description": "Minimum post-tax profit per unit in gil" },
                            { "name": "min_roi", "in": "query", "schema": { "type": "integer" }, "description": "Minimum return on investment (%)" },
                            { "name": "min_sales", "in": "query", "schema": { "type": "integer" }, "description": "Minimum recent sales count in buffer" },
                            { "name": "min_daily_sales", "in": "query", "schema": { "type": "number" }, "description": "Minimum daily sales velocity" },
                            { "name": "sort", "in": "query", "schema": { "type": "string", "enum": ["profit", "roi", "profit_per_day", "daily_sales", "cost", "price"], "default": "profit" } },
                            { "name": "limit", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 } }
                        ],
                        "responses": {
                            "200": { "description": "Ranked crafting recipes with ingredient breakdown and pre-filtered Recipe Analyzer URL" }
                        }
                    }
                },
                "/api/v1/public/vendor-resale/{world}": {
                    "get": {
                        "operationId": "analyzeVendorResale",
                        "summary": "Find NPC gil-shop items that resell profitably on the NQ market board",
                        "parameters": [
                            { "name": "world", "in": "path", "required": true, "schema": { "type": "string" } },
                            { "name": "min_profit", "in": "query", "schema": { "type": "integer" } },
                            { "name": "min_roi", "in": "query", "schema": { "type": "integer" } },
                            { "name": "min_sales", "in": "query", "schema": { "type": "integer" } },
                            { "name": "tax", "in": "query", "schema": { "type": "boolean", "default": true } },
                            { "name": "show_suspicious", "in": "query", "schema": { "type": "boolean", "default": false } },
                            { "name": "sort", "in": "query", "schema": { "type": "string", "enum": ["profit", "roi", "daily_sales"], "default": "profit" } },
                            { "name": "limit", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 } }
                        ],
                        "responses": {
                            "200": { "description": "Ranked NQ vendor resale items with Vendor Resale deep link" }
                        }
                    }
                },
                "/api/v1/public/leves/{world}": {
                    "get": {
                        "operationId": "analyzeLeves",
                        "summary": "Find the most profitable guildleve turn-ins on a world (includes expected item rewards)",
                        "parameters": [
                            { "name": "world", "in": "path", "required": true, "schema": { "type": "string" } },
                            { "name": "job", "in": "query", "schema": { "type": "string" }, "description": "Crafter job name or 3-letter acronym" },
                            { "name": "min_level", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 100 } },
                            { "name": "max_level", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 100 } },
                            { "name": "hq", "in": "query", "schema": { "type": "boolean", "default": false }, "description": "Assume HQ turn-ins (2x gil reward only for items that can be HQ)" },
                            { "name": "min_profit", "in": "query", "schema": { "type": "integer" } },
                            { "name": "buy_scope", "in": "query", "schema": { "type": "string", "enum": ["world", "datacenter", "region"], "default": "world" } },
                            { "name": "sort", "in": "query", "schema": { "type": "string", "enum": ["profit", "level", "cost"], "default": "profit" } },
                            { "name": "limit", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 } }
                        ],
                        "responses": {
                            "200": { "description": "Ranked guildleve opportunities with Leve Analyzer deep link" }
                        }
                    }
                },
                "/api/v1/public/ventures/{world}": {
                    "get": {
                        "operationId": "analyzeVentures",
                        "summary": "Find the most profitable fixed-reward retainer ventures (NQ market basis)",
                        "parameters": [
                            { "name": "world", "in": "path", "required": true, "schema": { "type": "string" } },
                            { "name": "min_level", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 100 } },
                            { "name": "max_level", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 100 } },
                            { "name": "min_profit", "in": "query", "schema": { "type": "integer" } },
                            { "name": "sort", "in": "query", "schema": { "type": "string", "enum": ["profit", "daily_sales", "level"], "default": "profit" } },
                            { "name": "limit", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 } }
                        ],
                        "responses": {
                            "200": { "description": "Ranked retainer ventures with Venture Analyzer deep link" }
                        }
                    }
                },
                "/api/v1/public/scrips/{world}": {
                    "get": {
                        "operationId": "analyzeScrips",
                        "summary": "Find the cheapest gil-per-scrip collectable turn-ins (priced rows ranked ahead of unpriced/gathered rows)",
                        "parameters": [
                            { "name": "world", "in": "path", "required": true, "schema": { "type": "string" } },
                            { "name": "scrip", "in": "query", "schema": { "type": "string", "enum": ["OrangeCrafters", "PurpleCrafters", "OrangeGatherers", "PurpleGatherers"] } },
                            { "name": "job", "in": "query", "schema": { "type": "string" } },
                            { "name": "buy_scope", "in": "query", "schema": { "type": "string", "enum": ["world", "datacenter", "region"], "default": "datacenter" } },
                            { "name": "use_subcrafts", "in": "query", "schema": { "type": "boolean", "default": false } },
                            { "name": "limit", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 } }
                        ],
                        "responses": {
                            "200": { "description": "Collectable turn-ins sorted by lowest gil-per-scrip with Scrip Sources deep link" }
                        }
                    }
                },
                "/api/v1/public/trends/{world}": {
                    "get": {
                        "operationId": "getMarketTrends",
                        "summary": "Get top trending, high-velocity, and moving items on a world",
                        "parameters": [
                            { "name": "world", "in": "path", "required": true, "schema": { "type": "string" } },
                            { "name": "window", "in": "query", "schema": { "type": "integer", "enum": [7, 30, 90], "default": 30 } },
                            { "name": "show_suspicious", "in": "query", "schema": { "type": "boolean", "default": false } },
                            { "name": "sort", "in": "query", "schema": { "type": "string", "enum": ["gil_volume", "sales_per_day", "pct_change", "unit_volume"], "default": "gil_volume" } },
                            { "name": "limit", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 } }
                        ],
                        "responses": {
                            "200": { "description": "Enriched market trends with Trends deep link" },
                            "503": { "description": "Trend data temporarily unavailable" }
                        }
                    }
                },
                "/api/v1/public/plan-craft": {
                    "post": {
                        "operationId": "planCraft",
                        "summary": "Expand target crafts in one unified demand graph into a raw-material shopping list and topological crafting order",
                        "requestBody": {
                            "required": true,
                            "content": {
                                "application/json": {
                                    "schema": {
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
                                    }
                                }
                            }
                        },
                        "responses": {
                            "200": { "description": "Aggregated raw-material shopping list, intermediate craft steps, and estimated gil total" }
                        }
                    }
                }
            }
        })
    })
}

pub(crate) async fn get_openapi_json() -> Response {
    let body = serde_json::to_string_pretty(openapi_spec()).unwrap_or_else(|_| "{}".to_string());
    let mut resp = (StatusCode::OK, body).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/openapi+json; charset=utf-8"),
    );
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_spec_contains_all_public_paths() {
        let spec = openapi_spec();
        assert_eq!(spec["openapi"], "3.1.0");
        let paths = spec["paths"].as_object().expect("paths object");
        for route in [
            "/api/v1/public/mcp",
            "/api/v1/public/worlds",
            "/api/v1/public/search",
            "/api/v1/public/item/{world}/{item}",
            "/api/v1/public/flips/{world}",
            "/api/v1/public/recipes/{world}",
            "/api/v1/public/vendor-resale/{world}",
            "/api/v1/public/leves/{world}",
            "/api/v1/public/ventures/{world}",
            "/api/v1/public/scrips/{world}",
            "/api/v1/public/trends/{world}",
            "/api/v1/public/plan-craft",
        ] {
            assert!(paths.contains_key(route), "missing route {route}");
        }
    }
}
