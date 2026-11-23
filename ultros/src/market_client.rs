//! Typed `MarketClient` boundary between `ultros-web` and `ultros-market`.
//!
//! Two implementations share one trait:
//! - [`InProcessMarketClient`]: direct in-process calls when `ULTROS_ROLES`
//!   includes `market` (including the default `ULTROS_ROLES=all`), with zero
//!   extra network hop, serialization, or compression overhead.
//! - [`HttpMarketClient`]: coarse multiplexed HTTP/2 client targeting
//!   `ULTROS_MARKET_URL` when running stateless `ULTROS_ROLES=web` replicas.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use ultros_api_types::websocket::{ListingEventData, SaleEventData};
use ultros_api_types::world_helper::WorldHelper;
use ultros_clickhouse::ClickHouseClient;
use ultros_db::{UltrosDb, world_data::world_cache::WorldCache};
use universalis::UniversalisClient;
use url::Url;

use crate::{
    analyzer_service::AnalyzerService,
    event::{EventSenders, EventType},
    web::{
        PriceSeriesQuery,
        api::{
            BestDealsQuery, CheapestCache, TrendsQuery, serve_best_deals, serve_cheapest_per_world,
            serve_listing_stats, serve_market_pulse, serve_movers, serve_recent_sales,
            serve_sale_stats, serve_trends,
        },
        error::{SharedWebError, WebError},
        home_feed_cache::HomeFeedCache,
        price_series_cache::PriceSeriesCache,
        refresh_jobs::RefreshJobs,
        run_refresh_world_item_listings, serve_price_series,
        sitemap::collect_sitemap_item_sales,
        stats_cache::{ListingStatsCache, SaleStatsCache},
    },
};

/// Compact per-item sale timestamps needed by `/sitemap/items.xml`.
///
/// Sent as a single coarse payload across `MarketClient` instead of shipping
/// per-world raw sale history maps over the network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SitemapItemSaleSummary {
    pub(crate) item_id: i32,
    pub(crate) first_sale_unix: Option<i64>,
    pub(crate) median_sale_unix: Option<i64>,
}

/// Outcome of a manual market-listing refresh request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefreshOutcome {
    Refreshed,
    /// Returned when `test-auth` market isolation blocks upstream Universalis
    /// refreshes (`409 Conflict`).
    MarketIsolationBlocked,
}

/// NDJSON frame streamed over `GET /internal/market/events` from
/// `ultros-market` to `ultros-web` replicas so live market WebSockets
/// (`/api/v1/realtime/events` and `/alerts/websocket`) receive real-time
/// listings and sales across the two-role split.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum MarketStreamEvent {
    ListingsAdd { data: ListingEventData },
    ListingsRemove { data: ListingEventData },
    SalesAdd { data: SaleEventData },
    SalesRemove { data: SaleEventData },
    Lagged,
    Keepalive,
}

/// Typed boundary for market reads, sitemap sale summaries, and manual
/// refreshes owned by the `market` role.
pub(crate) trait MarketClient: Send + Sync {
    fn cheapest<'a>(
        &'a self,
        world: &'a str,
        format: Option<&'a str>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>>;

    fn recent_sales<'a>(
        &'a self,
        world: &'a str,
        format: Option<&'a str>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>>;

    fn best_deals<'a>(
        &'a self,
        world: &'a str,
        query: &'a BestDealsQuery,
    ) -> BoxFuture<'a, Result<Response, WebError>>;

    fn trends<'a>(
        &'a self,
        world: &'a str,
        query: &'a TrendsQuery,
    ) -> BoxFuture<'a, Result<Response, WebError>>;

    fn sale_stats<'a>(
        &'a self,
        world: &'a str,
        window: Option<u16>,
        format: Option<&'a str>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, WebError>>;

    fn listing_stats<'a>(
        &'a self,
        world: &'a str,
        window: Option<u16>,
        format: Option<&'a str>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, WebError>>;

    fn price_series<'a>(
        &'a self,
        world: &'a str,
        item_id: i32,
        query: &'a PriceSeriesQuery,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>>;

    fn market_pulse<'a>(
        &'a self,
        world: &'a str,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>>;

    fn movers<'a>(
        &'a self,
        world: &'a str,
        direction: Option<&'a str>,
        limit: Option<u32>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>>;

    fn sitemap_item_sales<'a>(
        &'a self,
    ) -> BoxFuture<'a, Result<Vec<SitemapItemSaleSummary>, WebError>>;

    fn refresh_listings<'a>(
        &'a self,
        world: &'a str,
        item_id: i32,
    ) -> BoxFuture<'a, Result<RefreshOutcome, WebError>>;
}

/// Dependencies owned by the `market` role for [`InProcessMarketClient`].
#[derive(Clone)]
pub(crate) struct MarketServices {
    pub(crate) db: UltrosDb,
    pub(crate) analyzer_service: AnalyzerService,
    pub(crate) world_cache: Arc<WorldCache>,
    pub(crate) world_helper: Arc<WorldHelper>,
    pub(crate) ch_client: ClickHouseClient,
    pub(crate) universalis: UniversalisClient,
    pub(crate) listing_events:
        ultros_clickhouse::writer::Writer<ultros_clickhouse::rows::ListingEventRow>,
    pub(crate) event_senders: EventSenders,
    pub(crate) price_series_cache: PriceSeriesCache,
    pub(crate) home_feed_cache: HomeFeedCache,
    pub(crate) sale_stats_cache: SaleStatsCache,
    pub(crate) listing_stats_cache: ListingStatsCache,
    pub(crate) cheapest_cache: CheapestCache,
    pub(crate) refresh_jobs: RefreshJobs,
}

/// Direct in-process [`MarketClient`] used whenever the `market` role is active
/// in this binary (`ULTROS_ROLES=all` or `ULTROS_ROLES=ingest,market,bot`).
#[derive(Clone)]
pub(crate) struct InProcessMarketClient {
    services: MarketServices,
}

impl InProcessMarketClient {
    pub(crate) fn new(services: MarketServices) -> Self {
        Self { services }
    }
}

impl MarketClient for InProcessMarketClient {
    fn cheapest<'a>(
        &'a self,
        world: &'a str,
        format: Option<&'a str>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
        Box::pin(async move {
            serve_cheapest_per_world(
                &self.services.analyzer_service,
                &self.services.world_cache,
                &self.services.cheapest_cache,
                headers,
                world,
                format,
            )
            .await
        })
    }

    fn recent_sales<'a>(
        &'a self,
        world: &'a str,
        format: Option<&'a str>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
        Box::pin(async move {
            serve_recent_sales(
                &self.services.analyzer_service,
                &self.services.world_cache,
                &self.services.home_feed_cache,
                headers,
                world,
                format,
            )
            .await
        })
    }

    fn best_deals<'a>(
        &'a self,
        world: &'a str,
        query: &'a BestDealsQuery,
    ) -> BoxFuture<'a, Result<Response, WebError>> {
        Box::pin(async move {
            serve_best_deals(
                &self.services.analyzer_service,
                &self.services.world_cache,
                world,
                query,
            )
            .await
        })
    }

    fn trends<'a>(
        &'a self,
        world: &'a str,
        query: &'a TrendsQuery,
    ) -> BoxFuture<'a, Result<Response, WebError>> {
        Box::pin(async move {
            serve_trends(
                &self.services.analyzer_service,
                &self.services.world_cache,
                world,
                query,
            )
            .await
        })
    }

    fn sale_stats<'a>(
        &'a self,
        world: &'a str,
        window: Option<u16>,
        format: Option<&'a str>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, WebError>> {
        Box::pin(async move {
            serve_sale_stats(
                &self.services.ch_client,
                &self.services.world_cache,
                &self.services.sale_stats_cache,
                headers,
                world,
                window,
                format,
            )
            .await
        })
    }

    fn listing_stats<'a>(
        &'a self,
        world: &'a str,
        window: Option<u16>,
        format: Option<&'a str>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, WebError>> {
        Box::pin(async move {
            serve_listing_stats(
                &self.services.ch_client,
                &self.services.world_cache,
                &self.services.listing_stats_cache,
                headers,
                world,
                window,
                format,
            )
            .await
        })
    }

    fn price_series<'a>(
        &'a self,
        world: &'a str,
        item_id: i32,
        query: &'a PriceSeriesQuery,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
        Box::pin(async move {
            serve_price_series(
                &self.services.world_cache,
                &self.services.ch_client,
                &self.services.price_series_cache,
                world,
                item_id,
                query,
            )
            .await
        })
    }

    fn market_pulse<'a>(
        &'a self,
        world: &'a str,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
        Box::pin(async move {
            serve_market_pulse(
                &self.services.ch_client,
                &self.services.world_cache,
                &self.services.home_feed_cache,
                headers,
                world,
            )
            .await
        })
    }

    fn movers<'a>(
        &'a self,
        world: &'a str,
        direction: Option<&'a str>,
        limit: Option<u32>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
        Box::pin(async move {
            serve_movers(
                &self.services.ch_client,
                &self.services.world_helper,
                &self.services.home_feed_cache,
                headers,
                world,
                direction,
                limit,
            )
            .await
        })
    }

    fn sitemap_item_sales<'a>(
        &'a self,
    ) -> BoxFuture<'a, Result<Vec<SitemapItemSaleSummary>, WebError>> {
        Box::pin(async move {
            collect_sitemap_item_sales(&self.services.world_helper, &self.services.analyzer_service)
                .await
        })
    }

    fn refresh_listings<'a>(
        &'a self,
        world: &'a str,
        item_id: i32,
    ) -> BoxFuture<'a, Result<RefreshOutcome, WebError>> {
        Box::pin(async move {
            run_refresh_world_item_listings(
                &self.services.db,
                &self.services.event_senders,
                &self.services.world_cache,
                &self.services.universalis,
                &self.services.refresh_jobs,
                &self.services.listing_events,
                world,
                item_id,
            )
            .await
        })
    }
}

/// Coarse HTTP/2 [`MarketClient`] used by stateless `ULTROS_ROLES=web`
/// replicas to query `ultros-market` over `ULTROS_MARKET_URL`.
///
/// Uses `reqwest` with auto-decompression disabled so pre-compressed
/// `Arc<EncodedBody>` slabs (`br`/`zstd`/`gzip`) pass straight through from
/// `ultros-market` to browsers without decompressing or re-compressing on
/// `ultros-web`, while in-process SSR (`SsrApi`, no `Accept-Encoding`) gets
/// uncompressed JSON directly.
#[derive(Clone)]
pub(crate) struct HttpMarketClient {
    base_url: Url,
    client: reqwest::Client,
    stream_client: reqwest::Client,
    bearer_auth: Option<HeaderValue>,
}

const READ_TIMEOUT: Duration = Duration::from_secs(15);
const REFRESH_TIMEOUT: Duration = Duration::from_secs(35);
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_ACCEPT_ENCODING_LEN: usize = 1024;
const MAX_STREAM_LINE_BYTES: usize = 4 * 1024 * 1024;

impl HttpMarketClient {
    pub(crate) fn new(base_url: Url, bearer_token: Option<&str>) -> anyhow::Result<Self> {
        let build_client = |total_timeout: Option<Duration>| -> anyhow::Result<reqwest::Client> {
            let mut builder = reqwest::Client::builder()
                // Private internal RPC must never route through ambient
                // HTTP_PROXY/HTTPS_PROXY settings (which break h2c and could
                // leak ULTROS_MARKET_TOKEN to an external proxy).
                .no_proxy()
                .connect_timeout(Duration::from_secs(3))
                .pool_idle_timeout(Duration::from_secs(90))
                .http2_keep_alive_interval(Some(Duration::from_secs(20)))
                .http2_keep_alive_timeout(Duration::from_secs(10))
                .http2_keep_alive_while_idle(true)
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(crate::UNIVERSALIS_USER_AGENT);
            if let Some(timeout) = total_timeout {
                builder = builder.timeout(timeout);
            }
            if base_url.scheme() == "http" {
                // Plaintext internal Docker/LAN links speak h2c prior-knowledge
                // directly against `ultros-market`'s HTTP/2-capable Axum server.
                builder = builder.http2_prior_knowledge();
            }
            Ok(builder.build()?)
        };
        let client = build_client(Some(READ_TIMEOUT))?;
        let stream_client = build_client(None)?;
        let bearer_auth =
            match bearer_token.map(str::trim).filter(|s| !s.is_empty()) {
                Some(token) => Some(HeaderValue::from_str(&format!("Bearer {token}")).map_err(
                    |e| anyhow::anyhow!("invalid ULTROS_MARKET_TOKEN header value: {e}"),
                )?),
                None => None,
            };
        Ok(Self {
            base_url,
            client,
            stream_client,
            bearer_auth,
        })
    }

    /// Spawn a background HTTP/2 NDJSON relay from `GET /internal/market/events`
    /// onto this web replica's local `EventSenders.listings` and `.history`
    /// buses so live market WebSockets keep receiving real-time events in split
    /// mode (`ULTROS_ROLES=web`).
    pub(crate) fn spawn_event_relay(
        &self,
        senders: EventSenders,
        token: CancellationToken,
    ) -> JoinHandle<()> {
        let client = self.clone();
        tokio::spawn(async move {
            let mut backoff = Duration::from_millis(250);
            let mut needs_resync = false;
            loop {
                if token.is_cancelled() {
                    return;
                }
                match client
                    .run_event_stream_once(&senders, &token, &mut needs_resync)
                    .await
                {
                    Ok(()) => return,
                    Err(connected) => {
                        // Either an initial connect attempt failed while browser
                        // subscriptions may already exist, or a live stream
                        // dropped — emit `market_resync` once reconnected.
                        needs_resync = true;
                        if connected {
                            backoff = Duration::from_millis(250);
                        }
                        if token.is_cancelled() {
                            return;
                        }
                        tokio::select! {
                            _ = token.cancelled() => return,
                            _ = tokio::time::sleep(backoff) => {}
                        }
                        backoff = (backoff * 2).min(Duration::from_secs(5));
                    }
                }
            }
        })
    }

    /// Connects once to `/internal/market/events` and relays frames until
    /// shutdown (`Ok(())`) or stream error (`Err(did_connect)`).
    async fn run_event_stream_once(
        &self,
        senders: &EventSenders,
        token: &CancellationToken,
        needs_resync: &mut bool,
    ) -> Result<(), bool> {
        let segments = ["internal", "market", "events"];
        let url = match self.endpoint_url(&segments, &[]) {
            Ok(url) => url,
            Err(error) => {
                warn!(?error, "invalid market event stream URL");
                return Err(false);
            }
        };
        let mut req = self.stream_client.get(url);
        if let Some(auth) = &self.bearer_auth {
            req = req.header(header::AUTHORIZATION, auth.clone());
        }
        // Bound the wait for response headers too so a peer that accepts TCP
        // and stalls before sending headers triggers backoff and retry.
        let resp = tokio::select! {
            _ = token.cancelled() => return Ok(()),
            res = tokio::time::timeout(READ_TIMEOUT, req.send()) => match res {
                Ok(Ok(resp)) => resp,
                Ok(Err(error)) => {
                    warn!(?error, "failed connecting to ultros-market event stream");
                    return Err(false);
                }
                Err(_) => {
                    warn!("timed out waiting for ultros-market event stream headers");
                    return Err(false);
                }
            }
        };
        if resp.status() != StatusCode::OK {
            warn!(
                status = %resp.status(),
                "ultros-market event stream rejected connection"
            );
            return Err(false);
        }
        info!("connected to ultros-market live event relay");
        if *needs_resync {
            let _ = senders.market_resync.send(());
            *needs_resync = false;
        }

        let mut resp = resp;
        let mut line_buf: Vec<u8> = Vec::new();
        loop {
            let next_chunk = tokio::select! {
                _ = token.cancelled() => return Ok(()),
                res = tokio::time::timeout(STREAM_IDLE_TIMEOUT, resp.chunk()) => res,
            };
            let chunk = match next_chunk {
                Ok(Ok(Some(bytes))) => bytes,
                Ok(Ok(None)) => {
                    warn!("ultros-market event stream ended; reconnecting");
                    return Err(true);
                }
                Ok(Err(error)) => {
                    warn!(
                        ?error,
                        "ultros-market event stream read failed; reconnecting"
                    );
                    return Err(true);
                }
                Err(_) => {
                    warn!("ultros-market event stream idle timeout; reconnecting");
                    return Err(true);
                }
            };

            let mut slice = chunk.as_ref();
            while let Some(pos) = slice.iter().position(|&b| b == b'\n') {
                let segment = &slice[..pos];
                if line_buf.len() + segment.len() > MAX_STREAM_LINE_BYTES {
                    warn!("ultros-market event stream frame exceeded max line size; reconnecting");
                    return Err(true);
                }
                line_buf.extend_from_slice(segment);
                if !line_buf.is_empty() {
                    dispatch_market_stream_frame(&line_buf, senders);
                    line_buf.clear();
                }
                slice = &slice[pos + 1..];
            }
            if !slice.is_empty() {
                if line_buf.len() + slice.len() > MAX_STREAM_LINE_BYTES {
                    warn!("ultros-market event stream frame exceeded max line size; reconnecting");
                    return Err(true);
                }
                line_buf.extend_from_slice(slice);
            }
        }
    }

    fn endpoint_url(&self, segments: &[&str], query: &[(&str, String)]) -> Result<Url, WebError> {
        let mut url = self.base_url.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("ULTROS_MARKET_URL cannot be a base"))?;
            path.pop_if_empty();
            for segment in segments {
                path.push(segment);
            }
        }
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        Ok(url)
    }

    /// Combine and strictly validate all incoming `Accept-Encoding` header
    /// fields per RFC 9110 §12.5.3 so repeated headers and explicit `q=0`
    /// exclusions are preserved verbatim, while malformed syntax (e.g.
    /// `gzip;q=bogus`) or oversized values fail with `400 Bad Request`.
    fn forward_accept_encoding(
        headers: Option<&HeaderMap>,
    ) -> Result<Option<HeaderValue>, WebError> {
        let Some(headers) = headers else {
            return Ok(None);
        };
        let mut combined = String::new();
        for raw in headers.get_all(header::ACCEPT_ENCODING) {
            let text = raw.to_str().map_err(|_| WebError::BadRequest)?;
            if !combined.is_empty() {
                combined.push_str(", ");
            }
            combined.push_str(text);
            if combined.len() > MAX_ACCEPT_ENCODING_LEN {
                return Err(WebError::BadRequest);
            }
        }
        if combined.is_empty() {
            return Ok(None);
        }
        validate_accept_encoding(&combined)?;
        HeaderValue::from_str(&combined)
            .map(Some)
            .map_err(|_| WebError::BadRequest)
    }

    async fn proxy_json_get(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
        incoming_headers: Option<&HeaderMap>,
    ) -> Result<Response, WebError> {
        let url = self.endpoint_url(segments, query)?;
        let mut req = self.client.get(url);
        if let Some(ae) = Self::forward_accept_encoding(incoming_headers)? {
            req = req.header(header::ACCEPT_ENCODING, ae);
        }
        let resp = req.send().await.map_err(|error| {
            warn!(?error, path = ?segments, "ultros-market GET failed");
            WebError::TemporarilyUnavailable
        })?;
        let status = resp.status();
        if status == StatusCode::NOT_ACCEPTABLE {
            // Preserve content-negotiation 406 responses (e.g. `identity;q=0`
            // with all codings forbidden) instead of turning them into 503.
            let cache_control = resp
                .headers()
                .get(header::CACHE_CONTROL)
                .cloned()
                .unwrap_or_else(|| HeaderValue::from_static("no-store"));
            let vary = resp
                .headers()
                .get(header::VARY)
                .cloned()
                .unwrap_or_else(|| HeaderValue::from_static("accept-encoding"));
            return Response::builder()
                .status(StatusCode::NOT_ACCEPTABLE)
                .header(header::CACHE_CONTROL, cache_control)
                .header(header::VARY, vary)
                .body(Body::empty())
                .map_err(|e| WebError::from(anyhow::Error::from(e)));
        }
        if status != StatusCode::OK {
            return Err(map_upstream_status(status, segments));
        }

        let content_type_ok = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.to_ascii_lowercase().starts_with("application/json"));
        if !content_type_ok {
            warn!(
                path = ?segments,
                content_type = ?resp.headers().get(header::CONTENT_TYPE),
                "ultros-market returned non-JSON 200 response"
            );
            return Err(WebError::TemporarilyUnavailable);
        }

        // Fail-closed: preserve upstream Cache-Control (including `no-store` on
        // degraded 200 fallbacks), and default to `no-store` if missing.
        let cache_control = resp
            .headers()
            .get(header::CACHE_CONTROL)
            .cloned()
            .unwrap_or_else(|| HeaderValue::from_static("no-store"));
        let content_encoding = resp.headers().get(header::CONTENT_ENCODING).cloned();
        let vary = resp.headers().get(header::VARY).cloned();

        let body = resp.bytes().await.map_err(|error| {
            warn!(?error, path = ?segments, "ultros-market body read failed");
            WebError::TemporarilyUnavailable
        })?;

        let mut builder = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CACHE_CONTROL, cache_control);
        if let Some(encoding) = content_encoding {
            builder = builder.header(header::CONTENT_ENCODING, encoding);
            if vary.is_none() {
                builder = builder.header(header::VARY, HeaderValue::from_static("accept-encoding"));
            }
        }
        if let Some(vary) = vary {
            builder = builder.header(header::VARY, vary);
        }
        builder
            .body(Body::from(body))
            .map_err(|e| WebError::from(anyhow::Error::from(e)))
    }
}

fn is_http_tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// Validate RFC 9110 §12.4.2 qvalue syntax: `0` / `0.` / `0.d{1,3}` or
/// `1` / `1.` / `1.0{1,3}`.
fn is_valid_qvalue(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    match bytes {
        [b'0'] | [b'1'] => true,
        [b'0', b'.', frac @ ..] if frac.len() <= 3 => frac.iter().all(u8::is_ascii_digit),
        [b'1', b'.', frac @ ..] if frac.len() <= 3 => frac.iter().all(|&b| b == b'0'),
        _ => false,
    }
}

fn validate_accept_encoding(combined: &str) -> Result<(), WebError> {
    for raw_item in combined.split(',') {
        let item = raw_item.trim();
        if item.is_empty() {
            continue;
        }
        let mut parts = item.split(';');
        let coding = parts.next().unwrap_or("").trim();
        if coding.is_empty() || !coding.bytes().all(is_http_tchar) {
            return Err(WebError::BadRequest);
        }
        let mut saw_weight = false;
        for raw_param in parts {
            let param = raw_param.trim();
            let Some((k, v)) = param.split_once('=') else {
                return Err(WebError::BadRequest);
            };
            if saw_weight || !k.trim().eq_ignore_ascii_case("q") || !is_valid_qvalue(v.trim()) {
                return Err(WebError::BadRequest);
            }
            saw_weight = true;
        }
    }
    Ok(())
}

fn dispatch_market_stream_frame(line: &[u8], senders: &EventSenders) {
    match serde_json::from_slice::<MarketStreamEvent>(line) {
        Ok(MarketStreamEvent::ListingsAdd { data }) => {
            let _ = senders.listings.send(EventType::added(data));
        }
        Ok(MarketStreamEvent::ListingsRemove { data }) => {
            let _ = senders.listings.send(EventType::removed(data));
        }
        Ok(MarketStreamEvent::SalesAdd { data }) => {
            let _ = senders.history.send(EventType::added(data));
        }
        Ok(MarketStreamEvent::SalesRemove { data }) => {
            let _ = senders.history.send(EventType::removed(data));
        }
        Ok(MarketStreamEvent::Lagged) => {
            warn!("ultros-market event stream reported lag; signaling web market_resync");
            let _ = senders.market_resync.send(());
        }
        Ok(MarketStreamEvent::Keepalive) => {
            debug!("ultros-market event stream keepalive");
        }
        Err(error) => {
            warn!(?error, "failed decoding ultros-market event stream frame");
        }
    }
}

fn map_upstream_status(status: StatusCode, segments: &[&str]) -> WebError {
    match status {
        StatusCode::BAD_REQUEST => WebError::BadRequest,
        StatusCode::NOT_FOUND => WebError::NotFound,
        StatusCode::SERVICE_UNAVAILABLE => WebError::TemporarilyUnavailable,
        other => {
            warn!(status = %other, path = ?segments, "ultros-market returned unexpected status");
            WebError::TemporarilyUnavailable
        }
    }
}

impl MarketClient for HttpMarketClient {
    fn cheapest<'a>(
        &'a self,
        world: &'a str,
        format: Option<&'a str>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
        Box::pin(async move {
            let mut query = Vec::new();
            if let Some(format) = format {
                query.push(("format", format.to_string()));
            }
            self.proxy_json_get(&["api", "v1", "cheapest", world], &query, Some(headers))
                .await
                .map_err(SharedWebError::from)
        })
    }

    fn recent_sales<'a>(
        &'a self,
        world: &'a str,
        format: Option<&'a str>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
        Box::pin(async move {
            let mut query = Vec::new();
            if let Some(format) = format {
                query.push(("format", format.to_string()));
            }
            self.proxy_json_get(&["api", "v1", "recentSales", world], &query, Some(headers))
                .await
                .map_err(SharedWebError::from)
        })
    }

    fn best_deals<'a>(
        &'a self,
        world: &'a str,
        q: &'a BestDealsQuery,
    ) -> BoxFuture<'a, Result<Response, WebError>> {
        Box::pin(async move {
            let mut query = Vec::new();
            if let Some(v) = q.min_profit {
                query.push(("min_profit", v.to_string()));
            }
            if let Some(v) = &q.filter_sale {
                query.push(("filter_sale", v.clone()));
            }
            if let Some(v) = q.show_suspicious {
                query.push(("show_suspicious", if v { "1" } else { "0" }.to_string()));
            }
            if let Some(v) = q.limit {
                query.push(("limit", v.to_string()));
            }
            if let Some(v) = q.min_velocity {
                query.push(("min_velocity", v.to_string()));
            }
            if let Some(v) = q.min_buffer_sales {
                query.push(("min_buffer_sales", v.to_string()));
            }
            if let Some(v) = q.max_roi {
                query.push(("max_roi", v.to_string()));
            }
            self.proxy_json_get(&["api", "v1", "best_deals", world], &query, None)
                .await
        })
    }

    fn trends<'a>(
        &'a self,
        world: &'a str,
        q: &'a TrendsQuery,
    ) -> BoxFuture<'a, Result<Response, WebError>> {
        Box::pin(async move {
            let mut query = Vec::new();
            if let Some(v) = q.window {
                query.push(("window", v.to_string()));
            }
            if let Some(v) = q.show_suspicious {
                query.push(("show_suspicious", if v { "1" } else { "0" }.to_string()));
            }
            self.proxy_json_get(&["api", "v1", "trends", world], &query, None)
                .await
        })
    }

    fn sale_stats<'a>(
        &'a self,
        world: &'a str,
        window: Option<u16>,
        format: Option<&'a str>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, WebError>> {
        Box::pin(async move {
            let mut query = Vec::new();
            if let Some(w) = window {
                query.push(("window", w.to_string()));
            }
            if let Some(f) = format {
                query.push(("format", f.to_string()));
            }
            self.proxy_json_get(&["api", "v1", "sale_stats", world], &query, Some(headers))
                .await
        })
    }

    fn listing_stats<'a>(
        &'a self,
        world: &'a str,
        window: Option<u16>,
        format: Option<&'a str>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, WebError>> {
        Box::pin(async move {
            let mut query = Vec::new();
            if let Some(w) = window {
                query.push(("window", w.to_string()));
            }
            if let Some(f) = format {
                query.push(("format", f.to_string()));
            }
            self.proxy_json_get(
                &["api", "v1", "listing_stats", world],
                &query,
                Some(headers),
            )
            .await
        })
    }

    fn price_series<'a>(
        &'a self,
        world: &'a str,
        item_id: i32,
        q: &'a PriceSeriesQuery,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
        Box::pin(async move {
            let item_seg = item_id.to_string();
            let mut query = Vec::new();
            if let Some(v) = q.from {
                query.push(("from", v.to_string()));
            }
            if let Some(v) = q.to {
                query.push(("to", v.to_string()));
            }
            if let Some(v) = q.bucket {
                query.push(("bucket", v.to_string()));
            }
            if let Some(v) = &q.group {
                query.push(("group", v.clone()));
            }
            if let Some(v) = &q.hq {
                query.push(("hq", v.clone()));
            }
            self.proxy_json_get(
                &["api", "v1", "price_series", world, &item_seg],
                &query,
                None,
            )
            .await
            .map_err(SharedWebError::from)
        })
    }

    fn market_pulse<'a>(
        &'a self,
        world: &'a str,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
        Box::pin(async move {
            self.proxy_json_get(&["api", "v1", "market_pulse", world], &[], Some(headers))
                .await
                .map_err(SharedWebError::from)
        })
    }

    fn movers<'a>(
        &'a self,
        world: &'a str,
        direction: Option<&'a str>,
        limit: Option<u32>,
        headers: &'a HeaderMap,
    ) -> BoxFuture<'a, Result<Response, SharedWebError>> {
        Box::pin(async move {
            let mut query = Vec::new();
            if let Some(d) = direction {
                query.push(("direction", d.to_string()));
            }
            if let Some(l) = limit {
                query.push(("limit", l.to_string()));
            }
            self.proxy_json_get(&["api", "v1", "movers", world], &query, Some(headers))
                .await
                .map_err(SharedWebError::from)
        })
    }

    fn sitemap_item_sales<'a>(
        &'a self,
    ) -> BoxFuture<'a, Result<Vec<SitemapItemSaleSummary>, WebError>> {
        Box::pin(async move {
            let segments = ["internal", "market", "sitemap_sales"];
            let url = self.endpoint_url(&segments, &[])?;
            let mut req = self.client.get(url);
            if let Some(auth) = &self.bearer_auth {
                req = req.header(header::AUTHORIZATION, auth.clone());
            }
            let resp = req.send().await.map_err(|error| {
                warn!(?error, "ultros-market sitemap_sales request failed");
                WebError::TemporarilyUnavailable
            })?;
            let status = resp.status();
            if status != StatusCode::OK {
                return Err(map_upstream_status(status, &segments));
            }
            let bytes = resp.bytes().await.map_err(|error| {
                warn!(?error, "ultros-market sitemap_sales body read failed");
                WebError::TemporarilyUnavailable
            })?;
            serde_json::from_slice(&bytes).map_err(|error| {
                warn!(?error, "ultros-market sitemap_sales JSON decode failed");
                WebError::TemporarilyUnavailable
            })
        })
    }

    fn refresh_listings<'a>(
        &'a self,
        world: &'a str,
        item_id: i32,
    ) -> BoxFuture<'a, Result<RefreshOutcome, WebError>> {
        Box::pin(async move {
            let item_seg = item_id.to_string();
            let segments = ["internal", "market", "refresh", world, &item_seg];
            let url = self.endpoint_url(&segments, &[])?;
            let mut req = self.client.post(url).timeout(REFRESH_TIMEOUT);
            if let Some(auth) = &self.bearer_auth {
                req = req.header(header::AUTHORIZATION, auth.clone());
            }
            let resp = req.send().await.map_err(|error| {
                warn!(?error, world, item_id, "ultros-market refresh failed");
                WebError::TemporarilyUnavailable
            })?;
            match resp.status() {
                StatusCode::OK | StatusCode::NO_CONTENT => Ok(RefreshOutcome::Refreshed),
                StatusCode::CONFLICT => Ok(RefreshOutcome::MarketIsolationBlocked),
                other => Err(map_upstream_status(other, &segments)),
            }
        })
    }
}
