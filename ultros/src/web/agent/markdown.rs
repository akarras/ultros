//! `Accept: text/markdown` and `?format=md` content negotiation for public Ultros routes.
//!
//! Safety & caching invariants:
//! - Normal browsers send `Accept: text/html,...` and pass straight through to Leptos SSR.
//! - Matched Markdown routes enforce the **same** shared [`AgentRateLimiter`] as REST and MCP
//!   before running any analyzer or database query.
//! - Header-negotiated Markdown (`Accept: text/markdown`) sets `Cache-Control: private, no-store`
//!   and `Vary: Accept` so CDNs/shared caches can never serve Markdown to HTML browsers.
//!   Explicit `?format=md` URLs have a distinct cache key and use `public, max-age=30`.
//! - Every Markdown response sets `X-Robots-Tag: noindex`, a `Link: <canonical>; rel="canonical"`
//!   header, and public CORS (`Access-Control-Allow-Origin: *`).
//! - Errors on matched Markdown routes return typed Markdown error responses instead of
//!   falling back to a second full Leptos SSR render.

use std::collections::HashMap;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::web::error::WebError;
use crate::web::state::WebState;

use super::SITE_ORIGIN;
use super::analyzers::{
    PublicFlipsQuery, PublicLevesQuery, PublicRecipesQuery, PublicScripsQuery, PublicTrendsQuery,
    PublicVendorResaleQuery, PublicVenturesQuery, format_flips_md, format_item_market_md,
    format_leves_md, format_recipes_md, format_scrips_md, format_trends_md,
    format_vendor_resale_md, format_ventures_md, query_flips, query_item_market, query_leves,
    query_recipes, query_scrips, query_trends, query_vendor_resale, query_ventures,
};
use super::llms::LLMS_TXT;
use super::rate_limit::{
    AgentRateLimiter, RateDecision, apply_rate_headers, too_many_requests_response,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MarkdownTrigger {
    QueryParam,
    AcceptHeader,
}

/// Returns `Some(trigger)` only when the caller explicitly requested Markdown over HTML,
/// via `?format=md` / `?format=markdown` or an `Accept` header preferring `text/markdown`.
pub(crate) fn markdown_trigger(headers: &HeaderMap, query: &str) -> Option<MarkdownTrigger> {
    if query
        .split('&')
        .any(|pair| matches!(pair, "format=md" | "format=markdown"))
    {
        return Some(MarkdownTrigger::QueryParam);
    }

    let accept = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok())?;

    let mut md_q: Option<f32> = None;
    let mut html_q: Option<f32> = None;

    for part in accept.split(',') {
        let mut segments = part.split(';').map(str::trim);
        let Some(media) = segments.next() else {
            continue;
        };
        let q = segments
            .find_map(|s| s.strip_prefix("q=").and_then(|v| v.parse::<f32>().ok()))
            .unwrap_or(1.0);
        if media.eq_ignore_ascii_case("text/markdown")
            || media.eq_ignore_ascii_case("text/x-markdown")
        {
            md_q = Some(md_q.map_or(q, |prev| prev.max(q)));
        } else if media.eq_ignore_ascii_case("text/html")
            || media.eq_ignore_ascii_case("application/xhtml+xml")
        {
            html_q = Some(html_q.map_or(q, |prev| prev.max(q)));
        }
    }

    match (md_q, html_q) {
        (Some(m), Some(h)) if m > 0.0 && m >= h => Some(MarkdownTrigger::AcceptHeader),
        (Some(m), None) if m > 0.0 => Some(MarkdownTrigger::AcceptHeader),
        _ => None,
    }
}

fn parse_query_map(query: &str) -> HashMap<String, String> {
    url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn get_i32(map: &HashMap<String, String>, keys: &[&str]) -> Option<i32> {
    keys.iter()
        .find_map(|k| map.get(*k))
        .and_then(|v| v.parse::<i32>().ok())
}

fn get_i64(map: &HashMap<String, String>, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|k| map.get(*k))
        .and_then(|v| v.parse::<i64>().ok())
}

fn get_usize(map: &HashMap<String, String>, keys: &[&str]) -> Option<usize> {
    keys.iter()
        .find_map(|k| map.get(*k))
        .and_then(|v| v.parse::<usize>().ok())
}

fn get_bool(map: &HashMap<String, String>, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|k| map.get(*k)).and_then(|v| {
        match v.trim().to_ascii_lowercase().as_str() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        }
    })
}

fn is_supported_markdown_route(parts: &[&str]) -> bool {
    matches!(
        parts,
        [] | ["item", _, _]
            | ["item", _]
            | ["flip-finder", _]
            | ["analyzer", _]
            | ["recipe-analyzer", _]
            | ["vendor-resale", _]
            | ["leve-analyzer", _]
            | ["venture-analyzer", _]
            | ["scrip-sources", _]
            | ["trends", _]
    )
}

fn build_markdown_response(
    status: StatusCode,
    body: String,
    canonical_url: &str,
    trigger: MarkdownTrigger,
    limit: u32,
    decision: &RateDecision,
) -> Response {
    let mut resp = (status, body).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/markdown; charset=utf-8"),
    );
    resp.headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
    resp.headers_mut()
        .insert("x-robots-tag", HeaderValue::from_static("noindex"));
    resp.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    if let Ok(link_val) = HeaderValue::from_str(&format!("<{canonical_url}>; rel=\"canonical\"")) {
        resp.headers_mut().insert(header::LINK, link_val);
    }
    let cache_control = match (status, trigger) {
        (StatusCode::OK, MarkdownTrigger::QueryParam) => "public, max-age=30",
        // Header-negotiated variants on an HTML URL must never be stored in shared caches
        // (CDNs like Cloudflare ignore `Vary: Accept` for HTML).
        _ => "private, no-store",
    };
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    apply_rate_headers(&mut resp, limit, decision);
    resp
}

fn markdown_error_response(
    err: WebError,
    canonical_url: &str,
    trigger: MarkdownTrigger,
    limit: u32,
    decision: &RateDecision,
) -> Response {
    let (status, msg) = match &err {
        WebError::NotFound => (
            StatusCode::NOT_FOUND,
            "# 404 Not Found\n\nThe requested world or item was not found on Ultros (https://ultros.app).\n",
        ),
        WebError::BadRequest => (
            StatusCode::BAD_REQUEST,
            "# 400 Bad Request\n\nInvalid filter or world parameter. See https://ultros.app/llms.txt for supported parameters.\n",
        ),
        WebError::AnalyzerError(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "# 503 Service Unavailable\n\nUltros market data is warming up or temporarily unavailable. Please retry shortly.\n",
        ),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            "# 503 Service Unavailable\n\nUltros could not compute this view right now. Please retry shortly.\n",
        ),
    };
    tracing::warn!(error = %err, canonical_url, "markdown route failed");
    metrics::counter!(
        "ultros_agent_requests_total",
        "surface" => "markdown",
        "status" => status.as_u16().to_string()
    )
    .increment(1);
    build_markdown_response(
        status,
        msg.to_string(),
        canonical_url,
        trigger,
        limit,
        decision,
    )
}

pub(crate) async fn markdown_negotiation(
    State((state, limiter)): State<(WebState, AgentRateLimiter)>,
    req: Request,
    next: Next,
) -> Response {
    if req.method() != Method::GET {
        return next.run(req).await;
    }

    let query_str = req.uri().query().unwrap_or_default();
    let Some(trigger) = markdown_trigger(req.headers(), query_str) else {
        return next.run(req).await;
    };

    let path = req.uri().path();
    let trimmed = path.trim_matches('/');
    let parts: Vec<&str> = if trimmed.is_empty() {
        Vec::new()
    } else {
        trimmed.split('/').collect()
    };

    if !is_supported_markdown_route(&parts) {
        return next.run(req).await;
    }

    // Enforce the shared public agent rate limit BEFORE running any DB/analyzer work.
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0);
    let decision = limiter.check_request(req.headers(), peer);
    if !decision.allowed {
        return too_many_requests_response(limiter.limit(), &decision, true);
    }

    let canonical_url = format!("{SITE_ORIGIN}{path}");
    let qmap = parse_query_map(query_str);

    let result: Result<(String, String), WebError> = match parts.as_slice() {
        [] => Ok((LLMS_TXT.to_string(), format!("{SITE_ORIGIN}/"))),
        ["item", world, item] => query_item_market(&state, world, item)
            .await
            .map(|e| (format_item_market_md(&e), e.view_url)),
        ["item", item] => {
            let scope = qmap
                .get("world")
                .map(String::as_str)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("North-America");
            query_item_market(&state, scope, item)
                .await
                .map(|e| (format_item_market_md(&e), e.view_url))
        }
        ["flip-finder", world] | ["analyzer", world] => {
            let q = PublicFlipsQuery {
                min_profit: get_i32(&qmap, &["min_profit", "profit"]),
                filter_sale: qmap.get("filter_sale").cloned(),
                min_velocity: qmap
                    .get("min_velocity")
                    .or_else(|| qmap.get("vel"))
                    .and_then(|v| v.parse().ok()),
                min_buffer_sales: qmap
                    .get("min_buffer_sales")
                    .or_else(|| qmap.get("sales"))
                    .and_then(|v| v.parse().ok()),
                max_roi: qmap.get("max_roi").and_then(|v| v.parse().ok()),
                show_suspicious: get_bool(&qmap, &["show_suspicious", "show-suspicious"]),
                limit: get_usize(&qmap, &["limit"]),
            };
            query_flips(&state, world, q)
                .await
                .map(|e| (format_flips_md(&e), e.view_url))
        }
        ["recipe-analyzer", world] => {
            let q = PublicRecipesQuery {
                job: qmap.get("job").cloned(),
                min_level: qmap.get("min_level").and_then(|v| v.parse().ok()),
                max_level: qmap.get("max_level").and_then(|v| v.parse().ok()),
                buy_scope: qmap
                    .get("buy_scope")
                    .or_else(|| qmap.get("buy-scope"))
                    .cloned(),
                use_subcrafts: get_bool(&qmap, &["use_subcrafts", "subcrafts"]),
                require_hq: get_bool(&qmap, &["require_hq", "require-hq"]),
                exclude_shards: get_bool(&qmap, &["exclude_shards", "shards-exclude"]),
                min_profit: get_i32(&qmap, &["min_profit", "profit"]),
                min_roi: get_i32(&qmap, &["min_roi", "roi"]),
                min_sales: get_usize(&qmap, &["min_sales"]),
                min_daily_sales: qmap
                    .get("min_daily_sales")
                    .or_else(|| qmap.get("min-sales"))
                    .and_then(|v| v.parse().ok()),
                sort: qmap.get("sort").cloned(),
                limit: get_usize(&qmap, &["limit"]),
            };
            query_recipes(&state, world, q)
                .await
                .map(|e| (format_recipes_md(&e), e.view_url))
        }
        ["vendor-resale", world] => {
            let q = PublicVendorResaleQuery {
                min_profit: get_i32(&qmap, &["min_profit", "profit"]),
                min_roi: get_i32(&qmap, &["min_roi", "roi"]),
                min_sales: get_usize(&qmap, &["min_sales", "sales"]),
                tax: get_bool(&qmap, &["tax"]),
                show_suspicious: get_bool(&qmap, &["show_suspicious", "show-suspicious"]),
                sort: qmap.get("sort").cloned(),
                limit: get_usize(&qmap, &["limit"]),
            };
            query_vendor_resale(&state, world, q)
                .await
                .map(|e| (format_vendor_resale_md(&e), e.view_url))
        }
        ["leve-analyzer", world] => {
            let q = PublicLevesQuery {
                job: qmap.get("job").cloned(),
                min_level: qmap.get("min_level").and_then(|v| v.parse().ok()),
                max_level: qmap.get("max_level").and_then(|v| v.parse().ok()),
                hq: get_bool(&qmap, &["hq"]),
                min_profit: get_i32(&qmap, &["min_profit", "profit"]),
                buy_scope: qmap
                    .get("buy_scope")
                    .or_else(|| qmap.get("buy-scope"))
                    .cloned(),
                sort: qmap.get("sort").cloned(),
                limit: get_usize(&qmap, &["limit"]),
            };
            query_leves(&state, world, q)
                .await
                .map(|e| (format_leves_md(&e), e.view_url))
        }
        ["venture-analyzer", world] => {
            let q = PublicVenturesQuery {
                min_level: qmap.get("min_level").and_then(|v| v.parse().ok()),
                max_level: qmap.get("max_level").and_then(|v| v.parse().ok()),
                min_profit: get_i64(&qmap, &["min_profit", "profit"]),
                sort: qmap.get("sort").cloned(),
                limit: get_usize(&qmap, &["limit"]),
            };
            query_ventures(&state, world, q)
                .await
                .map(|e| (format_ventures_md(&e), e.view_url))
        }
        ["scrip-sources", world] => {
            let q = PublicScripsQuery {
                scrip: qmap.get("scrip").cloned(),
                job: qmap.get("job").cloned(),
                buy_scope: qmap
                    .get("buy_scope")
                    .or_else(|| qmap.get("buy-scope"))
                    .cloned(),
                use_subcrafts: get_bool(&qmap, &["use_subcrafts", "subcrafts"]),
                limit: get_usize(&qmap, &["limit"]),
            };
            query_scrips(&state, world, q)
                .await
                .map(|e| (format_scrips_md(&e), e.view_url))
        }
        ["trends", world] => {
            let q = PublicTrendsQuery {
                window: qmap.get("window").and_then(|v| v.parse().ok()),
                show_suspicious: get_bool(&qmap, &["show_suspicious", "show-suspicious"]),
                sort: qmap.get("sort").cloned(),
                limit: get_usize(&qmap, &["limit"]),
            };
            query_trends(&state, world, q)
                .await
                .map(|e| (format_trends_md(&e), e.view_url))
        }
        _ => return next.run(req).await,
    };

    match result {
        Ok((md, canonical)) => {
            metrics::counter!(
                "ultros_agent_requests_total",
                "surface" => "markdown",
                "status" => "200"
            )
            .increment(1);
            build_markdown_response(
                StatusCode::OK,
                md,
                &canonical,
                trigger,
                limiter.limit(),
                &decision,
            )
        }
        Err(err) => {
            markdown_error_response(err, &canonical_url, trigger, limiter.limit(), &decision)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn browser_accept_headers_do_not_trigger_markdown() {
        let mut h = HeaderMap::new();
        h.insert(
            header::ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        assert_eq!(markdown_trigger(&h, ""), None);
        assert_eq!(markdown_trigger(&h, "sort=profit"), None);
    }

    #[test]
    fn explicit_markdown_accept_or_query_triggers_expected_variant() {
        let mut h = HeaderMap::new();
        h.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/markdown, text/html;q=0.5"),
        );
        assert_eq!(
            markdown_trigger(&h, ""),
            Some(MarkdownTrigger::AcceptHeader)
        );

        let mut html_preferred = HeaderMap::new();
        html_preferred.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/html;q=1.0, text/markdown;q=0.8"),
        );
        assert_eq!(markdown_trigger(&html_preferred, ""), None);
        assert_eq!(
            markdown_trigger(&html_preferred, "job=CUL&format=md"),
            Some(MarkdownTrigger::QueryParam)
        );
    }

    #[test]
    fn accept_header_markdown_is_private_no_store_while_query_param_is_public() {
        let decision = RateDecision {
            allowed: true,
            remaining: 89,
            retry_after_secs: 0,
        };
        let via_header = build_markdown_response(
            StatusCode::OK,
            "# Hi".into(),
            "https://ultros.app/flip-finder/Gilgamesh",
            MarkdownTrigger::AcceptHeader,
            90,
            &decision,
        );
        assert_eq!(
            via_header.headers().get(header::CACHE_CONTROL).unwrap(),
            "private, no-store"
        );
        assert_eq!(via_header.headers().get("x-robots-tag").unwrap(), "noindex");
        assert_eq!(
            via_header.headers().get(header::LINK).unwrap(),
            "<https://ultros.app/flip-finder/Gilgamesh>; rel=\"canonical\""
        );

        let via_query = build_markdown_response(
            StatusCode::OK,
            "# Hi".into(),
            "https://ultros.app/flip-finder/Gilgamesh",
            MarkdownTrigger::QueryParam,
            90,
            &decision,
        );
        assert_eq!(
            via_query.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=30"
        );
    }

    #[test]
    fn shared_rate_limiter_budget_applies_across_rest_mcp_and_markdown() {
        let limiter = AgentRateLimiter::new(2, Duration::from_secs(60), vec![]);
        let h = HeaderMap::new();
        let peer: Option<SocketAddr> = Some("198.51.100.5:12345".parse().unwrap());

        // Request 1 (e.g. REST), Request 2 (e.g. MCP) exhaust the shared budget.
        assert!(limiter.check_request(&h, peer).allowed);
        assert!(limiter.check_request(&h, peer).allowed);

        // Request 3 (e.g. Markdown middleware) is blocked with 429 + Retry-After.
        let d3 = limiter.check_request(&h, peer);
        assert!(!d3.allowed);
        let resp = too_many_requests_response(limiter.limit(), &d3, true);
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/markdown; charset=utf-8"
        );
        assert!(resp.headers().contains_key(header::RETRY_AFTER));
    }
}
