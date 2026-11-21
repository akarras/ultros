//! `Accept: text/markdown` and `?format=md` content negotiation for public Ultros routes.
//!
//! Normal browsers send `Accept: text/html,...` and pass straight through to Leptos SSR.
//! AI fetchers that send `Accept: text/markdown` (or append `?format=md`) get a clean,
//! token-efficient Markdown summary with live data, tables, and canonical Ultros URLs.

use std::collections::HashMap;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::web::state::WebState;

use super::analyzers::{
    PublicFlipsQuery, PublicLevesQuery, PublicRecipesQuery, PublicScripsQuery, PublicTrendsQuery,
    PublicVendorResaleQuery, PublicVenturesQuery, format_flips_md, format_item_market_md,
    format_leves_md, format_recipes_md, format_scrips_md, format_trends_md,
    format_vendor_resale_md, format_ventures_md, query_flips, query_item_market, query_leves,
    query_recipes, query_scrips, query_trends, query_vendor_resale, query_ventures,
};
use super::llms::LLMS_TXT;

/// Returns true only when the caller explicitly requested Markdown over HTML, via
/// `?format=md` / `?format=markdown` or an `Accept` header preferring `text/markdown`.
pub(crate) fn wants_markdown(headers: &HeaderMap, query: &str) -> bool {
    if query
        .split('&')
        .any(|pair| matches!(pair, "format=md" | "format=markdown"))
    {
        return true;
    }

    let Some(accept) = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()) else {
        return false;
    };

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
        (Some(m), Some(h)) => m > 0.0 && m >= h,
        (Some(m), None) => m > 0.0,
        _ => false,
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

fn markdown_ok(body: String) -> Response {
    let mut resp = (StatusCode::OK, body).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/markdown; charset=utf-8"),
    );
    resp.headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=30"),
    );
    resp
}

pub(crate) async fn markdown_negotiation(
    State(state): State<WebState>,
    req: Request,
    next: Next,
) -> Response {
    if req.method() != Method::GET {
        return next.run(req).await;
    }

    let query_str = req.uri().query().unwrap_or_default();
    if !wants_markdown(req.headers(), query_str) {
        return next.run(req).await;
    }

    let path = req.uri().path();
    let trimmed = path.trim_matches('/');
    let parts: Vec<&str> = if trimmed.is_empty() {
        Vec::new()
    } else {
        trimmed.split('/').collect()
    };

    let qmap = parse_query_map(query_str);
    let rendered: Option<String> = match parts.as_slice() {
        [] => Some(LLMS_TXT.to_string()),
        ["item", world, item] => query_item_market(&state, world, item)
            .await
            .ok()
            .map(|e| format_item_market_md(&e)),
        ["item", item] => query_item_market(&state, "North-America", item)
            .await
            .ok()
            .map(|e| format_item_market_md(&e)),
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
                .ok()
                .map(|e| format_flips_md(&e))
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
                min_sales: get_usize(&qmap, &["min_sales", "min-sales"]),
                min_daily_sales: qmap.get("min_daily_sales").and_then(|v| v.parse().ok()),
                sort: qmap.get("sort").cloned(),
                limit: get_usize(&qmap, &["limit"]),
            };
            query_recipes(&state, world, q)
                .await
                .ok()
                .map(|e| format_recipes_md(&e))
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
                .ok()
                .map(|e| format_vendor_resale_md(&e))
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
                .ok()
                .map(|e| format_leves_md(&e))
        }
        ["venture-analyzer", world] => {
            let q = PublicVenturesQuery {
                min_level: qmap.get("min_level").and_then(|v| v.parse().ok()),
                max_level: qmap.get("max_level").and_then(|v| v.parse().ok()),
                min_profit: get_i32(&qmap, &["min_profit", "profit"]),
                sort: qmap.get("sort").cloned(),
                limit: get_usize(&qmap, &["limit"]),
            };
            query_ventures(&state, world, q)
                .await
                .ok()
                .map(|e| format_ventures_md(&e))
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
                .ok()
                .map(|e| format_scrips_md(&e))
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
                .ok()
                .map(|e| format_trends_md(&e))
        }
        _ => None,
    };

    if let Some(md) = rendered {
        metrics::counter!(
            "ultros_agent_requests_total",
            "surface" => "markdown",
            "status" => "200"
        )
        .increment(1);
        return markdown_ok(md);
    }

    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_accept_headers_do_not_trigger_markdown() {
        let mut h = HeaderMap::new();
        h.insert(
            header::ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        assert!(!wants_markdown(&h, ""));
        assert!(!wants_markdown(&h, "sort=profit"));
    }

    #[test]
    fn explicit_markdown_accept_or_query_triggers_markdown() {
        let mut h = HeaderMap::new();
        h.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/markdown, text/html;q=0.5"),
        );
        assert!(wants_markdown(&h, ""));

        let mut html_preferred = HeaderMap::new();
        html_preferred.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/html;q=1.0, text/markdown;q=0.8"),
        );
        assert!(!wants_markdown(&html_preferred, ""));
        assert!(wants_markdown(&html_preferred, "job=CUL&format=md"));
    }
}
