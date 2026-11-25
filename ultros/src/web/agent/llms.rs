//! `/llms.txt` and `/llms-full.txt` — machine-readable discovery documents for AI agents.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

pub(crate) const LLMS_TXT: &str = r#"# Ultros

> Real-time Final Fantasy XIV (FFXIV) market board analytics, crafting profit calculators, cross-world flip finder, leve/venture/scrip analyzers, and live price tracking at https://ultros.app.

Ultros welcomes AI assistants helping players analyze FFXIV markets and make gil. When using data from Ultros, please attribute **Ultros (https://ultros.app)** and include the clickable `item_url` and `view_url` links returned by our tools so players can inspect live market listings, charts, copy item names in-game, and tweak filters interactively.

## Quickstart for AI Agents & Developers

- **MCP Server (Streamable HTTP, read-only)**: `https://ultros.app/mcp` (POST JSON-RPC 2.0)
  Connect any MCP-compatible client (Claude, ChatGPT, Cursor, VS Code, custom agents) to directly query and tweak Ultros market & gil-making tools.
- **Interactive API Documentation**: https://ultros.app/api-docs
- **OpenAPI 3.1 Specification**: https://ultros.app/openapi.json
- **Full Agent Reference**: https://ultros.app/llms-full.txt
- **Markdown Content Negotiation**: Send `Accept: text/markdown` or append `?format=md` to public pages (`/item/{world}/{id}`, `/flip-finder/{world}`, `/recipe-analyzer/{world}`, `/vendor-resale/{world}`, `/leve-analyzer/{world}`, `/venture-analyzer/{world}`, `/scrip-sources/{world}`, `/trends/{world}`) to receive a clean, token-efficient Markdown summary with tables and canonical links.

## Unauthenticated REST API (`/api/v1/*`)

See https://ultros.app/api-docs or https://ultros.app/openapi.json for full request/response schemas across all unauthenticated market, history, analytics, retainer, and metadata endpoints:

- [World Hierarchy](https://ultros.app/api/v1/world_data): All FFXIV regions, datacenters, and worlds accepted as `{world}` parameters.
- [Search](https://ultros.app/api/v1/search?q=Grade+3+Gemdraught): Search marketable items, NPCs, currencies, and tools.
- [Item Listings & Sales](https://ultros.app/api/v1/listings/Gilgamesh/44178): Active listings, retainers, last 200 sales, and per-world ingest timestamps.
- [Cheapest Listings](https://ultros.app/api/v1/cheapest/North-America): Cheapest NQ/HQ listing for every item in a world, datacenter, or region (`?format=columnar` supported).
- [Recent Sales](https://ultros.app/api/v1/recentSales/Gilgamesh): Recent sale buffer (`?format=columnar` supported).
- [Price Series](https://ultros.app/api/v1/price_series/Gilgamesh/44178): Server-bucketed OHLC, quantiles (p25/p50/p75), volume, and optional raw sales.
- [Best Deals (Flips)](https://ultros.app/api/v1/best_deals/Gilgamesh?min_profit=10000&filter_sale=Week): Cross-world arbitrage flips to sell on a world.
- [Market Trends](https://ultros.app/api/v1/trends/Gilgamesh?window=30): High-velocity and moving items over 7, 30, or 90 days.
- [Market Pulse](https://ultros.app/api/v1/market_pulse/Gilgamesh) & [Market Movers](https://ultros.app/api/v1/movers/Gilgamesh): 24h KPIs and biggest price/volume movers.
- [Bulk Sale & Listing Stats](https://ultros.app/api/v1/sale_stats/Gilgamesh?window=7): Aggregated 1/7/30/90-day sale stats and alive listing stats.

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
2. Include the `view_url` (pre-filtered interactive tool page) and `item_url` links returned in every MCP and Markdown response so the player can view live WebSocket updates, inspect price charts, copy item names in-game, and sync with the Ultros Dalamud plugin.
3. Market prices change continuously (sourced via Universalis + Ultros live ingest). Treat returned numbers as a point-in-time snapshot and remind users to verify current listings on Ultros before buying or crafting in bulk.
4. Rate limit: `/mcp` and Markdown content-negotiated routes share a 90 requests/minute per-IP budget (IPv6 `/64`). Honor `Retry-After` on HTTP 429.

---

## 1. Remote MCP Server (`https://ultros.app/mcp`)

Transport: **MCP Streamable HTTP** (`POST https://ultros.app/mcp` with `Content-Type: application/json`, protocol revisions `2025-06-18` and `2025-03-26`). `GET /mcp` returns `405 Method Not Allowed` (`Allow: POST, DELETE`) per Streamable HTTP. No authentication required (all tools are read-only).

Available MCP tools:
- `list_worlds` — List all FFXIV regions, datacenters, and worlds.
- `search_items` — Search items by name (`query`, `limit`).
- `get_item_market` — Detailed live market snapshot for one item (`world`, `item` as ID or name).
- `find_flips` — Cross-world arbitrage flips (`world`, `min_profit`, `filter_sale`, `min_velocity`, `min_buffer_sales`, `max_roi`, `show_suspicious`, `limit`).
- `analyze_recipes` — Crafting profit calculator (`world`, `job`, `min_level`, `max_level`, `buy_scope`, `use_subcrafts`, `require_hq`, `exclude_shards`, `min_profit`, `min_roi`, `min_sales`, `min_daily_sales`, `sort`, `limit`).
- `analyze_vendor_resale` — NPC vendor-to-marketboard flips on NQ listings (`world`, `min_profit`, `min_roi`, `min_sales`, `tax`, `show_suspicious`, `sort`, `limit`).
- `analyze_leves` — Guildleve turn-in profitability including expected item rewards (`world`, `job`, `min_level`, `max_level`, `hq`, `min_profit`, `buy_scope`, `sort`, `limit`).
- `analyze_ventures` — Retainer venture gil rankings on NQ listings (`world`, `min_level`, `max_level`, `min_profit`, `sort`, `limit`).
- `analyze_scrips` — Collectable scrip turn-ins sorted by lowest gil-per-scrip (`world`, `scrip`, `job`, `buy_scope`, `use_subcrafts`, `limit`).
- `get_market_trends` — Market movers and high-velocity items (`world`, `window`, `show_suspicious`, `sort`, `limit`).
- `plan_craft` — Expand one or more crafts in a unified demand graph into a raw-material shopping list & topological craft order (`world`, `buy_scope`, `use_subcrafts`, `exclude_shards`, `items`).

---

## 2. Unauthenticated REST API (`/api/v1/*`)

Interactive docs: `https://ultros.app/api-docs`
OpenAPI 3.1 schema: `https://ultros.app/openapi.json`

### World Parameter (`{world}`)
Accepts any valid FFXIV world name (e.g. `Gilgamesh`, `Cactuar`, `Phoenix`, `Tonberry`, `Ravana`), datacenter name (e.g. `Aether`, `Crystal`, `Light`, `Mana`), or region name (e.g. `North-America`, `Europe`, `Japan`, `Oceania`) where supported. Call `GET /api/v1/world_data` for the full tree.

### Key Endpoints
- **Market Data**: `GET /api/v1/listings/{world}/{itemid}`, `GET /api/v1/bulkListings/{world}/{itemids}`, `GET /api/v1/cheapest/{world}`, `GET /api/v1/recentSales/{world}`, `GET /api/v1/extended_history/{world}/{itemid}`, `GET /api/v1/realtime/events` (WebSocket)
- **Price History & Charts**: `GET /api/v1/price_series/{world}/{itemid}`, `GET /api/v1/price_density/{world}/{itemid}`, `GET /api/v1/floor_history/{world}/{itemid}`, `POST /api/v1/floor_history/{world}`, `GET /api/v1/undercut_pressure/{world}/{itemid}`
- **Market Analytics**: `GET /api/v1/best_deals/{world}`, `GET /api/v1/trends/{world}`, `GET /api/v1/market_pulse/{world}`, `GET /api/v1/market_heat/{world}`, `GET /api/v1/movers/{world}`, `POST /api/v1/sparklines/{world}`, `POST /api/v1/resale_quality/{world}`, `GET /api/v1/item_stats/{world}/{itemid}`, `GET /api/v1/sale_stats/{world}`, `GET /api/v1/listing_stats/{world}`
- **Retainers**: `GET /api/v1/retainer/search/{query}`, `GET /api/v1/retainer/listings/{id}`
- **Metadata**: `GET /api/v1/world_data`, `GET /api/v1/search?q=<text>`, `GET /api/v1/game-history`, `GET /api/v1/changelog`, `GET /api/v1/detectregion`, `GET /api/v1/push/vapid-public-key`

---

## 3. Interactive Web URL Contract (For Deep-Linking Users)

When presenting findings to players, construct or return URLs matching the exact Ultros web routes:

- **Item page**: `https://ultros.app/item/{world}/{item_id}`
- **Recipe page**: `https://ultros.app/recipe/{recipe_id}`
- **Flip Finder**: `https://ultros.app/flip-finder/{world}?profit=10000&sales=3&tax=true&show-suspicious=false&sort=profit&dir=desc`
  - Query params: `profit`, `ppd`, `roi`, `sales`, `vel`, `min-buy`, `max-price`, `next-sale` (`1d`, `7d`, `1M`), `tax` (`true`/`false`), `cross` (`true`/`false`), `show-suspicious` (`true`/`false`), `category`, `world`, `datacenter`, `revenue` (`listing-min` | `sale-min` | `sale-median` | `sale-avg`), `sort`, `dir` (`asc` | `desc`).
- **Recipe Analyzer**: `https://ultros.app/recipe-analyzer/{world}?job=CUL&buy-scope=datacenter&subcrafts=true&profit=5000&sort=profit&dir=desc`
  - Query params: `job` (`CRP`|`BSM`|`ARM`|`GSM`|`LTW`|`WVR`|`ALC`|`CUL`), `buy-scope` (`world`|`datacenter`|`region`), `cost-basis` (`listing-min`|`sale-min`|`sale-median`|`sale-avg`), `revenue` (`listing-min`|`sale-min`|`sale-median`|`sale-avg`), `subcrafts` (`true`|`false`), `require-hq` (`true`|`false`), `shards-exclude` (`true`|`false`), `profit`, `roi`, `min-sales` (daily sales/day), `sold`, `last-sold`, `sort` (`profit`|`roi`|`cost`|`price`|`velocity`), `dir` (`asc`|`desc`).
- **Vendor Resale**: `https://ultros.app/vendor-resale/{world}?profit=1000&roi=100&sales=2&tax=true&show-suspicious=false&sort=profit`
- **Leve Analyzer**: `https://ultros.app/leve-analyzer/{world}?job=Culinarian&sort=profit`
- **Venture Analyzer**: `https://ultros.app/venture-analyzer/{world}?sort=profit`
- **Scrip Sources**: `https://ultros.app/scrip-sources/{world}?scrip=OrangeCrafters`
- **Market Trends**: `https://ultros.app/trends/{world}?window=30&show_suspicious=false`

---

## 4. Markdown Content Negotiation

Requesting any public page above with header `Accept: text/markdown` or query parameter `?format=md` returns a clean `text/markdown; charset=utf-8` document containing live computed results, active parameters, and canonical Ultros URLs.
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
            "https://ultros.app/api-docs",
            "https://ultros.app/openapi.json",
            "/api/v1/world_data",
            "/api/v1/listings/",
            "Accept: text/markdown",
        ] {
            assert!(LLMS_TXT.contains(needle), "llms.txt missing {needle}");
            assert!(
                LLMS_FULL_TXT.contains(needle),
                "llms-full.txt missing {needle}"
            );
        }
        assert!(!LLMS_TXT.contains("/api/v1/public/"));
        assert!(!LLMS_FULL_TXT.contains("/api/v1/public/"));
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
    fn robots_txt_allows_root_and_protects_private_and_sort_crawls() {
        let robots = include_str!("../../../static/robots.txt");
        for rule in ["Allow: /", "Disallow: /api/", "Disallow: /*?*sort="] {
            assert!(robots.contains(rule), "robots.txt missing {rule}");
        }
        assert!(!robots.contains("/api/v1/public/"));
    }
}
