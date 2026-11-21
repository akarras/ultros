//! `/api/v1/market_pulse/{world}` — feeds the home-page Market Pulse strip.
//!
//! Pulls the 24h / 24-48h KPI rollups (`world_kpi_5min`) and the active-listing
//! count (`listing_alive`) concurrently from ClickHouse under an in-process
//! stale-while-revalidate `HomeFeedCache` slot — eliminating the live Postgres
//! `COUNT(*)` over `active_listing` that previously drove a ~9.75s p95 tail.
//! `{world}` may also name a datacenter or region, which sums its worlds.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::HeaderMap,
    response::IntoResponse,
};
use ultros_api_types::market_pulse::MarketPulseDto;
use ultros_clickhouse::{ClickHouseClient, queries::ActiveListingCount};
use ultros_db::world_data::world_cache::{AnySelector, WorldCache};

use crate::web::{
    cached_encoded_json,
    error::{SharedWebError, WebError},
    home_feed_cache::HomeFeedCache,
    json_cache::SwrLoad,
};

pub(crate) async fn get_market_pulse(
    State(ch): State<ClickHouseClient>,
    State(world_cache): State<Arc<WorldCache>>,
    State(cache): State<HomeFeedCache>,
    headers: HeaderMap,
    Path(world_name): Path<String>,
) -> Result<impl IntoResponse, SharedWebError> {
    let scope = world_cache
        .lookup_value_by_name(&world_name)
        .map_err(WebError::from)?;
    // A world reports as itself; a datacenter or region sums its worlds and
    // reports `world_id: 0` (the logged-out home page shows a whole region).
    let (scope_id, world_ids) = match AnySelector::from(&scope) {
        AnySelector::World(id) => (id, vec![id]),
        _ => (0, world_cache.get_all_worlds_in(&scope).unwrap_or_default()),
    };
    let ttl = Duration::from_secs(30);
    let stale_ttl = Duration::from_secs(300);
    let cache_key = format!("pulse:{}", scope.get_name());

    let loaded = cache
        .get_or_load_swr(cache_key, ttl, stale_ttl, move || async move {
            // Two fast ClickHouse queries concurrently: KPI rollup +
            // `listing_alive` sum (no Postgres `COUNT(*)` scan).
            let pulse_future = ultros_clickhouse::queries::market_pulse(&ch, scope_id, &world_ids);
            let listings_future = ultros_clickhouse::queries::active_listing_count(&ch, &world_ids);
            let (pulse, active_listings_res) = tokio::join!(pulse_future, listings_future);

            let (active_listings, listings_ready) = match active_listings_res {
                Ok(ActiveListingCount::Ready(count)) => (count, true),
                Ok(ActiveListingCount::NotReady { count }) => {
                    tracing::warn!(
                        scope_id,
                        count,
                        "market_pulse listing_alive rollup not ready or stale, serving uncached"
                    );
                    (count, false)
                }
                Err(e) => {
                    tracing::warn!(
                        error = ?e,
                        scope_id,
                        "market_pulse active_listing_count CH query failed, returning 0 placeholder"
                    );
                    (0, false)
                }
            };

            // ClickHouse soft-fail: when either query errors — or when
            // `listing_alive` is unseeded, unrefreshed post-seed, or stale — we
            // still render the strip rather than 500ing the home page. Degraded
            // responses set `cacheable: false` so they never populate/overwrite
            // `HomeFeedCache` and emit `Cache-Control: no-store`.
            let soft_failed = pulse.is_err() || !listings_ready;
            let dto = match pulse {
                Ok(p) => MarketPulseDto {
                    world_id: p.world_id,
                    sales_today: p.sales_today,
                    sales_yesterday: p.sales_yesterday,
                    gil_volume_today: p.gil_volume_today,
                    gil_volume_yesterday: p.gil_volume_yesterday,
                    unit_volume_today: p.unit_volume_today,
                    unit_volume_yesterday: p.unit_volume_yesterday,
                    active_listings,
                },
                Err(e) => {
                    tracing::warn!(
                        error = ?e,
                        scope_id,
                        "market_pulse CH query failed, returning quiet placeholders"
                    );
                    MarketPulseDto {
                        world_id: scope_id,
                        sales_today: 0,
                        sales_yesterday: 0,
                        gil_volume_today: 0,
                        gil_volume_yesterday: 0,
                        unit_volume_today: 0,
                        unit_volume_yesterday: 0,
                        active_listings,
                    }
                }
            };

            let body = serde_json::to_vec(&dto).map_err(anyhow::Error::from)?;
            Ok(SwrLoad {
                body: Bytes::from(body),
                cacheable: !soft_failed,
            })
        })
        .await?;

    metrics::counter!(
        "ultros_home_feed_cache_total",
        "feed" => "market_pulse",
        "disposition" => loaded.disposition.as_str()
    )
    .increment(1);
    if !loaded.cacheable {
        return Ok(loaded
            .body
            .into_json_response(
                &headers,
                axum::http::HeaderValue::from_static("no-store"),
                &[],
            )
            .await);
    }
    Ok(cached_encoded_json(&loaded, &headers, ttl).await)
}

#[cfg(test)]
mod tests {
    use super::get_market_pulse;
    use crate::web::{
        encoded_body::EncodedBody,
        home_feed_cache::HomeFeedCache,
        json_cache::{SwrDisposition, SwrValue},
    };
    use axum::{
        extract::{Path, State},
        http::{HeaderMap, StatusCode, header},
        response::IntoResponse,
    };
    use http_body_util::BodyExt;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use ultros_api_types::market_pulse::MarketPulseDto;
    use ultros_clickhouse::{ClickHouseClient, queries::LISTING_ALIVE_MAX_STALENESS_SECS};
    use ultros_db::{
        entity::{datacenter, region, world},
        world_data::world_cache::WorldCache,
    };

    async fn read_http_request(socket: &mut tokio::net::TcpStream) -> String {
        let mut buf = Vec::with_capacity(4096);
        let mut chunk = [0u8; 1024];
        let header_end = loop {
            let n = socket.read(&mut chunk).await.unwrap();
            if n == 0 {
                return String::new();
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
        };
        let headers = String::from_utf8_lossy(&buf[..header_end]);
        let content_len = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        while buf.len() < header_end + content_len {
            let n = socket.read(&mut chunk).await.unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn encode_pulse_row(world_id: i32) -> Vec<u8> {
        let mut out = Vec::with_capacity(52);
        out.extend_from_slice(&world_id.to_le_bytes());
        for v in [100u64, 80, 500_000, 400_000, 250, 200] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    fn encode_listing_count_row(
        total: u64,
        is_seeded: u8,
        seeded_at_unix: i64,
        last_refresh_unix: i64,
        now_unix: i64,
    ) -> Vec<u8> {
        let mut out = Vec::with_capacity(33);
        out.extend_from_slice(&total.to_le_bytes());
        out.push(is_seeded);
        out.extend_from_slice(&seeded_at_unix.to_le_bytes());
        out.extend_from_slice(&last_refresh_unix.to_le_bytes());
        out.extend_from_slice(&now_unix.to_le_bytes());
        out
    }

    #[derive(Clone, Copy)]
    enum MockListings {
        Row {
            total: u64,
            is_seeded: u8,
            seeded_at_unix: i64,
            last_refresh_unix: i64,
            now_unix: i64,
        },
        Error,
    }

    async fn spawn_mock_clickhouse(
        pulse_world_id: Option<i32>,
        listings: MockListings,
    ) -> (
        ClickHouseClient,
        Arc<Mutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen_requests = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen_requests.clone();
        let handle = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let req = read_http_request(&mut socket).await;
                seen_clone.lock().unwrap().push(req.clone());
                let (status, body) = if req.contains("listing_alive") {
                    match listings {
                        MockListings::Row {
                            total,
                            is_seeded,
                            seeded_at_unix,
                            last_refresh_unix,
                            now_unix,
                        } => (
                            200,
                            encode_listing_count_row(
                                total,
                                is_seeded,
                                seeded_at_unix,
                                last_refresh_unix,
                                now_unix,
                            ),
                        ),
                        MockListings::Error => (500, b"Code: 159. TIMEOUT_EXCEEDED".to_vec()),
                    }
                } else if let Some(wid) = pulse_world_id {
                    (200, encode_pulse_row(wid))
                } else {
                    (500, b"Code: 241. MEMORY_LIMIT_EXCEEDED".to_vec())
                };
                let reason = if status == 200 {
                    "OK"
                } else {
                    "Internal Server Error"
                };
                let head = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(head.as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
                let _ = socket.shutdown().await;
            }
        });
        let client = ClickHouseClient::from_client(
            clickhouse::Client::default()
                .with_url(url)
                .with_compression(clickhouse::Compression::None)
                .with_validation(false),
        );
        (client, seen_requests, handle)
    }

    fn test_world_cache() -> Arc<WorldCache> {
        Arc::new(WorldCache::from_models(
            vec![
                world::Model {
                    id: 54,
                    name: "Sargatanas".into(),
                    datacenter_id: 10,
                },
                world::Model {
                    id: 55,
                    name: "Gilgamesh".into(),
                    datacenter_id: 10,
                },
            ],
            vec![datacenter::Model {
                id: 10,
                name: "Aether".into(),
                region_id: 1,
            }],
            vec![region::Model {
                id: 1,
                name: "North-America".into(),
            }],
        ))
    }

    #[tokio::test]
    async fn market_pulse_returns_no_store_and_skips_cache_when_listing_count_fails() {
        let (ch, _seen, server) = spawn_mock_clickhouse(Some(54), MockListings::Error).await;
        let cache = HomeFeedCache::new(16);
        let response = get_market_pulse(
            State(ch),
            State(test_world_cache()),
            State(cache.clone()),
            HeaderMap::new(),
            Path("Sargatanas".into()),
        )
        .await
        .expect("soft-fail returns 200")
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert!(cache.get("pulse:Sargatanas").is_none());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let dto: MarketPulseDto = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(dto.sales_today, 100);
        assert_eq!(dto.active_listings, 0);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn market_pulse_returns_no_store_when_kpi_fails_but_listings_succeed() {
        let now = 1_800_000_000;
        let (ch, _seen, server) = spawn_mock_clickhouse(
            None,
            MockListings::Row {
                total: 12_345,
                is_seeded: 1,
                seeded_at_unix: now - 600,
                last_refresh_unix: now - 60,
                now_unix: now,
            },
        )
        .await;
        let cache = HomeFeedCache::new(16);
        let response = get_market_pulse(
            State(ch),
            State(test_world_cache()),
            State(cache.clone()),
            HeaderMap::new(),
            Path("Sargatanas".into()),
        )
        .await
        .expect("KPI soft-fail returns 200")
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert!(cache.get("pulse:Sargatanas").is_none());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let dto: MarketPulseDto = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(dto.sales_today, 0);
        assert_eq!(dto.active_listings, 12_345);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn market_pulse_does_not_cache_unseeded_unrefreshed_or_stale_listing_alive() {
        let now = 1_800_000_000;
        let cases = [
            (
                "unseeded",
                MockListings::Row {
                    total: 0,
                    is_seeded: 0,
                    seeded_at_unix: 0,
                    last_refresh_unix: 0,
                    now_unix: now,
                },
            ),
            (
                "seeded-but-unrefreshed",
                MockListings::Row {
                    total: 0,
                    is_seeded: 1,
                    seeded_at_unix: now - 300,
                    last_refresh_unix: 0,
                    now_unix: now,
                },
            ),
            (
                "pre-seed-partial-refresh",
                MockListings::Row {
                    total: 500,
                    is_seeded: 1,
                    seeded_at_unix: now - 300,
                    last_refresh_unix: now - 305,
                    now_unix: now,
                },
            ),
            (
                "stale-generation",
                MockListings::Row {
                    total: 12_345,
                    is_seeded: 1,
                    seeded_at_unix: now - 10_000,
                    last_refresh_unix: now - LISTING_ALIVE_MAX_STALENESS_SECS - 10,
                    now_unix: now,
                },
            ),
        ];

        for (label, mock_listings) in cases {
            let (ch, _seen, server) = spawn_mock_clickhouse(Some(54), mock_listings).await;
            let cache = HomeFeedCache::new(16);
            let response = get_market_pulse(
                State(ch),
                State(test_world_cache()),
                State(cache.clone()),
                HeaderMap::new(),
                Path("Sargatanas".into()),
            )
            .await
            .expect("not-ready rollup still renders 200")
            .into_response();

            assert_eq!(response.status(), StatusCode::OK, "case {label}");
            assert_eq!(
                response.headers().get(header::CACHE_CONTROL).unwrap(),
                "no-store",
                "case {label} must emit no-store"
            );
            assert!(
                cache.get("pulse:Sargatanas").is_none(),
                "case {label} must not populate HomeFeedCache"
            );
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn market_pulse_caches_genuine_zero_and_multi_world_scope() {
        let now = 1_800_000_000;

        // Genuine zero: seeded and freshly refreshed post-seed.
        let (ch, _seen, server) = spawn_mock_clickhouse(
            Some(54),
            MockListings::Row {
                total: 0,
                is_seeded: 1,
                seeded_at_unix: now - 600,
                last_refresh_unix: now - 60,
                now_unix: now,
            },
        )
        .await;
        let cache = HomeFeedCache::new(16);
        let response = get_market_pulse(
            State(ch),
            State(test_world_cache()),
            State(cache.clone()),
            HeaderMap::new(),
            Path("Sargatanas".into()),
        )
        .await
        .expect("genuine zero returns 200")
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=30, stale-while-revalidate=120, stale-if-error=600"
        );
        assert!(cache.get("pulse:Sargatanas").is_some());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let dto: MarketPulseDto = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(dto.active_listings, 0);
        server.await.unwrap();

        // Multi-world scope (region) queries FINAL across all constituent worlds.
        let (ch, seen, server) = spawn_mock_clickhouse(
            Some(0),
            MockListings::Row {
                total: 42_000,
                is_seeded: 1,
                seeded_at_unix: now - 600,
                last_refresh_unix: now - 30,
                now_unix: now,
            },
        )
        .await;
        let response = get_market_pulse(
            State(ch),
            State(test_world_cache()),
            State(cache.clone()),
            HeaderMap::new(),
            Path("North-America".into()),
        )
        .await
        .expect("region pulse returns 200")
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::VARY).unwrap(),
            "accept-encoding"
        );
        assert!(cache.get("pulse:North-America").is_some());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let dto: MarketPulseDto = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(dto.world_id, 0);
        assert_eq!(dto.active_listings, 42_000);
        server.await.unwrap();

        let requests = seen.lock().unwrap();
        let listing_req = requests
            .iter()
            .find(|r| r.contains("listing_alive"))
            .expect("listing_alive query issued");
        assert!(listing_req.contains("listing_alive FINAL"));
        assert!(
            listing_req.contains("_listing_alive_refresh"),
            "readiness query must check _listing_alive_refresh marker so empty boards succeed: {listing_req}"
        );
        assert!(
            listing_req.contains("world_id IN (54,55)")
                || listing_req.contains("world_id IN (55,54)"),
            "multi-world scope must fold all member worlds: {listing_req}"
        );
    }

    #[tokio::test]
    async fn stale_swr_response_emits_zero_max_age() {
        let now = std::time::Instant::now();
        let stale_val = SwrValue {
            body: EncodedBody::new(br#"{"world_id":54}"#.as_slice()),
            cacheable: true,
            disposition: SwrDisposition::Stale,
            stored_at: now - Duration::from_secs(299),
            stale_until: now + Duration::from_secs(1),
        };
        let resp =
            crate::web::cached_encoded_json(&stale_val, &HeaderMap::new(), Duration::from_secs(30))
                .await;
        assert_eq!(
            resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=0, stale-if-error=600"
        );
    }

    #[tokio::test]
    async fn degraded_market_pulse_honors_accept_encoding_and_406() {
        use std::io::Read;

        // Both KPI and active_listing_count fail -> tiny placeholder JSON
        // (< 256 bytes) must still honor `gzip, identity;q=0` and `*;q=0`.
        let (ch, _seen, server) = spawn_mock_clickhouse(None, MockListings::Error).await;
        let cache = HomeFeedCache::new(16);
        let mut gzip_only = HeaderMap::new();
        gzip_only.insert(
            header::ACCEPT_ENCODING,
            header::HeaderValue::from_static("gzip, identity;q=0"),
        );
        let resp = get_market_pulse(
            State(ch),
            State(test_world_cache()),
            State(cache.clone()),
            gzip_only,
            Path("Sargatanas".into()),
        )
        .await
        .expect("degraded fallback returns 200")
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert_eq!(
            resp.headers().get(header::CONTENT_ENCODING).unwrap(),
            "gzip"
        );
        assert_eq!(resp.headers().get(header::VARY).unwrap(), "accept-encoding");
        let wire = resp.into_body().collect().await.unwrap().to_bytes();
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(&wire[..])
            .read_to_end(&mut decoded)
            .unwrap();
        let dto: MarketPulseDto = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(dto.sales_today, 0);
        assert_eq!(dto.active_listings, 0);
        server.await.unwrap();

        let (ch, _seen, server) = spawn_mock_clickhouse(None, MockListings::Error).await;
        let mut reject_all = HeaderMap::new();
        reject_all.insert(
            header::ACCEPT_ENCODING,
            header::HeaderValue::from_static("*;q=0"),
        );
        let resp = get_market_pulse(
            State(ch),
            State(test_world_cache()),
            State(cache),
            reject_all,
            Path("Sargatanas".into()),
        )
        .await
        .expect("406 is an HTTP response")
        .into_response();
        assert_eq!(resp.status(), StatusCode::NOT_ACCEPTABLE);
        assert_eq!(
            resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert_eq!(resp.headers().get(header::VARY).unwrap(), "accept-encoding");
        server.await.unwrap();
    }
}
