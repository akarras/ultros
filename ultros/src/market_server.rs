//! Internal HTTP/2-capable market API server for `ULTROS_ROLES=ingest,market,bot`
//! (or any role combination where `market` is enabled without `web`).
//!
//! Serves the coarse market read endpoints (`cheapest`, `recentSales`,
//! `best_deals`, `trends`, `sale_stats`, `listing_stats`, `price_series`,
//! `market_pulse`, `movers`), the internal sitemap-sales summary and manual
//! refresh endpoints, and the live market event NDJSON stream
//! (`/internal/market/events`) consumed by
//! [`crate::market_client::HttpMarketClient`].

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio::sync::broadcast::error::RecvError;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

use crate::{
    bot_admin_client::{
        BotAdminClient, BotAdminError, BotDeliveryError, DeliverDiscordNotificationRequest,
        GuildDiscoveryRequest, RequireManageableGuildRequest, ResolveChannelRequest,
        SearchGuildMembersQuery, TriggerGroupSyncRequest, TriggerGroupSyncResponse,
    },
    event::{EventReceivers, EventType},
    market_client::{MarketClient, MarketStreamEvent, RefreshOutcome, SitemapItemSaleSummary},
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
    bot_admin: Arc<dyn BotAdminClient>,
    event_receivers: EventReceivers,
    roles: Roles,
    shutdown: CancellationToken,
    active_streams: Arc<AtomicUsize>,
}

impl axum::extract::FromRef<MarketServerState> for Arc<dyn MarketClient> {
    fn from_ref(input: &MarketServerState) -> Self {
        input.market.clone()
    }
}

impl axum::extract::FromRef<MarketServerState> for Arc<dyn BotAdminClient> {
    fn from_ref(input: &MarketServerState) -> Self {
        input.bot_admin.clone()
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

const STREAM_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
const STREAM_CHANNEL_CAPACITY: usize = 512;

fn encode_stream_frame(event: &MarketStreamEvent) -> Option<Bytes> {
    let mut buf = serde_json::to_vec(event).ok()?;
    buf.push(b'\n');
    Some(Bytes::from(buf))
}

struct StreamSubscriberGuard(Arc<AtomicUsize>);

impl StreamSubscriberGuard {
    fn enter(counter: Arc<AtomicUsize>) -> Self {
        let active = counter.fetch_add(1, Ordering::AcqRel) + 1;
        metrics::gauge!("ultros_market_event_stream_subscribers").set(active as f64);
        Self(counter)
    }
}

impl Drop for StreamSubscriberGuard {
    fn drop(&mut self) {
        let active = self.0.fetch_sub(1, Ordering::AcqRel).saturating_sub(1);
        metrics::gauge!("ultros_market_event_stream_subscribers").set(active as f64);
    }
}

async fn send_stream_frame(
    tx: &tokio::sync::mpsc::Sender<Result<Bytes, Infallible>>,
    shutdown: &CancellationToken,
    event: &MarketStreamEvent,
) -> bool {
    let Some(frame) = encode_stream_frame(event) else {
        return true;
    };
    tokio::select! {
        _ = shutdown.cancelled() => false,
        res = tx.send(Ok(frame)) => res.is_ok(),
    }
}

/// Streams live listing and sale events as NDJSON frames to connected
/// `ultros-web` replicas so browser WebSockets (`/api/v1/realtime/events` and
/// `/alerts/websocket`) receive real-time market traffic in split mode.
async fn internal_market_events(State(state): State<MarketServerState>) -> Response {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(STREAM_CHANNEL_CAPACITY);
    let mut listings = state.event_receivers.listings.resubscribe();
    let mut history = state.event_receivers.history.resubscribe();
    let shutdown = state.shutdown.clone();
    let active_streams = state.active_streams.clone();

    tokio::spawn(async move {
        let _guard = StreamSubscriberGuard::enter(active_streams);
        // Send an immediate keepalive so response headers flush right away.
        if !send_stream_frame(&tx, &shutdown, &MarketStreamEvent::Keepalive).await {
            return;
        }
        let mut keepalive = tokio::time::interval(STREAM_KEEPALIVE_INTERVAL);
        keepalive.tick().await;
        let mut listings_open = true;
        let mut history_open = true;

        loop {
            if !listings_open && !history_open {
                return;
            }
            let maybe_event = tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tx.closed() => return,
                _ = keepalive.tick() => Some(MarketStreamEvent::Keepalive),
                res = listings.recv(), if listings_open => match res {
                    Ok(EventType::Add(data)) => Some(MarketStreamEvent::ListingsAdd {
                        data: data.as_ref().clone(),
                    }),
                    Ok(EventType::Remove(data)) => Some(MarketStreamEvent::ListingsRemove {
                        data: data.as_ref().clone(),
                    }),
                    Ok(EventType::Update(data)) => Some(MarketStreamEvent::ListingsAdd {
                        data: data.as_ref().clone(),
                    }),
                    Err(RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "internal market event stream lagged on listings");
                        Some(MarketStreamEvent::Lagged)
                    }
                    Err(RecvError::Closed) => {
                        listings_open = false;
                        None
                    }
                },
                res = history.recv(), if history_open => match res {
                    Ok(EventType::Add(data)) => Some(MarketStreamEvent::SalesAdd {
                        data: data.as_ref().clone(),
                    }),
                    Ok(EventType::Remove(data)) => Some(MarketStreamEvent::SalesRemove {
                        data: data.as_ref().clone(),
                    }),
                    Ok(EventType::Update(data)) => Some(MarketStreamEvent::SalesAdd {
                        data: data.as_ref().clone(),
                    }),
                    Err(RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "internal market event stream lagged on sales");
                        Some(MarketStreamEvent::Lagged)
                    }
                    Err(RecvError::Closed) => {
                        history_open = false;
                        None
                    }
                },
            };

            if let Some(event) = maybe_event
                && !send_stream_frame(&tx, &shutdown, &event).await
            {
                return;
            }
        }
    });

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(ReceiverStream::new(rx)))
        .expect("valid NDJSON stream response")
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

/// Fail-closed bearer middleware for `/internal/bot/*`: unlike read-only
/// `/internal/market/*`, bot-admin routes MUST NEVER accept unauthenticated
/// requests when `ULTROS_MARKET_TOKEN` is unset or blank.
async fn require_bot_admin_bearer(
    State(expected): State<Option<Arc<str>>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected) = expected else {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::CACHE_CONTROL, "no-store")],
            "Unauthorized",
        )
            .into_response();
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

/// Ensures every `/internal/bot/*` response — including Axum extractor
/// rejections (`400`/`422`), `DefaultBodyLimit` (`413`), and auth rejections
/// (`401`) — carries `Cache-Control: no-store`.
async fn set_no_store_header(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn no_store_json<T: Serialize>(value: T) -> Response {
    let mut response = Json(value).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn internal_bot_guilds(
    State(bot_admin): State<Arc<dyn BotAdminClient>>,
) -> Result<Response, BotAdminError> {
    let ids = bot_admin
        .bot_guild_ids()
        .await
        .map_err(BotAdminError::from_api_error)?;
    Ok(no_store_json(ids))
}

async fn internal_bot_manageable_guilds(
    State(bot_admin): State<Arc<dyn BotAdminClient>>,
    Json(req): Json<GuildDiscoveryRequest>,
) -> Result<Response, BotAdminError> {
    let guilds = bot_admin
        .manageable_guilds(req.user_id, req.candidate_guild_ids)
        .await
        .map_err(BotAdminError::from_api_error)?;
    Ok(no_store_json(guilds))
}

async fn internal_bot_writable_guilds(
    State(bot_admin): State<Arc<dyn BotAdminClient>>,
    Json(req): Json<GuildDiscoveryRequest>,
) -> Result<Response, BotAdminError> {
    let guilds = bot_admin
        .writable_guilds(req.user_id, req.candidate_guild_ids)
        .await
        .map_err(BotAdminError::from_api_error)?;
    Ok(no_store_json(guilds))
}

async fn internal_bot_require_manageable_guild(
    State(bot_admin): State<Arc<dyn BotAdminClient>>,
    Json(req): Json<RequireManageableGuildRequest>,
) -> Result<Response, BotAdminError> {
    let guild = bot_admin
        .require_manageable_guild(req.guild_id, req.user_id)
        .await
        .map_err(BotAdminError::from_api_error)?;
    Ok(no_store_json(guild))
}

async fn internal_bot_resolve_channel(
    State(bot_admin): State<Arc<dyn BotAdminClient>>,
    Json(req): Json<ResolveChannelRequest>,
) -> Result<Response, BotAdminError> {
    let resolved = bot_admin
        .resolve_and_verify_channel(req.channel_id, req.user_id)
        .await
        .map_err(BotAdminError::from_api_error)?;
    Ok(no_store_json(resolved))
}

async fn internal_bot_guild_roles(
    State(bot_admin): State<Arc<dyn BotAdminClient>>,
    Path(guild_id): Path<i64>,
) -> Result<Response, BotAdminError> {
    let roles = bot_admin
        .importable_guild_roles(guild_id)
        .await
        .map_err(BotAdminError::from_api_error)?;
    Ok(no_store_json(roles))
}

async fn internal_bot_search_members(
    State(bot_admin): State<Arc<dyn BotAdminClient>>,
    Path(guild_id): Path<i64>,
    Query(query): Query<SearchGuildMembersQuery>,
) -> Result<Response, BotAdminError> {
    let members = bot_admin
        .search_guild_members(guild_id, &query.q, query.limit)
        .await
        .map_err(BotAdminError::from_api_error)?;
    Ok(no_store_json(members))
}

async fn internal_bot_trigger_group_sync(
    State(bot_admin): State<Arc<dyn BotAdminClient>>,
    Path(group_id): Path<i32>,
    Json(req): Json<TriggerGroupSyncRequest>,
) -> Result<Response, BotAdminError> {
    let status = bot_admin
        .trigger_group_sync(group_id, req.owner_id, req.trigger)
        .await
        .map_err(BotAdminError::from_api_error)?;
    Ok(no_store_json(TriggerGroupSyncResponse { status }))
}

async fn internal_bot_deliver_notification(
    State(bot_admin): State<Arc<dyn BotAdminClient>>,
    Json(req): Json<DeliverDiscordNotificationRequest>,
) -> Result<Response, BotAdminError> {
    bot_admin
        .deliver_discord_notification(req.endpoint_id, req.owner_id, &req.title, &req.body)
        .await
        .map_err(|err| match err {
            BotDeliveryError::BotOffline => BotAdminError::BotOffline,
            BotDeliveryError::Failed(message) => BotAdminError::Internal { message },
        })?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// Build the market-only HTTP/2 router (public market reads + authenticated
/// internal market/bot RPC and stream endpoints + `/healthz`).
pub(crate) fn router(
    market: Arc<dyn MarketClient>,
    bot_admin: Arc<dyn BotAdminClient>,
    event_receivers: EventReceivers,
    roles: Roles,
    internal_token: Option<String>,
    shutdown: CancellationToken,
) -> Router {
    router_with_stream_counter(
        market,
        bot_admin,
        event_receivers,
        roles,
        internal_token,
        shutdown,
        Arc::new(AtomicUsize::new(0)),
    )
}

const INTERNAL_BOT_MAX_BODY_BYTES: usize = 64 * 1024;

fn router_with_stream_counter(
    market: Arc<dyn MarketClient>,
    bot_admin: Arc<dyn BotAdminClient>,
    event_receivers: EventReceivers,
    roles: Roles,
    internal_token: Option<String>,
    shutdown: CancellationToken,
    active_streams: Arc<AtomicUsize>,
) -> Router {
    let state = MarketServerState {
        market,
        bot_admin,
        event_receivers,
        roles,
        shutdown,
        active_streams,
    };
    let expected_token: Option<Arc<str>> = internal_token
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(Arc::from);

    let internal_bot = Router::new()
        .route("/internal/bot/guilds", get(internal_bot_guilds))
        .route(
            "/internal/bot/manageable-guilds",
            post(internal_bot_manageable_guilds),
        )
        .route(
            "/internal/bot/writable-guilds",
            post(internal_bot_writable_guilds),
        )
        .route(
            "/internal/bot/require-manageable-guild",
            post(internal_bot_require_manageable_guild),
        )
        .route(
            "/internal/bot/resolve-channel",
            post(internal_bot_resolve_channel),
        )
        .route(
            "/internal/bot/guilds/{guild_id}/roles",
            get(internal_bot_guild_roles),
        )
        .route(
            "/internal/bot/guilds/{guild_id}/members/search",
            get(internal_bot_search_members),
        )
        .route(
            "/internal/bot/groups/{group_id}/sync",
            post(internal_bot_trigger_group_sync),
        )
        .route(
            "/internal/bot/deliver-notification",
            post(internal_bot_deliver_notification),
        )
        .layer(DefaultBodyLimit::max(INTERNAL_BOT_MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            expected_token.clone(),
            require_bot_admin_bearer,
        ))
        .layer(middleware::from_fn(set_no_store_header));

    let internal_market = Router::new()
        .route(
            "/internal/market/sitemap_sales",
            get(internal_sitemap_sales),
        )
        .route("/internal/market/events", get(internal_market_events))
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
        .merge(internal_market)
        .merge(internal_bot)
        .with_state(state)
}

/// Default port for a market-only container (`ULTROS_ROLES=ingest,market,bot`)
/// when `PORT` is unset.
pub(crate) const DEFAULT_MARKET_PORT: u16 = 8081;

pub(crate) async fn start_market_server(
    market: Arc<dyn MarketClient>,
    bot_admin: Arc<dyn BotAdminClient>,
    event_receivers: EventReceivers,
    roles: Roles,
    internal_token: Option<String>,
    prometheus_handle: metrics_exporter_prometheus::PrometheusHandle,
    token: CancellationToken,
) {
    if internal_token.is_none() {
        tracing::info!(
            "ULTROS_MARKET_TOKEN unset; /internal/market/* relies on private network isolation and /internal/bot/* fails closed (401)"
        );
    }
    let app = router(
        market,
        bot_admin,
        event_receivers,
        roles,
        internal_token,
        token.clone(),
    );
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
    use crate::bot_admin_client::{
        BotGuildMember, BotGuildRole, BotManageableGuild, BotResolvedChannel, GroupSyncTrigger,
        HttpBotAdminClient, InProcessBotAdminClient,
    };
    use crate::event::{EventSenders, create_event_busses};
    use crate::market_client::HttpMarketClient;
    use crate::roles::parse_market_url;
    use crate::web::{
        PriceSeriesQuery,
        api::{BestDealsQuery, TrendsQuery},
        encoded_body::EncodedBody,
        error::{ApiError, DiscordFailure, SharedWebError},
    };
    use axum::body::Body;
    use futures::future::BoxFuture;
    use http_body_util::BodyExt;
    use std::sync::Mutex;
    use ultros_api_types::{
        alert::{DiscordWritableChannel, DiscordWritableGuild},
        user::GroupSyncStatus,
        websocket::{ListingEventData, SaleEventData},
    };

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

    fn test_cheapest_payload() -> Bytes {
        // Larger than EncodedBody's 256-byte compression threshold so real
        // brotli/gzip negotiation and passthrough are exercised end-to-end.
        Bytes::from(
            serde_json::to_vec(&serde_json::json!({
                "item_id": (1..=120).collect::<Vec<i32>>()
            }))
            .unwrap(),
        )
    }

    impl MarketClient for StubMarketClient {
        fn cheapest<'a>(
            &'a self,
            world: &'a str,
            format: Option<&'a str>,
            headers: &'a axum::http::HeaderMap,
        ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
            Box::pin(async move {
                let all_ae: Vec<&str> = headers
                    .get_all(header::ACCEPT_ENCODING)
                    .iter()
                    .filter_map(|v| v.to_str().ok())
                    .collect();
                self.record(format!("cheapest:{world}:{format:?}:{}", all_ae.join("|")));
                if world == "MissingWorld" {
                    return Err(WebError::NotFound.into());
                }
                // Exercise real `EncodedBody` content negotiation (including
                // repeated headers, `q=0` exclusions, and `406 Not Acceptable`).
                let slab = EncodedBody::new(test_cheapest_payload());
                Ok(slab
                    .into_json_response(
                        headers,
                        HeaderValue::from_static(
                            "public, max-age=5, stale-while-revalidate=10, stale-if-error=600",
                        ),
                        &[],
                    )
                    .await)
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

    #[derive(Clone, Copy, Default)]
    enum StubBotMode {
        #[default]
        Ok,
        Offline,
        BadRequest,
        Forbidden,
        NotFound,
        Misconfigured,
        Transient,
        DeliveryFailed,
    }

    #[derive(Default)]
    struct StubBotAdminClient {
        mode: Mutex<StubBotMode>,
        recorded: Mutex<Vec<String>>,
    }

    impl StubBotAdminClient {
        fn set_mode(&self, mode: StubBotMode) {
            *self.mode.lock().unwrap() = mode;
        }

        fn mode(&self) -> StubBotMode {
            *self.mode.lock().unwrap()
        }

        fn record(&self, entry: String) {
            self.recorded.lock().unwrap().push(entry);
        }

        fn maybe_error(&self) -> Result<(), ApiError> {
            match self.mode() {
                StubBotMode::Ok | StubBotMode::DeliveryFailed => Ok(()),
                StubBotMode::Offline => Err(ApiError::ServiceUnavailable(
                    crate::bot_admin_client::DISCORD_BOT_OFFLINE,
                )),
                StubBotMode::BadRequest => {
                    Err(ApiError::BadRequest("invalid guild_id or reauth required"))
                }
                StubBotMode::Forbidden => Err(ApiError::Forbidden("not allowed")),
                StubBotMode::NotFound => Err(ApiError::from(anyhow::Error::from(
                    ultros_db::group_roles::GroupError::NotFound,
                ))),
                StubBotMode::Misconfigured => Err(ApiError::Discord {
                    message: "Discord refused the member search (403). The Ultros bot needs the Server Members intent enabled in the Discord developer portal.".into(),
                    kind: DiscordFailure::Misconfigured,
                }),
                StubBotMode::Transient => Err(ApiError::Discord {
                    message: "Discord member search failed: 429 Too Many Requests".into(),
                    kind: DiscordFailure::Transient,
                }),
            }
        }
    }

    impl BotAdminClient for StubBotAdminClient {
        fn bot_guild_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<i64>, ApiError>> {
            Box::pin(async move {
                self.record("bot_guild_ids".into());
                self.maybe_error()?;
                Ok(vec![100, 200, 300])
            })
        }

        fn manageable_guilds<'a>(
            &'a self,
            user_id: i64,
            candidate_guild_ids: Option<Vec<i64>>,
        ) -> BoxFuture<'a, Result<Vec<BotManageableGuild>, ApiError>> {
            Box::pin(async move {
                self.record(format!(
                    "manageable_guilds:{user_id}:{candidate_guild_ids:?}"
                ));
                self.maybe_error()?;
                Ok(vec![BotManageableGuild {
                    id: 100,
                    name: "Alpha Guild".into(),
                    icon_url: Some("https://cdn.discordapp.com/icons/100/abc.png".into()),
                }])
            })
        }

        fn writable_guilds<'a>(
            &'a self,
            user_id: i64,
            candidate_guild_ids: Option<Vec<i64>>,
        ) -> BoxFuture<'a, Result<Vec<DiscordWritableGuild>, ApiError>> {
            Box::pin(async move {
                self.record(format!("writable_guilds:{user_id}:{candidate_guild_ids:?}"));
                self.maybe_error()?;
                Ok(vec![DiscordWritableGuild {
                    id: 100,
                    name: "Alpha Guild".into(),
                    icon_url: None,
                    channels: vec![DiscordWritableChannel {
                        id: 555,
                        name: "market-alerts".into(),
                    }],
                }])
            })
        }

        fn require_manageable_guild<'a>(
            &'a self,
            guild_id: i64,
            user_id: i64,
        ) -> BoxFuture<'a, Result<BotManageableGuild, ApiError>> {
            Box::pin(async move {
                self.record(format!("require_manageable_guild:{guild_id}:{user_id}"));
                self.maybe_error()?;
                Ok(BotManageableGuild {
                    id: guild_id,
                    name: "Verified Guild".into(),
                    icon_url: None,
                })
            })
        }

        fn resolve_and_verify_channel<'a>(
            &'a self,
            channel_id: i64,
            user_id: i64,
        ) -> BoxFuture<'a, Result<BotResolvedChannel, ApiError>> {
            Box::pin(async move {
                self.record(format!("resolve_channel:{channel_id}:{user_id}"));
                self.maybe_error()?;
                Ok(BotResolvedChannel {
                    channel_name: "alerts".into(),
                    guild_id: 100,
                    guild_name: "Alpha Guild".into(),
                })
            })
        }

        fn importable_guild_roles<'a>(
            &'a self,
            guild_id: i64,
        ) -> BoxFuture<'a, Result<Vec<BotGuildRole>, ApiError>> {
            Box::pin(async move {
                self.record(format!("importable_guild_roles:{guild_id}"));
                self.maybe_error()?;
                Ok(vec![BotGuildRole {
                    id: 777,
                    name: "Raiders".into(),
                    position: 5,
                    color: Some("#5865f2".into()),
                }])
            })
        }

        fn search_guild_members<'a>(
            &'a self,
            guild_id: i64,
            query: &'a str,
            limit: u64,
        ) -> BoxFuture<'a, Result<Vec<BotGuildMember>, ApiError>> {
            Box::pin(async move {
                self.record(format!("search_guild_members:{guild_id}:{query}:{limit}"));
                self.maybe_error()?;
                Ok(vec![BotGuildMember {
                    user_id: 42,
                    display_name: "Y'shtola Rhul".into(),
                    avatar_url: "https://cdn.discordapp.com/avatars/42/face.png".into(),
                }])
            })
        }

        fn trigger_group_sync<'a>(
            &'a self,
            group_id: i32,
            owner_id: i64,
            trigger: GroupSyncTrigger,
        ) -> BoxFuture<'a, Result<GroupSyncStatus, ApiError>> {
            Box::pin(async move {
                self.record(format!(
                    "trigger_group_sync:{group_id}:{owner_id}:{trigger:?}"
                ));
                self.maybe_error()?;
                match trigger {
                    GroupSyncTrigger::Manual => Ok(GroupSyncStatus::RecentlySynced),
                    GroupSyncTrigger::PostRoleImport => Ok(GroupSyncStatus::Ran),
                }
            })
        }

        fn deliver_discord_notification<'a>(
            &'a self,
            endpoint_id: i32,
            owner_id: i64,
            title: &'a str,
            body: &'a str,
        ) -> BoxFuture<'a, Result<(), BotDeliveryError>> {
            Box::pin(async move {
                self.record(format!(
                    "deliver_discord_notification:{endpoint_id}:{owner_id}:{title}:{body}"
                ));
                match self.mode() {
                    StubBotMode::Ok => Ok(()),
                    StubBotMode::Offline => Err(BotDeliveryError::BotOffline),
                    StubBotMode::DeliveryFailed => Err(BotDeliveryError::Failed(
                        "Cannot send messages to this user".into(),
                    )),
                    StubBotMode::BadRequest | StubBotMode::Forbidden | StubBotMode::NotFound => {
                        Err(BotDeliveryError::Failed("endpoint rejected".into()))
                    }
                    StubBotMode::Misconfigured => {
                        Err(BotDeliveryError::Failed("Missing Permissions".into()))
                    }
                    StubBotMode::Transient => {
                        Err(BotDeliveryError::Failed("429 Too Many Requests".into()))
                    }
                }
            })
        }
    }

    struct TestMarketServer {
        addr: SocketAddr,
        url: url::Url,
        senders: EventSenders,
        active_streams: Arc<AtomicUsize>,
        shutdown: CancellationToken,
        handle: tokio::task::JoinHandle<()>,
    }

    async fn spawn_test_market_server_with_bot_on(
        bind_addr: SocketAddr,
        stub: Arc<StubMarketClient>,
        bot_stub: Arc<StubBotAdminClient>,
        token: Option<String>,
    ) -> TestMarketServer {
        let listener = TcpListener::bind(bind_addr).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let roles = Roles::parse(Some("ingest,market,bot")).unwrap();
        let (senders, receivers) = create_event_busses();
        let active_streams = Arc::new(AtomicUsize::new(0));
        let shutdown = CancellationToken::new();
        let app = router_with_stream_counter(
            stub,
            bot_stub,
            receivers,
            roles,
            token,
            shutdown.clone(),
            active_streams.clone(),
        );
        let server_shutdown = shutdown.clone();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    server_shutdown.cancelled().await;
                })
                .await
                .unwrap();
        });
        let url = parse_market_url(&format!("http://{addr}")).unwrap();
        TestMarketServer {
            addr,
            url,
            senders,
            active_streams,
            shutdown,
            handle,
        }
    }

    async fn spawn_test_market_server_on(
        bind_addr: SocketAddr,
        stub: Arc<StubMarketClient>,
        token: Option<String>,
    ) -> TestMarketServer {
        spawn_test_market_server_with_bot_on(
            bind_addr,
            stub,
            Arc::new(StubBotAdminClient::default()),
            token,
        )
        .await
    }

    async fn spawn_test_market_server(
        stub: Arc<StubMarketClient>,
        token: Option<String>,
    ) -> TestMarketServer {
        spawn_test_market_server_on(SocketAddr::from(([127, 0, 0, 1], 0)), stub, token).await
    }

    #[tokio::test]
    async fn http2_market_client_round_trips_all_endpoints_and_preserves_headers() {
        let stub = Arc::new(StubMarketClient::default());
        let srv = spawn_test_market_server(stub.clone(), Some("secret-token".into())).await;
        let url = srv.url.clone();
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
        let mut decoded = Vec::new();
        brotli::BrotliDecompress(&mut &body[..], &mut decoded).unwrap();
        assert_eq!(&decoded[..], &test_cheapest_payload()[..]);

        // 1b. Repeated Accept-Encoding headers combine and honor explicit `q=0`
        //     exclusions (header 1 allows `br, gzip`; header 2 forbids `br`).
        let mut repeated_ae = axum::http::HeaderMap::new();
        repeated_ae.append(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("br, gzip"),
        );
        repeated_ae.append(header::ACCEPT_ENCODING, HeaderValue::from_static("br;q=0"));
        let resp_q0 = client
            .cheapest("North-America", Some("columnar"), &repeated_ae)
            .await
            .unwrap();
        assert_eq!(resp_q0.status(), StatusCode::OK);
        assert_eq!(
            resp_q0.headers().get(header::CONTENT_ENCODING).unwrap(),
            "gzip",
            "combined Accept-Encoding headers must honor br;q=0 from second header"
        );

        // 1c. Deliberate `406 Not Acceptable` (all codings + identity forbidden)
        //     is preserved with Vary + Cache-Control: no-store rather than
        //     turning into a 503 outage.
        let mut forbid_all = axum::http::HeaderMap::new();
        forbid_all.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("*;q=0"));
        let resp_406 = client
            .cheapest("North-America", Some("columnar"), &forbid_all)
            .await
            .unwrap();
        assert_eq!(resp_406.status(), StatusCode::NOT_ACCEPTABLE);
        assert_eq!(
            resp_406.headers().get(header::VARY).unwrap(),
            "accept-encoding"
        );
        assert_eq!(
            resp_406.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );

        // 1d. Malformed and oversized Accept-Encoding headers fail fast with
        //     400 Bad Request rather than silently ignoring invalid qvalues or
        //     dropping exclusions (including RFC 9110 §12.4.2 whitespace around `=`).
        for bad in ["gzip, gzip;q=bogus", "gzip;q =0", "gzip;q= 0"] {
            let mut malformed_ae = axum::http::HeaderMap::new();
            malformed_ae.insert(header::ACCEPT_ENCODING, HeaderValue::from_static(bad));
            assert!(
                matches!(
                    client
                        .cheapest("North-America", Some("columnar"), &malformed_ae)
                        .await,
                    Err(e) if matches!(e.0.as_ref(), WebError::BadRequest)
                ),
                "malformed Accept-Encoding {bad:?} must return 400 Bad Request"
            );
        }
        let mut oversized_ae = axum::http::HeaderMap::new();
        let long_header = std::iter::repeat_n("gzip", 300)
            .collect::<Vec<_>>()
            .join(", ");
        oversized_ae.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_str(&long_header).unwrap(),
        );
        assert!(matches!(
            client
                .cheapest("North-America", Some("columnar"), &oversized_ae)
                .await,
            Err(e) if matches!(e.0.as_ref(), WebError::BadRequest)
        ));

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
        let unauthed_events = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("{url}internal/market/events"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauthed_events.status(), StatusCode::UNAUTHORIZED);

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
        let health = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
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

        srv.shutdown.cancel();
        let _ = srv.handle.await;
    }

    #[tokio::test]
    async fn split_market_event_relay_delivers_live_listings_and_sales_between_separate_buses() {
        let stub = Arc::new(StubMarketClient::default());
        let srv1 = spawn_test_market_server(stub.clone(), Some("relay-secret".into())).await;
        let bind_addr = srv1.addr;

        // Separate web-replica bus (no shared channels with `srv1.senders`).
        let (web_senders, web_receivers) = create_event_busses();
        let mut web_listings = web_receivers.listings.resubscribe();
        let mut web_sales = web_receivers.history.resubscribe();
        let mut web_resync = web_receivers.market_resync.resubscribe();

        let web_token = CancellationToken::new();
        let client = HttpMarketClient::new(srv1.url.clone(), Some("relay-secret")).unwrap();
        let relay = client.spawn_event_relay(web_senders, web_token.clone());

        // Wait deterministically for the actual `/internal/market/events` stream
        // handler to subscribe (never relying on Axum state clone counts).
        tokio::time::timeout(Duration::from_secs(5), async {
            while srv1.active_streams.load(Ordering::Acquire) < 1 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("relay must connect to /internal/market/events");

        // Publish listing add + listing remove + sale add on the market bus.
        srv1.senders
            .listings
            .send(EventType::added(ListingEventData {
                item_id: 5333,
                world_id: 54,
                listings: vec![],
            }))
            .unwrap();
        srv1.senders
            .listings
            .send(EventType::removed(ListingEventData {
                item_id: 5334,
                world_id: 54,
                listings: vec![],
            }))
            .unwrap();
        srv1.senders
            .history
            .send(EventType::added(SaleEventData { sales: vec![] }))
            .unwrap();

        let got_add = tokio::time::timeout(Duration::from_secs(5), web_listings.recv())
            .await
            .expect("listing add timeout")
            .unwrap();
        assert!(matches!(got_add, EventType::Add(ref d) if d.item_id == 5333 && d.world_id == 54));

        let got_remove = tokio::time::timeout(Duration::from_secs(5), web_listings.recv())
            .await
            .expect("listing remove timeout")
            .unwrap();
        assert!(
            matches!(got_remove, EventType::Remove(ref d) if d.item_id == 5334 && d.world_id == 54)
        );

        let got_sale = tokio::time::timeout(Duration::from_secs(5), web_sales.recv())
            .await
            .expect("sale add timeout")
            .unwrap();
        assert!(matches!(got_sale, EventType::Add(_)));

        // Restart the market server on the same address: the relay must
        // reconnect, emit `market_resync` so open sockets signal `Stale`, and
        // continue delivering new live events.
        srv1.shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(5), srv1.handle)
            .await
            .expect("first market server should shut down cleanly")
            .unwrap();

        let srv2 = spawn_test_market_server_on(bind_addr, stub, Some("relay-secret".into())).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            while srv2.active_streams.load(Ordering::Acquire) < 1 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("relay must reconnect to restarted market server");

        tokio::time::timeout(Duration::from_secs(5), web_resync.recv())
            .await
            .expect("market_resync must fire on reconnect")
            .unwrap();

        srv2.senders
            .listings
            .send(EventType::added(ListingEventData {
                item_id: 5335,
                world_id: 54,
                listings: vec![],
            }))
            .unwrap();
        let post_reconnect = tokio::time::timeout(Duration::from_secs(5), web_listings.recv())
            .await
            .expect("post-reconnect listing add timeout")
            .unwrap();
        assert!(
            matches!(post_reconnect, EventType::Add(ref d) if d.item_id == 5335 && d.world_id == 54)
        );

        web_token.cancel();
        srv2.shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(5), relay)
            .await
            .expect("relay should shut down cleanly")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), srv2.handle)
            .await
            .expect("second market server should shut down cleanly")
            .unwrap();
    }

    #[tokio::test]
    async fn http2_bot_admin_client_round_trips_all_endpoints_and_preserves_errors() {
        let market_stub = Arc::new(StubMarketClient::default());
        let bot_stub = Arc::new(StubBotAdminClient::default());
        let srv = spawn_test_market_server_with_bot_on(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            market_stub,
            bot_stub.clone(),
            Some("bot-secret".into()),
        )
        .await;

        let client = HttpBotAdminClient::new(srv.url.clone(), Some("bot-secret")).unwrap();

        // 1. All 9 endpoints succeed over HTTP/2.
        assert_eq!(client.bot_guild_ids().await.unwrap(), vec![100, 200, 300]);

        let manageable = client
            .manageable_guilds(99, Some(vec![100, 200]))
            .await
            .unwrap();
        assert_eq!(manageable.len(), 1);
        assert_eq!(manageable[0].name, "Alpha Guild");

        // Legacy fallback (`None`) also round-trips cleanly.
        let legacy_manageable = client.manageable_guilds(99, None).await.unwrap();
        assert_eq!(legacy_manageable.len(), 1);

        let writable = client.writable_guilds(99, Some(vec![100])).await.unwrap();
        assert_eq!(writable.len(), 1);
        assert_eq!(writable[0].channels[0].name, "market-alerts");

        let verified = client.require_manageable_guild(100, 99).await.unwrap();
        assert_eq!(verified.name, "Verified Guild");

        let resolved = client.resolve_and_verify_channel(555, 99).await.unwrap();
        assert_eq!(resolved.channel_name, "alerts");
        assert_eq!(resolved.guild_id, 100);

        let roles = client.importable_guild_roles(100).await.unwrap();
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].name, "Raiders");
        assert_eq!(roles[0].color.as_deref(), Some("#5865f2"));

        // Special characters and Unicode survive query encoding cleanly.
        let members = client
            .search_guild_members(100, "Y'shtola & アルフィノ #1", 10)
            .await
            .unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].user_id, 42);
        assert_eq!(members[0].display_name, "Y'shtola Rhul");

        // Whitespace-only query short-circuits without hitting the server.
        let empty = client.search_guild_members(100, "   ", 10).await.unwrap();
        assert!(empty.is_empty());

        assert_eq!(
            client
                .trigger_group_sync(7, 99, GroupSyncTrigger::PostRoleImport)
                .await
                .unwrap(),
            GroupSyncStatus::Ran
        );
        assert_eq!(
            client
                .trigger_group_sync(7, 99, GroupSyncTrigger::Manual)
                .await
                .unwrap(),
            GroupSyncStatus::RecentlySynced
        );

        client
            .deliver_discord_notification(7, 99, "Test title", "Test body")
            .await
            .unwrap();

        // 2. Structured error fidelity across HTTP/2:
        //    BadRequest -> 400 with exact client message intact.
        bot_stub.set_mode(StubBotMode::BadRequest);
        let bad_req_err = client.bot_guild_ids().await.unwrap_err();
        let bad_req_resp = bad_req_err.into_response();
        assert_eq!(bad_req_resp.status(), StatusCode::BAD_REQUEST);
        let bad_req_body = bad_req_resp.into_body().collect().await.unwrap().to_bytes();
        assert!(
            std::str::from_utf8(&bad_req_body)
                .unwrap()
                .contains("invalid guild_id or reauth required")
        );

        //    Forbidden -> 403.
        bot_stub.set_mode(StubBotMode::Forbidden);
        let forbidden_err = client
            .trigger_group_sync(7, 99, GroupSyncTrigger::Manual)
            .await
            .unwrap_err();
        assert_eq!(
            forbidden_err.into_response().status(),
            StatusCode::FORBIDDEN
        );

        //    NotFound -> 404.
        bot_stub.set_mode(StubBotMode::NotFound);
        let not_found_err = client
            .trigger_group_sync(7, 99, GroupSyncTrigger::Manual)
            .await
            .unwrap_err();
        assert_eq!(
            not_found_err.into_response().status(),
            StatusCode::NOT_FOUND
        );

        //    BotOffline -> 503 ServiceUnavailable + BotDeliveryError::BotOffline.
        bot_stub.set_mode(StubBotMode::Offline);
        let offline_err = client.bot_guild_ids().await.unwrap_err();
        assert!(matches!(offline_err, ApiError::ServiceUnavailable(_)));
        assert_eq!(
            offline_err.into_response().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            client
                .deliver_discord_notification(7, 99, "Test", "Body")
                .await
                .unwrap_err(),
            BotDeliveryError::BotOffline
        );

        //    DiscordFailure::Misconfigured -> 502 Bad Gateway with Server Members wording intact.
        bot_stub.set_mode(StubBotMode::Misconfigured);
        let misconfig_err = client
            .search_guild_members(100, "ali", 10)
            .await
            .unwrap_err();
        assert!(matches!(
            misconfig_err,
            ApiError::Discord {
                kind: DiscordFailure::Misconfigured,
                ..
            }
        ));
        let misconfig_resp = misconfig_err.into_response();
        assert_eq!(misconfig_resp.status(), StatusCode::BAD_GATEWAY);
        let misconfig_body = misconfig_resp
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        assert!(
            std::str::from_utf8(&misconfig_body)
                .unwrap()
                .contains("Server Members intent")
        );

        //    DiscordFailure::Transient -> 503 Service Unavailable with message intact.
        bot_stub.set_mode(StubBotMode::Transient);
        let transient_err = client
            .search_guild_members(100, "ali", 10)
            .await
            .unwrap_err();
        assert!(matches!(
            transient_err,
            ApiError::Discord {
                kind: DiscordFailure::Transient,
                ..
            }
        ));
        assert_eq!(
            transient_err.into_response().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );

        //    Delivery failure preserves exact error string.
        bot_stub.set_mode(StubBotMode::DeliveryFailed);
        assert_eq!(
            client
                .deliver_discord_notification(7, 99, "Test", "Body")
                .await
                .unwrap_err(),
            BotDeliveryError::Failed("Cannot send messages to this user".into())
        );

        // 3. Wrong bearer token rejected with 401 and surfaced as internal error (500).
        bot_stub.set_mode(StubBotMode::Ok);
        let bad_client = HttpBotAdminClient::new(srv.url.clone(), Some("wrong-secret")).unwrap();
        let auth_err = bad_client.bot_guild_ids().await.unwrap_err();
        assert_eq!(
            auth_err.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        let delivery_auth_err = bad_client
            .deliver_discord_notification(7, 99, "Test", "Body")
            .await
            .unwrap_err();
        assert!(matches!(delivery_auth_err, BotDeliveryError::Failed(_)));

        // 4. Raw HTTP checks: Cache-Control: no-store on 200, 401, 413, and malformed JSON.
        let raw_client = reqwest::Client::builder()
            .no_proxy()
            .http2_prior_knowledge()
            .build()
            .unwrap();
        let raw_ok = raw_client
            .get(format!("{}internal/bot/guilds", srv.url))
            .header(header::AUTHORIZATION, "Bearer bot-secret")
            .send()
            .await
            .unwrap();
        assert_eq!(raw_ok.status(), StatusCode::OK);
        assert_eq!(
            raw_ok
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store")
        );

        let raw_unauth = raw_client
            .get(format!("{}internal/bot/guilds", srv.url))
            .send()
            .await
            .unwrap();
        assert_eq!(raw_unauth.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            raw_unauth
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store")
        );

        let oversized = vec![b'a'; INTERNAL_BOT_MAX_BODY_BYTES + 1024];
        let raw_big = raw_client
            .post(format!("{}internal/bot/deliver-notification", srv.url))
            .header(header::AUTHORIZATION, "Bearer bot-secret")
            .header(header::CONTENT_TYPE, "application/json")
            .body(oversized)
            .send()
            .await
            .unwrap();
        assert_eq!(raw_big.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            raw_big
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store")
        );

        let raw_bad_json = raw_client
            .post(format!("{}internal/bot/deliver-notification", srv.url))
            .header(header::AUTHORIZATION, "Bearer bot-secret")
            .header(header::CONTENT_TYPE, "application/json")
            .body("{not valid json")
            .send()
            .await
            .unwrap();
        assert!(raw_bad_json.status().is_client_error());
        assert_eq!(
            raw_bad_json
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store")
        );

        let recorded = bot_stub.recorded.lock().unwrap().clone();
        assert!(
            recorded
                .iter()
                .any(|e| e == "manageable_guilds:99:Some([100, 200])")
        );
        assert!(recorded.iter().any(|e| e == "manageable_guilds:99:None"));
        assert!(
            recorded
                .iter()
                .any(|e| e == "search_guild_members:100:Y'shtola & アルフィノ #1:10")
        );
        assert!(
            recorded
                .iter()
                .any(|e| e == "trigger_group_sync:7:99:PostRoleImport")
        );
        assert!(
            recorded
                .iter()
                .any(|e| e == "trigger_group_sync:7:99:Manual")
        );
        assert!(
            recorded
                .iter()
                .any(|e| e == "deliver_discord_notification:7:99:Test title:Test body")
        );

        srv.shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(5), srv.handle)
            .await
            .expect("market server should shut down cleanly")
            .unwrap();
    }

    #[tokio::test]
    async fn bot_admin_endpoints_fail_closed_when_token_is_unset_or_blank() {
        for token in [None, Some("   ".to_string())] {
            let market_stub = Arc::new(StubMarketClient::default());
            let bot_stub = Arc::new(StubBotAdminClient::default());
            let srv = spawn_test_market_server_with_bot_on(
                SocketAddr::from(([127, 0, 0, 1], 0)),
                market_stub,
                bot_stub,
                token.clone(),
            )
            .await;

            // HttpBotAdminClient also refuses construction without a non-blank token.
            assert!(HttpBotAdminClient::new(srv.url.clone(), token.as_deref()).is_err());

            let raw_client = reqwest::Client::builder()
                .no_proxy()
                .http2_prior_knowledge()
                .build()
                .unwrap();
            let resp = raw_client
                .get(format!("{}internal/bot/guilds", srv.url))
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                resp.headers()
                    .get(header::CACHE_CONTROL)
                    .and_then(|v| v.to_str().ok()),
                Some("no-store")
            );

            // Presenting an arbitrary Bearer header still fails closed.
            let resp_with_bearer = raw_client
                .get(format!("{}internal/bot/guilds", srv.url))
                .header(header::AUTHORIZATION, "Bearer anything")
                .send()
                .await
                .unwrap();
            assert_eq!(resp_with_bearer.status(), StatusCode::UNAUTHORIZED);

            srv.shutdown.cancel();
            tokio::time::timeout(Duration::from_secs(5), srv.handle)
                .await
                .expect("market server should shut down cleanly")
                .unwrap();
        }
    }

    #[tokio::test]
    async fn deliver_discord_notification_rejects_unexpected_200_ok() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        let app = Router::new().route(
            "/internal/bot/deliver-notification",
            post(|| async { (StatusCode::OK, "misrouted upstream html") }),
        );
        let server_shutdown = shutdown.clone();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    server_shutdown.cancelled().await;
                })
                .await
                .unwrap();
        });

        let url = parse_market_url(&format!("http://{addr}")).unwrap();
        let client = HttpBotAdminClient::new(url.clone(), Some("bot-secret")).unwrap();
        let err = client
            .deliver_discord_notification(7, 99, "Test", "Body")
            .await
            .unwrap_err();
        assert!(
            matches!(err, BotDeliveryError::Failed(ref msg) if msg.contains("200") && msg.contains("204")),
            "expected explicit 204 contract error on unexpected 200 OK, got: {err:?}"
        );

        shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn http2_bot_admin_client_times_out_cleanly_on_stalled_upstream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        let app = Router::new()
            .route(
                "/internal/bot/guilds",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    Json(vec![1i64])
                }),
            )
            .route(
                "/internal/bot/deliver-notification",
                post(|| async {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    StatusCode::NO_CONTENT
                }),
            );
        let server_shutdown = shutdown.clone();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    server_shutdown.cancelled().await;
                })
                .await
                .unwrap();
        });

        let url = parse_market_url(&format!("http://{addr}")).unwrap();
        let client = HttpBotAdminClient::with_timeouts_for_test(
            url,
            Some("bot-secret"),
            Duration::from_millis(80),
        )
        .unwrap();

        let err = client.bot_guild_ids().await.unwrap_err();
        assert!(matches!(err, ApiError::ServiceUnavailable(_)));

        let delivery_err = client
            .deliver_discord_notification(7, 99, "Test", "Body")
            .await
            .unwrap_err();
        assert!(matches!(delivery_err, BotDeliveryError::Failed(_)));

        shutdown.cancel();
        handle.abort();
    }

    #[tokio::test]
    async fn in_process_bot_admin_over_http2_rejects_invalid_ids_without_panicking() {
        let pool = sea_orm::sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://ultros:ultros@127.0.0.1:1/ultros")
            .unwrap();
        let db = ultros_db::UltrosDb::from_connection(
            sea_orm::SqlxPostgresConnector::from_sqlx_postgres_pool(pool),
        );
        let bot_admin: Arc<dyn BotAdminClient> = Arc::new(InProcessBotAdminClient::new(db));
        let market_stub = Arc::new(StubMarketClient::default());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let roles = Roles::parse(Some("ingest,market,bot")).unwrap();
        let (_senders, receivers) = create_event_busses();
        let shutdown = CancellationToken::new();
        let app = router(
            market_stub,
            bot_admin,
            receivers,
            roles,
            Some("bot-secret".into()),
            shutdown.clone(),
        );
        let server_shutdown = shutdown.clone();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    server_shutdown.cancelled().await;
                })
                .await
                .unwrap();
        });

        let url = parse_market_url(&format!("http://{addr}")).unwrap();
        let client = HttpBotAdminClient::new(url.clone(), Some("bot-secret")).unwrap();

        for invalid in [0i64, -1] {
            assert_eq!(
                client
                    .manageable_guilds(invalid, None)
                    .await
                    .unwrap_err()
                    .into_response()
                    .status(),
                StatusCode::BAD_REQUEST
            );
            assert_eq!(
                client
                    .manageable_guilds(42, Some(vec![100, invalid]))
                    .await
                    .unwrap_err()
                    .into_response()
                    .status(),
                StatusCode::BAD_REQUEST
            );
            assert_eq!(
                client
                    .require_manageable_guild(invalid, 42)
                    .await
                    .unwrap_err()
                    .into_response()
                    .status(),
                StatusCode::BAD_REQUEST
            );
            assert_eq!(
                client
                    .resolve_and_verify_channel(invalid, 42)
                    .await
                    .unwrap_err()
                    .into_response()
                    .status(),
                StatusCode::BAD_REQUEST
            );
            assert_eq!(
                client
                    .importable_guild_roles(invalid)
                    .await
                    .unwrap_err()
                    .into_response()
                    .status(),
                StatusCode::BAD_REQUEST
            );
            assert_eq!(
                client
                    .search_guild_members(invalid, "ali", 10)
                    .await
                    .unwrap_err()
                    .into_response()
                    .status(),
                StatusCode::BAD_REQUEST
            );
            assert_eq!(
                client
                    .trigger_group_sync(0, 42, GroupSyncTrigger::Manual)
                    .await
                    .unwrap_err()
                    .into_response()
                    .status(),
                StatusCode::BAD_REQUEST
            );
            assert_eq!(
                client
                    .trigger_group_sync(7, invalid, GroupSyncTrigger::Manual)
                    .await
                    .unwrap_err()
                    .into_response()
                    .status(),
                StatusCode::BAD_REQUEST
            );
            assert!(matches!(
                client
                    .deliver_discord_notification(0, 42, "Test", "Body")
                    .await
                    .unwrap_err(),
                BotDeliveryError::Failed(_)
            ));
        }

        // Also hit the server directly with `guild_id=0` on member search
        // (bypassing HttpBotAdminClient's client-side guard) to verify the
        // worker rejects it with 400 + no-store and never panics in Serenity.
        let raw_client = reqwest::Client::builder()
            .no_proxy()
            .http2_prior_knowledge()
            .build()
            .unwrap();
        let raw_zero_search = raw_client
            .get(format!(
                "{}internal/bot/guilds/0/members/search?q=ali&limit=10",
                url
            ))
            .header(header::AUTHORIZATION, "Bearer bot-secret")
            .send()
            .await
            .unwrap();
        assert_eq!(raw_zero_search.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            raw_zero_search
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store")
        );

        shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .unwrap()
            .unwrap();
    }
}
