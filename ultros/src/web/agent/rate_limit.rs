//! Lightweight per-IP rate limiter for public AI agent endpoints (`/api/v1/public/*`, `/mcp`).
//!
//! Keeps public read-only automation safe from runaway loops without adding external
//! crate dependencies. Buckets are pruned lazily when the table grows so memory stays bounded.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Default request allowance per IP per window for public agent endpoints.
pub(crate) const DEFAULT_AGENT_RPM: u32 = 90;
pub(crate) const DEFAULT_WINDOW: Duration = Duration::from_secs(60);
const MAX_TRACKED_IPS: usize = 8_192;

#[derive(Clone, Debug)]
struct Bucket {
    window_start: Instant,
    count: u32,
}

#[derive(Clone)]
pub(crate) struct AgentRateLimiter {
    limit: u32,
    window: Duration,
    buckets: Arc<Mutex<HashMap<String, Bucket>>>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RateDecision {
    pub(crate) allowed: bool,
    pub(crate) remaining: u32,
    pub(crate) retry_after_secs: u64,
}

impl AgentRateLimiter {
    pub(crate) fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit: limit.max(1),
            window: window.max(Duration::from_secs(1)),
            buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn default_public() -> Self {
        Self::new(DEFAULT_AGENT_RPM, DEFAULT_WINDOW)
    }

    pub(crate) fn check(&self, key: &str, now: Instant) -> RateDecision {
        let mut guard = self
            .buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if guard.len() >= MAX_TRACKED_IPS {
            let window = self.window;
            guard.retain(|_, bucket| now.saturating_duration_since(bucket.window_start) < window);
            // If every tracked bucket is still hot, clear the oldest half so an IP-spray
            // flood cannot grow the map unboundedly.
            if guard.len() >= MAX_TRACKED_IPS {
                let mut entries: Vec<(String, Instant)> = guard
                    .iter()
                    .map(|(k, b)| (k.clone(), b.window_start))
                    .collect();
                entries.sort_by_key(|(_, start)| *start);
                for (k, _) in entries.into_iter().take(MAX_TRACKED_IPS / 2) {
                    guard.remove(&k);
                }
            }
        }

        let entry = guard.entry(key.to_string()).or_insert(Bucket {
            window_start: now,
            count: 0,
        });

        let elapsed = now.saturating_duration_since(entry.window_start);
        if elapsed >= self.window {
            entry.window_start = now;
            entry.count = 0;
        }

        if entry.count >= self.limit {
            let retry_after = self
                .window
                .saturating_sub(now.saturating_duration_since(entry.window_start))
                .as_secs()
                .max(1);
            RateDecision {
                allowed: false,
                remaining: 0,
                retry_after_secs: retry_after,
            }
        } else {
            entry.count += 1;
            RateDecision {
                allowed: true,
                remaining: self.limit.saturating_sub(entry.count),
                retry_after_secs: 0,
            }
        }
    }
}

/// Extract a client identifier for rate limiting. Prefers Cloudflare's connecting IP
/// header when behind CF, then the left-most `X-Forwarded-For` entry, then the peer socket.
pub(crate) fn client_key(headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
    if let Some(cf_ip) = headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return cf_ip.to_string();
    }
    if let Some(xff) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return xff.to_string();
    }
    peer.map(|addr| addr.ip())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]))
        .to_string()
}

pub(crate) async fn rate_limit_middleware(
    State(limiter): State<AgentRateLimiter>,
    req: Request,
    next: Next,
) -> Response {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0);
    let key = client_key(req.headers(), peer);
    let decision = limiter.check(&key, Instant::now());

    if !decision.allowed {
        metrics::counter!(
            "ultros_agent_requests_total",
            "surface" => "rate_limit",
            "status" => "429"
        )
        .increment(1);
        let mut resp = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "rate_limit_exceeded",
                "message": "Too many requests to Ultros public agent endpoints. Please slow down and honor Retry-After.",
                "retry_after_seconds": decision.retry_after_secs,
                "source": "Ultros (https://ultros.app)",
            })),
        )
            .into_response();
        if let Ok(val) = HeaderValue::from_str(&decision.retry_after_secs.to_string()) {
            resp.headers_mut().insert(header::RETRY_AFTER, val);
        }
        return resp;
    }

    let mut response = next.run(req).await;
    if let Ok(val) = HeaderValue::from_str(&limiter.limit.to_string()) {
        response.headers_mut().insert("x-ratelimit-limit", val);
    }
    if let Ok(val) = HeaderValue::from_str(&decision.remaining.to_string()) {
        response.headers_mut().insert("x-ratelimit-remaining", val);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_limit_then_resets_after_window() {
        let limiter = AgentRateLimiter::new(2, Duration::from_secs(60));
        let t0 = Instant::now();

        let d1 = limiter.check("1.2.3.4", t0);
        assert!(d1.allowed);
        assert_eq!(d1.remaining, 1);

        let d2 = limiter.check("1.2.3.4", t0 + Duration::from_secs(5));
        assert!(d2.allowed);
        assert_eq!(d2.remaining, 0);

        let d3 = limiter.check("1.2.3.4", t0 + Duration::from_secs(10));
        assert!(!d3.allowed);
        assert!(d3.retry_after_secs >= 49);

        // Different IP is unaffected.
        let other = limiter.check("5.6.7.8", t0 + Duration::from_secs(10));
        assert!(other.allowed);

        // Window rolls over.
        let after = limiter.check("1.2.3.4", t0 + Duration::from_secs(61));
        assert!(after.allowed);
        assert_eq!(after.remaining, 1);
    }

    #[test]
    fn prefers_cf_connecting_ip_then_xff_then_peer() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("10.0.0.1, 10.0.0.2"),
        );
        assert_eq!(client_key(&headers, None), "10.0.0.1");

        headers.insert("cf-connecting-ip", HeaderValue::from_static("203.0.113.7"));
        assert_eq!(client_key(&headers, None), "203.0.113.7");
    }
}
