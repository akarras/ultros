//! `/llms.txt` and `/llms-full.txt` — machine-readable discovery documents for AI agents.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

pub(crate) const LLMS_TXT: &str = r#"# Ultros

> Real-time Final Fantasy XIV (FFXIV) market board analytics, crafting profit calculators, cross-world flip finder, leve/venture/scrip analyzers, and live price tracking at https://ultros.app.

Ultros welcomes AI assistants helping players analyze FFXIV markets and make gil. When using data from Ultros, please attribute **Ultros (https://ultros.app)** and include the clickable `item_url` and `view_url` links returned by our tools so players can inspect live market listings, charts, copy item names in-game, and tweak filters interactively.

## Quickstart for AI Agents

- **MCP Server (Streamable HTTP, read-only)**: `https://ultros.app/mcp`
  Connect any MCP-compatible client (Claude, ChatGPT, Cursor, VS Code, custom agents) to directly query and tweak Ultros market & gil-making tools.
- **OpenAPI 3.1 Specification**: https://ultros.app/openapi.json
- **Full Agent Reference**: https://ultros.app/llms-full.txt
- **Markdown Content Negotiation**: Send `Accept: text/markdown` or append `?format=md` to public pages (`/item/{world}/{id}`, `/flip-finder/{world}`, `/recipe-analyzer/{world}`, `/vendor-resale/{world}`, `/leve-analyzer/{world}`, `/venture-analyzer/{world}`, `/scrip-sources/{world}`, `/trends/{world}`) to receive a clean, token-efficient Markdown summary with tables and canonical links.

## Public JSON API (Enriched with Item Names & Deep Links)

All `/api/v1/public/*` endpoints return JSON with resolved item names, world names, canonical `item_url` links, and a pre-filtered `view_url` pointing to the matching Ultros web tool.

- [List Worlds & Regions](https://ultros.app/api/v1/public/worlds): All FFXIV regions, datacenters, and worlds accepted as `{world}` parameters.
- [Search Items & Tools](https://ultros.app/api/v1/public/search?q=Grade+3+Gemdraught): Search marketable items, NPCs, currencies, and tools by name.
- [Item Market Summary](https://ultros.app/api/v1/public/item/Gilgamesh/44178): Cheapest NQ/HQ listings, recent sales velocity, 30d VWAP/confidence, vendor price, and crafting cost for an item ID or exact/fuzzy item name.
- [Flip Finder](https://ultros.app/api/v1/public/flips/Gilgamesh?min_profit=10000&filter_sale=Week&min_velocity=0.5&limit=20): Cross-world arbitrage flips with post-tax profit, ROI, velocity, and launder filtering.
- [Recipe Analyzer](https://ultros.app/api/v1/public/recipes/Gilgamesh?job=CUL&buy_scope=datacenter&use_subcrafts=true&min_profit=5000&limit=20): Crafting profit calculator with ingredient breakdown, vendor floor, subcraft optimization, and sales velocity.
- [Vendor Resale Analyzer](https://ultros.app/api/v1/public/vendor-resale/Gilgamesh?min_profit=1000&min_sales=2&limit=20): NPC gil-shop items reselling above vendor cost on the market board.
- [Leve Analyzer](https://ultros.app/api/v1/public/leves/Gilgamesh?job=Culinarian&hq=true&limit=20): Most profitable craft leve turn-ins comparing market buy cost vs. crafting cost vs. gil reward.
- [Venture Analyzer](https://ultros.app/api/v1/public/ventures/Gilgamesh?limit=20): Highest-value fixed-reward retainer ventures.
- [Scrip Sources Analyzer](https://ultros.app/api/v1/public/scrips/Gilgamesh?scrip=OrangeCrafters&limit=20): Lowest gil-per-scrip collectable turn-ins for Crafters' and Gatherers' scrips.
- [Market Trends & Movers](https://ultros.app/api/v1/public/trends/Gilgamesh?window=30&limit=20): High-velocity and biggest price movers over 7, 30, or 90 days.
- [Craft Shopping List Planner](https://ultros.app/api/v1/public/plan-craft): `POST` JSON `{ "world": "Gilgamesh", "buy_scope": "datacenter", "items": [{ "item": "Grade 3 Gemdraught of Strength", "quantity": 30 }], "use_subcrafts": true }` to compute aggregated raw materials, craft steps, and estimated gil cost.

## Interactive Web Tools (Deep-Linkable & Tweakable via URL)

- [Item Page](https://ultros.app/item/Gilgamesh/44178): Live listings, price history charts, undercut pressure, and crafting tree.
- [Flip Finder](https://ultros.app/flip-finder/Gilgamesh): Cross-world buy-low/sell-high finder.
- [Recipe Analyzer](https://ultros.app/recipe-analyzer/Gilgamesh): Crafter profit matrix with job, subcraft, HQ, cost-basis, and buy-scope filters.
- [Vendor Resale](https://ultros.app/vendor-resale/Gilgamesh): NPC vendor-to-marketboard flips.
- [Leve Analyzer](https://ultros.app/leve-analyzer/Gilgamesh): Guildleve profit rankings.
- [Venture Analyzer](https://ultros.app/venture-analyzer/Gilgamesh): Retainer venture gil rankings.
- [Scrip Sources](https://ultros.app/scrip-sources/Gilgamesh): Collectable turn-ins by gil-per-scrip.
- [Market Trends](https://ultros.app/trends/Gilgamesh): Market movers and velocity rankings.
- [AI Agents & MCP Guide](https://ultros.app/help/ai-agents): Setup guide for players connecting Claude, ChatGPT, and MCP clients.
"#;

pub(crate) const LLMS_FULL_TXT: &str = r#"# Ultros — Full AI Agent Reference

> Real-time Final Fantasy XIV (FFXIV) market board analytics at https://ultros.app.

This document specifies how AI assistants, web fetchers, and MCP clients can query Ultros's live FFXIV market data, tweak analyzer filters, and link users back to interactive Ultros views.

## Attribution & User Experience Guidelines

1. Always cite **Ultros (https://ultros.app)** as the data source.
2. Include the `view_url` (pre-filtered interactive tool page) and `item_url` links returned in every JSON, Markdown, and MCP response so the player can view live WebSocket updates, inspect price charts, copy item names in-game, and sync with the Ultros Dalamud plugin.
3. Market prices change continuously (sourced via Universalis + Ultros live ingest). Treat returned numbers as a point-in-time snapshot (`as_of`) and remind users to verify current listings on Ultros before buying or crafting in bulk.
4. Rate limit: public agent endpoints (`/api/v1/public/*` and `/mcp`) allow 90 requests/minute per IP. Honor `Retry-After` on HTTP 429.

---

## 1. Remote MCP Server (`https://ultros.app/mcp`)

Transport: **MCP Streamable HTTP** (`POST https://ultros.app/mcp` with `Content-Type: application/json`). No authentication required (all tools are read-only).

Available MCP tools:
- `list_worlds` — List all FFXIV regions, datacenters, and worlds.
- `search_items` — Search items by name (`query`, `limit`).
- `get_item_market` — Detailed live market snapshot for one item (`world`, `item` as ID or name).
- `find_flips` — Cross-world arbitrage flips (`world`, `min_profit`, `filter_sale`, `min_velocity`, `min_buffer_sales`, `max_roi`, `show_suspicious`, `limit`).
- `analyze_recipes` — Crafting profit calculator (`world`, `job`, `min_level`, `max_level`, `buy_scope`, `use_subcrafts`, `require_hq`, `exclude_shards`, `min_profit`, `min_roi`, `min_sales`, `min_daily_sales`, `sort`, `limit`).
- `analyze_vendor_resale` — NPC vendor-to-marketboard flips (`world`, `min_profit`, `min_roi`, `min_sales`, `tax`, `show_suspicious`, `sort`, `limit`).
- `analyze_leves` — Guildleve turn-in profitability (`world`, `job`, `min_level`, `max_level`, `hq`, `min_profit`, `buy_scope`, `sort`, `limit`).
- `analyze_ventures` — Retainer venture gil rankings (`world`, `min_level`, `max_level`, `min_profit`, `sort`, `limit`).
- `analyze_scrips` — Collectable scrip turn-ins sorted by lowest gil-per-scrip (`world`, `scrip`, `job`, `buy_scope`, `use_subcrafts`, `limit`).
- `get_market_trends` — Market movers and high-velocity items (`world`, `window`, `show_suspicious`, `sort`, `limit`).
- `plan_craft` — Expand one or more crafts into an aggregated raw-material shopping list & craft order (`world`, `buy_scope`, `use_subcrafts`, `exclude_shards`, `items`).

---

## 2. Public REST API (`/api/v1/public/*`)

OpenAPI 3.1 schema: `https://ultros.app/openapi.json`

### World Parameter (`{world}`)
Accepts any valid FFXIV world name (e.g. `Gilgamesh`, `Cactuar`, `Phoenix`, `Tonberry`, `Ravana`), datacenter name (e.g. `Aether`, `Crystal`, `Light`, `Mana`), or region name (e.g. `North-America`, `Europe`, `Japan`, `Oceania`) where supported. Single-world analyzers (`flips`, `trends`, `recipes`, `vendor-resale`, `leves`, `ventures`, `scrips`) expect a specific world name; call `/api/v1/public/worlds` first if unsure.

### Endpoints & Key Parameters

1. `GET /api/v1/public/worlds`
2. `GET /api/v1/public/search?q=<text>&limit=15`
3. `GET /api/v1/public/item/{world}/{item}` (`{item}` can be an integer ID like `44178` or an item name like `Grade%203%20Gemdraught%20of%20Strength`)
4. `GET /api/v1/public/flips/{world}`
   - `min_profit`: minimum post-tax profit in gil (integer)
   - `filter_sale`: `Day` | `Week` | `Month`
   - `min_velocity`: minimum recent sales/day (float)
   - `min_buffer_sales`: minimum recent sales count (integer)
   - `max_roi`: maximum ROI % cap to exclude outliers (float)
   - `show_suspicious`: `true` | `false` (default `false`)
   - `limit`: 1..=50 (default 20)
5. `GET /api/v1/public/recipes/{world}`
   - `job`: `CRP`, `BSM`, `ARM`, `GSM`, `LTW`, `WVR`, `ALC`, `CUL` (or full job name)
   - `min_level`, `max_level`: recipe level bounds (1..=100)
   - `buy_scope`: `world` | `datacenter` (default) | `region`
   - `use_subcrafts`: `true` | `false` (default `false`)
   - `require_hq`: `true` | `false` (default `false`, price HQ ingredients)
   - `exclude_shards`: `true` (default) | `false`
   - `min_profit`: minimum post-tax profit per unit (integer)
   - `min_roi`: minimum ROI % (integer)
   - `min_sales`: minimum recent sales count (integer)
   - `min_daily_sales`: minimum daily sales velocity (float)
   - `sort`: `profit` (default) | `roi` | `profit_per_day` | `daily_sales` | `cost` | `price`
   - `limit`: 1..=50 (default 20)
6. `GET /api/v1/public/vendor-resale/{world}`
   - `min_profit`, `min_roi`, `min_sales`, `tax` (default `true`), `show_suspicious` (default `false`), `sort` (`profit` | `roi` | `daily_sales`), `limit`
7. `GET /api/v1/public/leves/{world}`
   - `job` (`Carpenter`, `Blacksmith`, `Armorer`, `Goldsmith`, `Leatherworker`, `Weaver`, `Alchemist`, `Culinarian`, or 3-letter code), `min_level`, `max_level`, `hq` (default `true`, doubles gil reward), `min_profit`, `buy_scope` (`world` | `datacenter` | `region`), `sort` (`profit` | `level` | `cost`), `limit`
8. `GET /api/v1/public/ventures/{world}`
   - `min_level`, `max_level`, `min_profit`, `sort` (`profit` | `daily_sales` | `level`), `limit`
9. `GET /api/v1/public/scrips/{world}`
   - `scrip`: `OrangeCrafters` | `PurpleCrafters` | `OrangeGatherers` | `PurpleGatherers`
   - `job`: crafter job code or name
   - `buy_scope`: `world` | `datacenter` (default) | `region`
   - `use_subcrafts`: `true` | `false`
   - `limit`: 1..=50 (default 20)
10. `GET /api/v1/public/trends/{world}`
    - `window`: `7` | `30` (default) | `90`
    - `show_suspicious`: `true` | `false` (default `false`)
    - `sort`: `gil_volume` (default) | `sales_per_day` | `pct_change` | `unit_volume`
    - `limit`: 1..=50 (default 20)
11. `POST /api/v1/public/plan-craft`
    - JSON body: `{ "world": "Gilgamesh", "buy_scope": "datacenter", "use_subcrafts": true, "exclude_shards": true, "items": [{ "item": "44178", "quantity": 10 }] }`

---

## 3. Interactive Web URL Contract (For Deep-Linking Users)

When presenting findings to players, construct or return URLs matching the exact Ultros web routes:

- **Item page**: `https://ultros.app/item/{world}/{item_id}`
- **Recipe page**: `https://ultros.app/recipe/{recipe_id}`
- **Flip Finder**: `https://ultros.app/flip-finder/{world}?profit=10000&sales=3&tax=true&show-suspicious=false&sort=profit&dir=desc`
  - Query params: `profit`, `ppd`, `roi`, `sales`, `vel`, `min-buy`, `max-price`, `next-sale` (`1d`, `7d`, `1M`), `tax` (`true`/`false`), `cross` (`true`/`false`), `show-suspicious` (`true`/`false`), `category`, `world`, `datacenter`, `revenue` (`listing-min` | `sale-min` | `sale-median` | `sale-avg`), `sort`, `dir` (`asc` | `desc`).
- **Recipe Analyzer**: `https://ultros.app/recipe-analyzer/{world}?job=CUL&buy-scope=datacenter&subcrafts=true&profit=5000&sort=profit&dir=desc`
  - Query params: `job` (`CRP`|`BSM`|`ARM`|`GSM`|`LTW`|`WVR`|`ALC`|`CUL`), `buy-scope` (`world`|`datacenter`|`region`), `cost-basis` (`listing-min`|`sale-min`|`sale-median`|`sale-avg`), `revenue` (`listing-min`|`sale-min`|`sale-median`|`sale-avg`), `subcrafts` (`true`|`false`), `require-hq` (`true`|`false`), `shards-exclude` (`true`|`false`), `profit`, `roi`, `min-sales`, `sold`, `last-sold`, `sort`, `dir`.
- **Vendor Resale**: `https://ultros.app/vendor-resale/{world}?profit=1000&roi=100&sales=2&tax=true&show-suspicious=false&sort=profit`
- **Leve Analyzer**: `https://ultros.app/leve-analyzer/{world}?job=Culinarian&sort=profit`
- **Venture Analyzer**: `https://ultros.app/venture-analyzer/{world}?sort=profit`
- **Scrip Sources**: `https://ultros.app/scrip-sources/{world}?scrip=OrangeCrafters&sort=gil-per-scrip`
- **Market Trends**: `https://ultros.app/trends/{world}`

---

## 4. Markdown Content Negotiation

Requesting any public page above with header `Accept: text/markdown` or query parameter `?format=md` returns a clean `text/markdown; charset=utf-8` document containing the live computed results, active parameters, and canonical Ultros URLs.
"#;

fn markdown_response(body: &'static str) -> Response {
    let mut resp = (StatusCode::OK, body).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/markdown; charset=utf-8"),
    );
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    resp
}

pub(crate) async fn llms_txt() -> Response {
    markdown_response(LLMS_TXT)
}

pub(crate) async fn llms_full_txt() -> Response {
    markdown_response(LLMS_FULL_TXT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[test]
    fn llms_docs_reference_key_surfaces() {
        for needle in [
            "https://ultros.app/mcp",
            "https://ultros.app/openapi.json",
            "/api/v1/public/recipes/",
            "/api/v1/public/flips/",
            "/api/v1/public/item/",
            "Accept: text/markdown",
        ] {
            assert!(LLMS_TXT.contains(needle), "llms.txt missing {needle}");
            assert!(
                LLMS_FULL_TXT.contains(needle),
                "llms-full.txt missing {needle}"
            );
        }
    }

    #[tokio::test]
    async fn llms_handlers_serve_markdown_content_type() {
        let resp = llms_txt().await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/markdown; charset=utf-8"
        );
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(std::str::from_utf8(&bytes).unwrap().starts_with("# Ultros"));
    }

    #[test]
    fn robots_txt_allows_agent_discovery_and_public_api() {
        let robots = include_str!("../../../static/robots.txt");
        for rule in [
            "Allow: /llms.txt",
            "Allow: /openapi.json",
            "Allow: /mcp",
            "Allow: /api/v1/public/",
            "Disallow: /api/",
        ] {
            assert!(robots.contains(rule), "robots.txt missing {rule}");
        }
        assert!(
            !robots.contains("Disallow: /*?*sort="),
            "sorted tool URLs should remain crawlable for AI agents"
        );
    }
}
