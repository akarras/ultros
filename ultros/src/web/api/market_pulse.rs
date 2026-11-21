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
use ultros_clickhouse::ClickHouseClient;
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

            let listings_failed = active_listings_res.is_err();
            if let Err(e) = &active_listings_res {
                tracing::warn!(
                    error = ?e,
                    scope_id,
                    "market_pulse active_listing_count CH query failed, returning 0 placeholder"
                );
            }
            let active_listings = active_listings_res.unwrap_or(0);

            // ClickHouse soft-fail: when either query errors we still render
            // the strip with whatever succeeded and zeros for the rest rather
            // than 500ing the whole home page. Degraded responses set
            // `cacheable: false` so they never populate/overwrite
            // `HomeFeedCache` and emit `Cache-Control: no-store`.
            let soft_failed = pulse.is_err() || listings_failed;
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
        return Ok(crate::web::uncached_json(loaded.body.raw().clone()));
    }
    Ok(cached_encoded_json(&loaded.body, &headers, ttl).await)
}

#[cfg(test)]
mod tests {
    use super::get_market_pulse;
    use crate::web::home_feed_cache::HomeFeedCache;
    use axum::{
        extract::{Path, State},
        http::{HeaderMap, StatusCode, header},
        response::IntoResponse,
    };
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use ultros_api_types::market_pulse::MarketPulseDto;
    use ultros_clickhouse::ClickHouseClient;
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

    async fn spawn_mock_clickhouse(
        pulse_ok: bool,
        listings_ok: bool,
    ) -> (ClickHouseClient, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let req = read_http_request(&mut socket).await;
                let (status, body) = if req.contains("listing_alive") {
                    if listings_ok {
                        (200, 12_345u64.to_le_bytes().to_vec())
                    } else {
                        (500, b"Code: 159. TIMEOUT_EXCEEDED".to_vec())
                    }
                } else if pulse_ok {
                    (200, encode_pulse_row(54))
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
        (client, handle)
    }

    fn test_world_cache() -> Arc<WorldCache> {
        Arc::new(WorldCache::from_models(
            vec![world::Model {
                id: 54,
                name: "Sargatanas".into(),
                datacenter_id: 10,
            }],
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
        let (ch, server) = spawn_mock_clickhouse(true, false).await;
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
        assert!(
            cache.get("pulse:Sargatanas").is_none(),
            "degraded pulse must not populate HomeFeedCache"
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let dto: MarketPulseDto = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(dto.sales_today, 100);
        assert_eq!(dto.active_listings, 0);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn market_pulse_caches_healthy_clickhouse_response() {
        let (ch, server) = spawn_mock_clickhouse(true, true).await;
        let cache = HomeFeedCache::new(16);
        let response = get_market_pulse(
            State(ch),
            State(test_world_cache()),
            State(cache.clone()),
            HeaderMap::new(),
            Path("Sargatanas".into()),
        )
        .await
        .expect("healthy pulse returns 200")
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=30, stale-while-revalidate=120, stale-if-error=600"
        );
        assert_eq!(
            response.headers().get(header::VARY).unwrap(),
            "accept-encoding"
        );
        assert!(cache.get("pulse:Sargatanas").is_some());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let dto: MarketPulseDto = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(dto.sales_today, 100);
        assert_eq!(dto.active_listings, 12_345);
        server.await.unwrap();
    }
}
