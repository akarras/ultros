//! Trusted-proxy-aware per-IP rate limiter for public AI agent surfaces
//! (`/mcp` and `Accept: text/markdown` / `?format=md` routes).
//!
//! Security & memory invariants:
//! - Defaults to the direct TCP socket peer (`ConnectInfo<SocketAddr>`).
//! - Honors `CF-Connecting-IP` or `X-Forwarded-For` **only** when the peer IP falls
//!   inside an explicitly configured trusted proxy CIDR (`ULTROS_TRUSTED_PROXIES` or
//!   `ULTROS_PLUGIN_TRUSTED_PROXIES`), preventing header spoofing on direct connections.
//! - Keys buckets by a fixed-size [`ClientBucketKey`] (IPv4 address or IPv6 `/64` prefix),
//!   so untrusted header strings can never consume memory and an IPv6 `/64` cannot mint
//!   unlimited buckets.
//! - When full after pruning expired windows, fails closed for unseen clients rather than
//!   evicting active buckets and resetting legitimate clients' quotas.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Default request allowance per IP (or IPv6 /64) per window for public agent surfaces.
pub(crate) const DEFAULT_AGENT_RPM: u32 = 90;
pub(crate) const DEFAULT_WINDOW: Duration = Duration::from_secs(60);
const MAX_TRACKED_KEYS: usize = 8_192;
const MAX_FORWARDED_HEADER_LEN: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ClientBucketKey {
    V4(Ipv4Addr),
    /// First 64 bits of an IPv6 address (four 16-bit segments).
    V6Prefix([u16; 4]),
}

impl From<IpAddr> for ClientBucketKey {
    fn from(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(v4) => Self::V4(v4),
            IpAddr::V6(v6) => {
                // Map IPv4-mapped IPv6 addresses (`::ffff:a.b.c.d`) onto the IPv4 keyspace.
                if let Some(v4) = v6.to_ipv4_mapped() {
                    Self::V4(v4)
                } else {
                    let s = v6.segments();
                    Self::V6Prefix([s[0], s[1], s[2], s[3]])
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Bucket {
    window_start: Instant,
    count: u32,
}

#[derive(Clone)]
pub(crate) struct AgentRateLimiter {
    limit: u32,
    window: Duration,
    max_keys: usize,
    trusted_proxies: Arc<Vec<ipnet::IpNet>>,
    buckets: Arc<Mutex<HashMap<ClientBucketKey, Bucket>>>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RateDecision {
    pub(crate) allowed: bool,
    pub(crate) remaining: u32,
    pub(crate) retry_after_secs: u64,
}

fn parse_trusted_proxy_list(raw: &str) -> Vec<ipnet::IpNet> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            entry
                .parse::<IpAddr>()
                .map(ipnet::IpNet::from)
                .or_else(|_| entry.parse::<ipnet::IpNet>())
                .ok()
        })
        .collect()
}

fn trusted_proxies_from_env() -> Vec<ipnet::IpNet> {
    let raw = std::env::var("ULTROS_TRUSTED_PROXIES")
        .or_else(|_| std::env::var("ULTROS_PLUGIN_TRUSTED_PROXIES"))
        .unwrap_or_default();
    parse_trusted_proxy_list(&raw)
}

impl AgentRateLimiter {
    pub(crate) fn new(limit: u32, window: Duration, trusted_proxies: Vec<ipnet::IpNet>) -> Self {
        Self::with_capacity(limit, window, MAX_TRACKED_KEYS, trusted_proxies)
    }

    pub(crate) fn with_capacity(
        limit: u32,
        window: Duration,
        max_keys: usize,
        trusted_proxies: Vec<ipnet::IpNet>,
    ) -> Self {
        Self {
            limit: limit.max(1),
            window: window.max(Duration::from_secs(1)),
            max_keys: max_keys.max(1),
            trusted_proxies: Arc::new(trusted_proxies),
            buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn default_public() -> Self {
        Self::new(
            DEFAULT_AGENT_RPM,
            DEFAULT_WINDOW,
            trusted_proxies_from_env(),
        )
    }

    pub(crate) fn limit(&self) -> u32 {
        self.limit
    }

    pub(crate) fn check_request(
        &self,
        headers: &HeaderMap,
        peer: Option<SocketAddr>,
    ) -> RateDecision {
        let ip = resolve_client_ip(headers, peer.map(|s| s.ip()), &self.trusted_proxies);
        self.check(ClientBucketKey::from(ip), Instant::now())
    }

    pub(crate) fn check(&self, key: ClientBucketKey, now: Instant) -> RateDecision {
        let mut guard = self
            .buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if !guard.contains_key(&key) && guard.len() >= self.max_keys {
            let window = self.window;
            guard.retain(|_, bucket| now.saturating_duration_since(bucket.window_start) < window);
            // Fail closed for unseen IPs rather than evicting active legitimate quotas.
            if guard.len() >= self.max_keys {
                let soonest_expiry = guard
                    .values()
                    .map(|b| {
                        self.window
                            .saturating_sub(now.saturating_duration_since(b.window_start))
                            .as_secs()
                    })
                    .min()
                    .unwrap_or(self.window.as_secs())
                    .max(1);
                return RateDecision {
                    allowed: false,
                    remaining: 0,
                    retry_after_secs: soonest_expiry,
                };
            }
        }

        let entry = guard.entry(key).or_insert(Bucket {
            window_start: now,
            count: 0,
        });

        if now.saturating_duration_since(entry.window_start) >= self.window {
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

/// Resolve the canonical client [`IpAddr`].
///
/// Only trusts `CF-Connecting-IP` or `X-Forwarded-For` (walking right-to-left past
/// trusted proxies, matching `ultros/src/web/plugin.rs::forwarded_client`) when the
/// direct TCP peer is inside `trusted_proxies`. Otherwise returns the socket peer IP.
pub(crate) fn resolve_client_ip(
    headers: &HeaderMap,
    peer: Option<IpAddr>,
    trusted_proxies: &[ipnet::IpNet],
) -> IpAddr {
    let fallback = peer.unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let is_trusted = |ip: IpAddr| trusted_proxies.iter().any(|net| net.contains(&ip));

    if !peer.is_some_and(is_trusted) {
        return fallback;
    }

    // When behind Cloudflare (and the direct peer is a trusted proxy), `CF-Connecting-IP`
    // carries a single canonical client address set by Cloudflare's edge.
    let mut cf_headers = headers.get_all("cf-connecting-ip").iter();
    if let Some(first) = cf_headers.next()
        && cf_headers.next().is_none()
        && let Ok(raw) = first.to_str()
        && raw.len() <= 64
        && let Ok(ip) = raw.trim().parse::<IpAddr>()
    {
        return ip;
    }

    // Otherwise walk X-Forwarded-For right-to-left, stripping trusted proxy hops.
    let mut xff_headers = headers.get_all("x-forwarded-for").iter();
    let Some(first_xff) = xff_headers
        .next()
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.len() <= MAX_FORWARDED_HEADER_LEN)
    else {
        return fallback;
    };
    if xff_headers.next().is_some() {
        return fallback;
    }
    let Ok(mut chain) = first_xff
        .split(',')
        .map(|part| part.trim().parse::<IpAddr>())
        .collect::<Result<Vec<_>, _>>()
    else {
        return fallback;
    };

    let mut current = peer;
    while current.is_some_and(is_trusted) {
        let Some(next) = chain.pop() else {
            break;
        };
        current = Some(next);
    }
    current.unwrap_or(fallback)
}

pub(crate) fn apply_rate_headers(resp: &mut Response, limit: u32, decision: &RateDecision) {
    if let Ok(val) = HeaderValue::from_str(&limit.to_string()) {
        resp.headers_mut().insert("x-ratelimit-limit", val);
    }
    if let Ok(val) = HeaderValue::from_str(&decision.remaining.to_string()) {
        resp.headers_mut().insert("x-ratelimit-remaining", val);
    }
}

pub(crate) fn too_many_requests_response(
    limit: u32,
    decision: &RateDecision,
    as_markdown: bool,
) -> Response {
    metrics::counter!(
        "ultros_agent_requests_total",
        "surface" => "rate_limit",
        "status" => "429"
    )
    .increment(1);
    let mut resp = if as_markdown {
        let body = format!(
            "# 429 Too Many Requests\n\nRate limit exceeded ({limit} requests/min per IP). Retry after {} seconds.\n\nSource: Ultros (https://ultros.app)\n",
            decision.retry_after_secs
        );
        let mut r = (StatusCode::TOO_MANY_REQUESTS, body).into_response();
        r.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/markdown; charset=utf-8"),
        );
        r
    } else {
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "rate_limit_exceeded",
                "message": "Too many requests to Ultros public agent endpoints. Please slow down and honor Retry-After.",
                "retry_after_seconds": decision.retry_after_secs,
                "source": "Ultros (https://ultros.app)",
            })),
        )
            .into_response()
    };
    if let Ok(val) = HeaderValue::from_str(&decision.retry_after_secs.to_string()) {
        resp.headers_mut().insert(header::RETRY_AFTER, val);
    }
    apply_rate_headers(&mut resp, limit, decision);
    resp
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
    let decision = limiter.check_request(req.headers(), peer);
    if !decision.allowed {
        return too_many_requests_response(limiter.limit(), &decision, false);
    }

    let mut response = next.run(req).await;
    apply_rate_headers(&mut response, limiter.limit(), &decision);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::routing::get;
    use std::net::Ipv6Addr;
    use tower::ServiceExt;

    #[test]
    fn allows_up_to_limit_then_resets_after_window() {
        let limiter = AgentRateLimiter::new(2, Duration::from_secs(60), vec![]);
        let t0 = Instant::now();
        let k1 = ClientBucketKey::from(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)));
        let k2 = ClientBucketKey::from(IpAddr::V4(Ipv4Addr::new(5, 6, 7, 8)));

        let d1 = limiter.check(k1, t0);
        assert!(d1.allowed);
        assert_eq!(d1.remaining, 1);

        let d2 = limiter.check(k1, t0 + Duration::from_secs(5));
        assert!(d2.allowed);
        assert_eq!(d2.remaining, 0);

        let d3 = limiter.check(k1, t0 + Duration::from_secs(10));
        assert!(!d3.allowed);
        assert!(d3.retry_after_secs >= 49);

        // Different IP is unaffected.
        assert!(limiter.check(k2, t0 + Duration::from_secs(10)).allowed);

        // Window rolls over.
        let after = limiter.check(k1, t0 + Duration::from_secs(61));
        assert!(after.allowed);
        assert_eq!(after.remaining, 1);
    }

    #[test]
    fn ignores_spoofed_headers_from_untrusted_peers() {
        let mut headers = HeaderMap::new();
        headers.insert("cf-connecting-ip", HeaderValue::from_static("203.0.113.7"));
        headers.insert("x-forwarded-for", HeaderValue::from_static("198.51.100.9"));
        let peer = Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)));

        // Without trusted proxies configured, spoofed headers are ignored completely.
        assert_eq!(
            resolve_client_ip(&headers, peer, &[]),
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10))
        );
    }

    #[test]
    fn honors_cf_and_xff_chains_only_from_trusted_proxies() {
        let proxies = parse_trusted_proxy_list("10.0.0.0/8, 172.16.0.1");
        let proxy_peer = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)));

        // Right-to-left XFF walk strips trusted proxy hops and ignores client-prepended junk.
        let mut xff_headers = HeaderMap::new();
        xff_headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("1.1.1.1, 198.51.100.42, 172.16.0.1"),
        );
        assert_eq!(
            resolve_client_ip(&xff_headers, proxy_peer, &proxies),
            IpAddr::V4(Ipv4Addr::new(198, 51, 100, 42))
        );

        // CF-Connecting-IP takes precedence when present and single-valued from a trusted proxy.
        xff_headers.insert("cf-connecting-ip", HeaderValue::from_static("203.0.113.7"));
        assert_eq!(
            resolve_client_ip(&xff_headers, proxy_peer, &proxies),
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))
        );
    }

    #[test]
    fn ipv6_addresses_in_same_slash_64_share_one_bucket() {
        let a: IpAddr = "2001:db8:abcd:0012::1".parse::<Ipv6Addr>().unwrap().into();
        let b: IpAddr = "2001:db8:abcd:0012:ffff::99"
            .parse::<Ipv6Addr>()
            .unwrap()
            .into();
        let c: IpAddr = "2001:db8:abcd:0013::1".parse::<Ipv6Addr>().unwrap().into();
        assert_eq!(ClientBucketKey::from(a), ClientBucketKey::from(b));
        assert_ne!(ClientBucketKey::from(a), ClientBucketKey::from(c));
    }

    #[test]
    fn overflow_fails_closed_without_evicting_active_buckets() {
        let limiter = AgentRateLimiter::with_capacity(1, Duration::from_secs(60), 2, vec![]);
        let t0 = Instant::now();
        let k1 = ClientBucketKey::from(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)));
        let k2 = ClientBucketKey::from(IpAddr::V4(Ipv4Addr::new(2, 2, 2, 2)));
        let k3 = ClientBucketKey::from(IpAddr::V4(Ipv4Addr::new(3, 3, 3, 3)));

        assert!(limiter.check(k1, t0).allowed);
        assert!(limiter.check(k2, t0).allowed);
        // Table is full with active windows: unseen k3 is rejected, and k1 stays exhausted.
        assert!(!limiter.check(k3, t0 + Duration::from_secs(1)).allowed);
        assert!(!limiter.check(k1, t0 + Duration::from_secs(2)).allowed);
    }

    #[tokio::test]
    async fn middleware_returns_429_with_retry_after_when_exhausted() {
        let limiter = AgentRateLimiter::new(1, Duration::from_secs(60), vec![]);
        let app = Router::new()
            .route("/ok", get(|| async { "ok" }))
            .route_layer(axum::middleware::from_fn_with_state(
                limiter,
                rate_limit_middleware,
            ));

        let r1 = app
            .clone()
            .oneshot(Request::builder().uri("/ok").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::OK);
        assert_eq!(r1.headers().get("x-ratelimit-remaining").unwrap(), "0");

        let r2 = app
            .oneshot(Request::builder().uri("/ok").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(r2.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(r2.headers().contains_key(header::RETRY_AFTER));
    }
}
