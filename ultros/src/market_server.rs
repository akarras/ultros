//! Internal HTTP/2-capable market API server for `ULTROS_ROLES=ingest,market,bot`
//! (or any role combination where `market` is enabled without `web`).
//!
//! Serves the coarse market read endpoints (`cheapest`, `recentSales`,
//! `best_deals`, `trends`, `sale_stats`, `listing_stats`, `price_series`,
//! `market_pulse`, `movers`) plus the internal sitemap-sales summary and manual
//! refresh endpoints consumed by [`crate::market_client::HttpMarketClient`].

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::{
    market_client::{MarketClient, RefreshOutcome, SitemapItemSaleSummary},
    roles::Roles,
    web::{
        api::{
            cheapest_per_world, get_best_deals, get_listing_stats, get_market_pulse, get_movers,
            get_sale_stats, get_trends, recent_sales,
        },
        error::WebError,
        price_series,
    },
    web_metrics::start_metrics_server,
};

#[derive(Clone)]
struct MarketServerState {
    market: Arc<dyn MarketClient>,
    roles: Roles,
}

impl axum::extract::FromRef<MarketServerState> for Arc<dyn MarketClient> {
    fn from_ref(input: &MarketServerState) -> Self {
        input.market.clone()
    }
}

#[derive(Serialize)]
struct HealthzResponse {
    status: &'static str,
    roles: Vec<&'static str>,
}

async fn healthz(State(state): State<MarketServerState>) -> impl IntoResponse {
    let mut response = Json(HealthzResponse {
        status: "ok",
        roles: state.roles.active_names(),
    })
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn internal_sitemap_sales(
    State(market): State<Arc<dyn MarketClient>>,
) -> Result<Response, WebError> {
    let summaries: Vec<SitemapItemSaleSummary> = market.sitemap_item_sales().await?;
    let mut response = Json(summaries).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn internal_refresh_listings(
    State(market): State<Arc<dyn MarketClient>>,
    Path((world, item_id)): Path<(String, i32)>,
) -> Result<Response, WebError> {
    let mut response = match market.refresh_listings(&world, item_id).await? {
        RefreshOutcome::Refreshed => StatusCode::NO_CONTENT.into_response(),
        RefreshOutcome::MarketIsolationBlocked => {
            (StatusCode::CONFLICT, "market fixture isolation enabled").into_response()
        }
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// Constant-time comparison of two byte slices via fixed-length SHA-256 digests
/// so neither token length nor prefix leaks through response timing.
fn bearer_token_matches(provided: &str, expected: &str) -> bool {
    let a = Sha256::digest(provided.as_bytes());
    let b = Sha256::digest(expected.as_bytes());
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0 && provided.len() == expected.len()
}

async fn require_internal_bearer(
    State(expected): State<Option<Arc<str>>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected) = expected else {
        return next.run(request).await;
    };
    let provided = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match provided {
        Some(token) if bearer_token_matches(token, &expected) => next.run(request).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            [(header::CACHE_CONTROL, "no-store")],
            "Unauthorized",
        )
            .into_response(),
    }
}

/// Build the market-only HTTP/2 router (public market reads + authenticated
/// internal RPC endpoints + `/healthz`).
pub(crate) fn router(
    market: Arc<dyn MarketClient>,
    roles: Roles,
    internal_token: Option<String>,
) -> Router {
    let state = MarketServerState { market, roles };
    let expected_token: Option<Arc<str>> = internal_token
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(Arc::from);

    let internal = Router::new()
        .route(
            "/internal/market/sitemap_sales",
            get(internal_sitemap_sales),
        )
        .route(
            "/internal/market/refresh/{world}/{itemid}",
            post(internal_refresh_listings),
        )
        .layer(middleware::from_fn_with_state(
            expected_token,
            require_internal_bearer,
        ));

    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/cheapest/{world}", get(cheapest_per_world))
        .route("/api/v1/recentSales/{world}", get(recent_sales))
        .route("/api/v1/best_deals/{world}", get(get_best_deals))
        .route("/api/v1/trends/{world}", get(get_trends))
        .route("/api/v1/sale_stats/{world}", get(get_sale_stats))
        .route("/api/v1/listing_stats/{world}", get(get_listing_stats))
        .route("/api/v1/price_series/{world}/{itemid}", get(price_series))
        .route("/api/v1/market_pulse/{world}", get(get_market_pulse))
        .route("/api/v1/movers/{world}", get(get_movers))
        .merge(internal)
        .with_state(state)
}

/// Default port for a market-only container (`ULTROS_ROLES=ingest,market,bot`)
/// when `PORT` is unset.
pub(crate) const DEFAULT_MARKET_PORT: u16 = 8081;

pub(crate) async fn start_market_server(
    market: Arc<dyn MarketClient>,
    roles: Roles,
    internal_token: Option<String>,
    prometheus_handle: metrics_exporter_prometheus::PrometheusHandle,
    token: CancellationToken,
) {
    if internal_token.is_none() {
        tracing::info!(
            "ULTROS_MARKET_TOKEN unset; /internal/market/* endpoints rely on private network isolation"
        );
    }
    let app = router(market, roles, internal_token);
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(DEFAULT_MARKET_PORT);
    let addr = SocketAddr::from((std::net::Ipv4Addr::UNSPECIFIED, port));
    let metrics_token = token.clone();
    let (_market_app, _metrics_app) = futures::future::join(
        async move {
            tracing::info!(
                "ultros-market listening on {} (HTTP/1.1 + HTTP/2 h2c)",
                addr
            );
            let listener = TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    token.cancelled().await;
                })
                .await
                .unwrap();
        },
        start_metrics_server(prometheus_handle, metrics_token),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market_client::HttpMarketClient;
    use crate::roles::parse_market_url;
    use crate::web::{
        PriceSeriesQuery,
        api::{BestDealsQuery, TrendsQuery},
        error::SharedWebError,
    };
    use axum::body::Body;
    use futures::future::BoxFuture;
    use http_body_util::BodyExt;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubMarketClient {
        recorded: Mutex<Vec<String>>,
    }

    impl StubMarketClient {
        fn record(&self, entry: String) {
            self.recorded.lock().unwrap().push(entry);
        }

        fn json_response(
            body: &'static str,
            cache_control: &'static str,
            encoding: Option<&'static str>,
        ) -> Response {
            let mut b = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::CACHE_CONTROL, cache_control);
            if let Some(enc) = encoding {
                b = b
                    .header(header::CONTENT_ENCODING, enc)
                    .header(header::VARY, "accept-encoding");
            }
            b.body(Body::from(body)).unwrap()
        }
    }

    impl MarketClient for StubMarketClient {
        fn cheapest<'a>(
            &'a self,
            world: &'a str,
            format: Option<&'a str>,
            headers: &'a axum::http::HeaderMap,
        ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
            Box::pin(async move {
                let ae = headers
                    .get(header::ACCEPT_ENCODING)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                self.record(format!("cheapest:{world}:{format:?}:{ae}"));
                if world == "MissingWorld" {
                    return Err(WebError::NotFound.into());
                }
                let enc = ae.contains("br").then_some("br");
                Ok(Self::json_response(
                    r#"{"item_id":[1]}"#,
                    "public, max-age=5, stale-while-revalidate=10, stale-if-error=600",
                    enc,
                ))
            })
        }

        fn recent_sales<'a>(
            &'a self,
            world: &'a str,
            format: Option<&'a str>,
            _headers: &'a axum::http::HeaderMap,
        ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
            Box::pin(async move {
                self.record(format!("recentSales:{world}:{format:?}"));
                Ok(Self::json_response(
                    r#"{"sales":[]}"#,
                    "public, max-age=30, stale-while-revalidate=120, stale-if-error=600",
                    None,
                ))
            })
        }

        fn best_deals<'a>(
            &'a self,
            world: &'a str,
            query: &'a BestDealsQuery,
        ) -> BoxFuture<'a, Result<Response, WebError>> {
            Box::pin(async move {
                self.record(format!(
                    "best_deals:{world}:{:?}:{:?}",
                    query.min_profit, query.show_suspicious
                ));
                // Simulate warming-up degraded 200 with no-store.
                Ok(Self::json_response("[]", "no-store", None))
            })
        }

        fn trends<'a>(
            &'a self,
            world: &'a str,
            query: &'a TrendsQuery,
        ) -> BoxFuture<'a, Result<Response, WebError>> {
            Box::pin(async move {
                self.record(format!("trends:{world}:{:?}", query.window));
                if world == "North-America" {
                    return Err(WebError::BadRequest);
                }
                Ok(Self::json_response(
                    r#"{"items":[]}"#,
                    "public, max-age=60, stale-while-revalidate=300, stale-if-error=600",
                    None,
                ))
            })
        }

        fn sale_stats<'a>(
            &'a self,
            world: &'a str,
            window: Option<u16>,
            format: Option<&'a str>,
            _headers: &'a axum::http::HeaderMap,
        ) -> BoxFuture<'a, Result<Response, WebError>> {
            Box::pin(async move {
                self.record(format!("sale_stats:{world}:{window:?}:{format:?}"));
                Ok(Self::json_response(
                    r#"{"stats":[]}"#,
                    "public, max-age=300, stale-while-revalidate=600, stale-if-error=600",
                    None,
                ))
            })
        }

        fn listing_stats<'a>(
            &'a self,
            world: &'a str,
            window: Option<u16>,
            format: Option<&'a str>,
            _headers: &'a axum::http::HeaderMap,
        ) -> BoxFuture<'a, Result<Response, WebError>> {
            Box::pin(async move {
                self.record(format!("listing_stats:{world}:{window:?}:{format:?}"));
                Ok(Self::json_response(
                    r#"{"stats":[]}"#,
                    "public, max-age=300, stale-while-revalidate=600, stale-if-error=600",
                    None,
                ))
            })
        }

        fn price_series<'a>(
            &'a self,
            world: &'a str,
            item_id: i32,
            query: &'a PriceSeriesQuery,
        ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
            Box::pin(async move {
                self.record(format!(
                    "price_series:{world}:{item_id}:{:?}:{:?}",
                    query.group, query.hq
                ));
                Ok(Self::json_response(
                    r#"{"series":[]}"#,
                    "public, max-age=60, stale-while-revalidate=300, stale-if-error=600",
                    None,
                ))
            })
        }

        fn market_pulse<'a>(
            &'a self,
            world: &'a str,
            _headers: &'a axum::http::HeaderMap,
        ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
            Box::pin(async move {
                self.record(format!("market_pulse:{world}"));
                Ok(Self::json_response(
                    r#"{"world_id":54,"sales_today":10}"#,
                    "no-store",
                    None,
                ))
            })
        }

        fn movers<'a>(
            &'a self,
            world: &'a str,
            direction: Option<&'a str>,
            limit: Option<u32>,
            _headers: &'a axum::http::HeaderMap,
        ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
            Box::pin(async move {
                self.record(format!("movers:{world}:{direction:?}:{limit:?}"));
                Ok(Self::json_response(
                    r#"{"items":[]}"#,
                    "public, max-age=60, stale-while-revalidate=240, stale-if-error=600",
                    None,
                ))
            })
        }

        fn sitemap_item_sales<'a>(
            &'a self,
        ) -> BoxFuture<'a, Result<Vec<SitemapItemSaleSummary>, WebError>> {
            Box::pin(async move {
                self.record("sitemap_sales".into());
                Ok(vec![SitemapItemSaleSummary {
                    item_id: 42,
                    first_sale_unix: Some(1_700_000_000),
                    median_sale_unix: Some(1_700_000_500),
                }])
            })
        }

        fn refresh_listings<'a>(
            &'a self,
            world: &'a str,
            item_id: i32,
        ) -> BoxFuture<'a, Result<RefreshOutcome, WebError>> {
            Box::pin(async move {
                self.record(format!("refresh:{world}:{item_id}"));
                if item_id == 999 {
                    return Ok(RefreshOutcome::MarketIsolationBlocked);
                }
                Ok(RefreshOutcome::Refreshed)
            })
        }
    }

    async fn spawn_test_market_server(
        stub: Arc<StubMarketClient>,
        token: Option<String>,
    ) -> (url::Url, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let roles = Roles::parse(Some("ingest,market,bot")).unwrap();
        let app = router(stub, roles, token);
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let url = parse_market_url(&format!("http://{addr}")).unwrap();
        (url, handle)
    }

    #[tokio::test]
    async fn http2_market_client_round_trips_all_endpoints_and_preserves_headers() {
        let stub = Arc::new(StubMarketClient::default());
        let (url, server) =
            spawn_test_market_server(stub.clone(), Some("secret-token".into())).await;
        let client = HttpMarketClient::new(url.clone(), Some("secret-token")).unwrap();

        // 1. Pre-compressed cheapest passthrough + Accept-Encoding forwarding.
        let mut req_headers = axum::http::HeaderMap::new();
        req_headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("br, gzip"),
        );
        req_headers.insert(
            header::COOKIE,
            HeaderValue::from_static("discord_auth=secret"),
        );
        let resp = client
            .cheapest("North-America", Some("columnar"), &req_headers)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get(header::CONTENT_ENCODING).unwrap(), "br");
        assert_eq!(resp.headers().get(header::VARY).unwrap(), "accept-encoding");
        assert_eq!(
            resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=5, stale-while-revalidate=10, stale-if-error=600"
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], br#"{"item_id":[1]}"#);

        // 2. Degraded 200 preserves Cache-Control: no-store verbatim.
        let deals = client
            .best_deals(
                "Sargatanas",
                &BestDealsQuery {
                    min_profit: Some(5000),
                    filter_sale: Some("Week".into()),
                    show_suspicious: Some(false),
                    limit: Some(25),
                    min_velocity: None,
                    min_buffer_sales: None,
                    max_roi: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(deals.status(), StatusCode::OK);
        assert_eq!(
            deals.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );

        // 3. 404 & 400 error mapping.
        let empty = axum::http::HeaderMap::new();
        assert!(matches!(
            client.cheapest("MissingWorld", None, &empty).await,
            Err(e) if matches!(e.0.as_ref(), WebError::NotFound)
        ));
        assert!(matches!(
            client
                .trends("North-America", &TrendsQuery::default())
                .await,
            Err(WebError::BadRequest)
        ));

        // 4. All remaining coarse market read endpoints.
        assert_eq!(
            client
                .recent_sales("Sargatanas", Some("columnar"), &empty)
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            client
                .sale_stats("Sargatanas", Some(7), Some("columnar"), &empty)
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            client
                .listing_stats("Sargatanas", Some(30), Some("columnar"), &empty)
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            client
                .price_series(
                    "Sargatanas",
                    5333,
                    &PriceSeriesQuery {
                        from: Some(100),
                        to: Some(200),
                        bucket: Some(3600),
                        group: Some("world".into()),
                        hq: Some("hq".into()),
                    },
                )
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let pulse = client.market_pulse("Sargatanas", &empty).await.unwrap();
        assert_eq!(
            pulse.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert_eq!(
            client
                .movers("Sargatanas", Some("rising"), Some(10), &empty)
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );

        // 5. Internal endpoints with valid bearer token.
        let summaries = client.sitemap_item_sales().await.unwrap();
        assert_eq!(
            summaries,
            vec![SitemapItemSaleSummary {
                item_id: 42,
                first_sale_unix: Some(1_700_000_000),
                median_sale_unix: Some(1_700_000_500),
            }]
        );
        assert_eq!(
            client.refresh_listings("Sargatanas", 5333).await.unwrap(),
            RefreshOutcome::Refreshed
        );
        assert_eq!(
            client.refresh_listings("Sargatanas", 999).await.unwrap(),
            RefreshOutcome::MarketIsolationBlocked
        );

        // 6. Unauthorized internal calls are rejected when ULTROS_MARKET_TOKEN is set.
        let unauthed = HttpMarketClient::new(url.clone(), None).unwrap();
        assert!(matches!(
            unauthed.sitemap_item_sales().await,
            Err(WebError::TemporarilyUnavailable)
        ));
        let wrong_token = HttpMarketClient::new(url.clone(), Some("wrong-token")).unwrap();
        assert!(matches!(
            wrong_token.refresh_listings("Sargatanas", 5333).await,
            Err(WebError::TemporarilyUnavailable)
        ));

        // 7. Path-traversal world input is percent-encoded as a single segment
        //    and cannot escape to /internal/market/*.
        let _ = client
            .cheapest("../internal/market/sitemap_sales", None, &empty)
            .await;
        let calls = stub.recorded.lock().unwrap().clone();
        assert_eq!(
            calls
                .iter()
                .filter(|c| c.as_str() == "sitemap_sales")
                .count(),
            1,
            "traversal world segment must not hit /internal/market/sitemap_sales"
        );

        // 8. /healthz works over HTTP/1.1 too (for Docker healthchecks).
        let health = reqwest::Client::new()
            .get(format!("{url}healthz"))
            .send()
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        assert_eq!(
            health.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        let health_json: serde_json::Value =
            serde_json::from_slice(&health.bytes().await.unwrap()).unwrap();
        assert_eq!(health_json["status"], "ok");
        assert_eq!(
            health_json["roles"],
            serde_json::json!(["market", "ingest", "bot"])
        );

        server.abort();
    }
}
