//! `/api/v1/movers/{world}` and `/api/v1/sparklines/{world}` — the data
//! feed for the home-page Market Movers list + sparkline-bearing rows
//! anywhere else (Top Deals retrofit, Continue Tracking, etc).

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use serde::Deserialize;
use ultros_api_types::{
    sparklines::{
        MoverItem, MoversResponse, SparklineSeries, SparklinesRequest, SparklinesResponse,
    },
    world_helper::{AnySelector, WorldHelper},
};
use ultros_clickhouse::{
    ClickHouseClient,
    queries::{MoverDirection, MoverScope},
};

use crate::web::{cached_json, error::WebError, home_feed_cache::HomeFeedCache};

#[derive(Debug, Deserialize)]
pub(crate) struct MoversQuery {
    /// `rising` (default), `falling`, `volume` (unit count), or `gil`.
    direction: Option<String>,
    /// Result count; clamped to [1, 50]. Default 10.
    limit: Option<u32>,
}

pub(crate) async fn get_movers(
    State(ch): State<ClickHouseClient>,
    State(world_helper): State<Arc<WorldHelper>>,
    State(cache): State<HomeFeedCache>,
    Path(world_name): Path<String>,
    Query(q): Query<MoversQuery>,
) -> Result<impl IntoResponse, WebError> {
    let scope = world_helper
        .lookup_world_by_name(&world_name)
        .ok_or(WebError::NotFound)?;
    // A world ranks its own market; a datacenter or region ranks its worlds
    // folded together (see `top_movers`) and reports `world_id: 0`.
    let (scope_id, world_ids, mover_scope): (i32, Vec<i32>, MoverScope) =
        match AnySelector::from(&scope) {
            AnySelector::World(id) => (id, vec![id], MoverScope::World),
            _ => (
                0,
                scope.all_worlds().map(|w| w.id).collect(),
                MoverScope::Group,
            ),
        };

    // Parse direction with a default. Reject unknown values rather than
    // silently coercing to rising, so the frontend can rely on round-trip
    // fidelity.
    let direction_str = q
        .direction
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "rising".to_string());
    let direction = match direction_str.as_str() {
        "rising" => MoverDirection::Rising,
        "falling" => MoverDirection::Falling,
        "volume" => MoverDirection::Volume,
        "gil" => MoverDirection::Gil,
        _ => return Err(WebError::BadRequest),
    };
    let limit = q.limit.unwrap_or(10).clamp(1, 50);

    // Sales_hourly refreshes every 15 min; 60s stays ahead of the data
    // without paying ClickHouse on every page load.
    let ttl = Duration::from_secs(60);
    let cache_key = format!("movers:{}:{direction_str}:{limit}", scope.get_name());
    if let Some(body) = cache.get(&cache_key) {
        return Ok(cached_json(body, ttl));
    }

    let rows = ultros_clickhouse::queries::top_movers(
        &ch,
        scope_id,
        &world_ids,
        mover_scope,
        direction,
        limit,
    )
    .await
    .map_err(|e| {
        tracing::warn!(error = ?e, scope_id, "top_movers CH query failed");
        crate::web::error::ClickHouseQueryError::new("top_movers", e)
    })?;

    // For each mover, fetch the 24h sparkline — folded over the same worlds
    // as the ranking — so the response is one round trip from the
    // frontend's perspective. If the sparkline query fails, degrade to
    // zero-filled sparklines but never cache the degraded body locally or
    // at the CDN.
    let spark_req: Vec<(i32, u8)> = rows.iter().map(|m| (m.item_id, m.hq)).collect();
    let (sparkline_rows, sparklines_degraded) = match ultros_clickhouse::queries::sparklines_folded(
        &ch, scope_id, &world_ids, &spark_req, 24,
    )
    .await
    {
        Ok(rows) => (rows, false),
        Err(e) => {
            tracing::warn!(
                error = ?e,
                scope_id,
                "sparklines_folded CH query failed, returning zero-filled sparklines without caching"
            );
            (Vec::new(), true)
        }
    };
    let mut spark_by_key: std::collections::HashMap<(i32, u8), Vec<u32>> = sparkline_rows
        .into_iter()
        .map(|s| ((s.item_id, s.hq), s.points))
        .collect();

    let items: Vec<MoverItem> = rows
        .into_iter()
        .map(|r| MoverItem {
            item_id: r.item_id,
            hq: r.hq != 0,
            world_id: r.world_id,
            price_now: r.price_now,
            pct_change_24h: r.pct_change_24h,
            volume_24h: r.volume_24h,
            gil_volume_24h: r.gil_volume_24h,
            sparkline: spark_by_key
                .remove(&(r.item_id, r.hq))
                .unwrap_or_else(|| vec![0; 24]),
        })
        .collect();

    let body = serde_json::to_string(&MoversResponse {
        world_id: scope_id,
        direction: direction_str,
        items,
    })
    .map_err(anyhow::Error::from)?;
    if sparklines_degraded {
        return Ok(crate::web::uncached_json(body));
    }
    cache.insert(cache_key, body.clone(), ttl);
    Ok(cached_json(body, ttl))
}

/// POST /api/v1/sparklines/{world} — bulk sparkline fetch by item id list.
pub(crate) async fn post_sparklines(
    State(ch): State<ClickHouseClient>,
    State(world_helper): State<Arc<WorldHelper>>,
    Path(world_name): Path<String>,
    Json(req): Json<SparklinesRequest>,
) -> Result<impl IntoResponse, WebError> {
    let world = world_helper
        .lookup_world_by_name(&world_name)
        .ok_or(WebError::NotFound)?;
    let world_id = match AnySelector::from(&world) {
        AnySelector::World(id) => id,
        _ => return Err(WebError::BadRequest),
    };
    if req.items.is_empty() {
        return Ok(Json(SparklinesResponse {
            world_id,
            series: vec![],
        })
        .into_response());
    }
    if req.items.len() > 200 {
        // The tuple-IN clause is fine up to ~thousands, but 200 covers
        // every reasonable page-of-rows use case and keeps the response
        // payload bounded.
        return Err(WebError::BadRequest);
    }
    let hours = req.hours.unwrap_or(24).clamp(6, 168);
    let scan_req: Vec<(i32, u8, i32)> = req
        .items
        .iter()
        .map(|(item, hq)| (*item, *hq as u8, world_id))
        .collect();

    let rows = ultros_clickhouse::queries::sparklines_batch(&ch, &scan_req, hours)
        .await
        .map_err(|e| {
            tracing::warn!(error = ?e, world_id, "sparklines_batch CH query failed");
            crate::web::error::ClickHouseQueryError::new("sparklines", e)
        })?;

    let series: Vec<SparklineSeries> = rows
        .into_iter()
        .map(|r| SparklineSeries {
            item_id: r.item_id,
            hq: r.hq != 0,
            world_id: r.world_id,
            points: r.points,
            first_price: r.first_price,
            last_price: r.last_price,
        })
        .collect();

    let mut response = Json(SparklinesResponse { world_id, series }).into_response();
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(60)));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::{MoversQuery, get_movers};
    use crate::web::home_feed_cache::HomeFeedCache;
    use axum::{
        extract::{Path, Query, State},
        http::{StatusCode, header},
        response::IntoResponse,
    };
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use ultros_api_types::{
        sparklines::MoversResponse,
        world::{Datacenter, Region, World, WorldData},
        world_helper::WorldHelper,
    };
    use ultros_clickhouse::ClickHouseClient;

    async fn read_http_request(socket: &mut tokio::net::TcpStream) {
        let mut buf = Vec::with_capacity(4096);
        let mut chunk = [0u8; 1024];
        let header_end = loop {
            let n = socket.read(&mut chunk).await.unwrap();
            if n == 0 {
                return;
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
    }

    async fn spawn_mock_clickhouse(
        responses: Vec<(u16, Vec<u8>)>,
    ) -> (ClickHouseClient, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            for (status, body) in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                read_http_request(&mut socket).await;
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

    fn encode_mover_row(item_id: i32, hq: u8, world_id: i32) -> Vec<u8> {
        let mut out = Vec::with_capacity(29);
        out.extend_from_slice(&item_id.to_le_bytes());
        out.push(hq);
        out.extend_from_slice(&world_id.to_le_bytes());
        out.extend_from_slice(&5000u32.to_le_bytes());
        out.extend_from_slice(&25.0f32.to_le_bytes());
        out.extend_from_slice(&10u32.to_le_bytes());
        out.extend_from_slice(&50_000u64.to_le_bytes());
        out
    }

    fn test_world_helper() -> Arc<WorldHelper> {
        Arc::new(WorldHelper::from(WorldData {
            regions: vec![Region {
                id: 1,
                name: "North-America".into(),
                datacenters: vec![Datacenter {
                    id: 10,
                    name: "Aether".into(),
                    region_id: 1,
                    worlds: vec![World {
                        id: 54,
                        name: "Sargatanas".into(),
                        datacenter_id: 10,
                    }],
                }],
            }],
        }))
    }

    #[tokio::test]
    async fn movers_returns_no_store_and_skips_local_cache_when_sparklines_fail() {
        let (ch, server) = spawn_mock_clickhouse(vec![
            (200, encode_mover_row(42, 0, 54)),
            (500, b"Code: 159. TIMEOUT_EXCEEDED".to_vec()),
        ])
        .await;
        let cache = HomeFeedCache::new(16);
        let response = get_movers(
            State(ch),
            State(test_world_helper()),
            State(cache.clone()),
            Path("Sargatanas".into()),
            Query(MoversQuery {
                direction: None,
                limit: None,
            }),
        )
        .await
        .expect("sparkline degradation returns 200")
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        assert!(
            cache.get("movers:Sargatanas:rising:10").is_none(),
            "degraded movers response must not be inserted into HomeFeedCache"
        );

        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: MoversResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.items.len(), 1);
        assert_eq!(body.items[0].item_id, 42);
        assert_eq!(body.items[0].sparkline, vec![0; 24]);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn movers_caches_healthy_response_locally_and_at_cdn() {
        let (ch, server) =
            spawn_mock_clickhouse(vec![(200, encode_mover_row(42, 0, 54)), (200, Vec::new())])
                .await;
        let cache = HomeFeedCache::new(16);
        let response = get_movers(
            State(ch),
            State(test_world_helper()),
            State(cache.clone()),
            Path("Sargatanas".into()),
            Query(MoversQuery {
                direction: None,
                limit: None,
            }),
        )
        .await
        .expect("healthy movers request returns 200")
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=60, stale-while-revalidate=240, stale-if-error=600"
        );
        assert!(
            cache.get("movers:Sargatanas:rising:10").is_some(),
            "healthy movers response should populate HomeFeedCache"
        );
        server.await.unwrap();
    }
}
