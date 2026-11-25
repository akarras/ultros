use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tracing::instrument;
use ultros_api_types::trends::TrendsData;
use ultros_db::world_data::world_cache::{AnySelector, WorldCache};

use crate::{
    analyzer_service::{AnalyzerQueryResult, AnalyzerService},
    market_client::MarketClient,
    web::error::WebError,
};

#[derive(Debug, Deserialize, Default, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TrendsQuery {
    /// One of 7, 30, or 90 — selects the v2 CH-backed window aggregate.
    /// When omitted the endpoint returns the legacy pre-bucketed payload
    /// (`high_velocity` / `rising_price` / `falling_price`) for backward
    /// compatibility with any existing API consumer.
    pub window: Option<u16>,
    /// `1` / `true` bypasses the cross-cutting `ResaleQualityFilter` so
    /// suspicious rows surface with a chip. Default false.
    #[serde(default, deserialize_with = "super::query::optional_flag")]
    pub show_suspicious: Option<bool>,
}

#[instrument(skip(analyzer, world_cache))]
pub(crate) async fn serve_trends(
    analyzer: &AnalyzerService,
    world_cache: &WorldCache,
    world_name: &str,
    query: &TrendsQuery,
) -> Result<Response, WebError> {
    let selector = world_cache
        .lookup_value_by_name(world_name)
        .map_err(|_| WebError::NotFound)?;
    let selector = AnySelector::from(&selector);

    let world_id = match selector {
        AnySelector::World(id) => id,
        _ => return Err(WebError::BadRequest),
    };

    // V2 path: ?window= supplied → return a flat sorted list under
    // `items`. Clamp the window to the values the rollup actually
    // produces (7/30/90); anything else falls back to 30.
    let result = if let Some(raw_window) = query.window {
        let window_days = match raw_window {
            7 | 30 | 90 => raw_window,
            _ => 30,
        };
        let include_suspicious = query.show_suspicious.unwrap_or(false);
        analyzer
            .get_trends_v2(world_id, window_days, include_suspicious)
            .await
            .map(|res| AnalyzerQueryResult {
                data: TrendsData {
                    items: res.data,
                    high_velocity: vec![],
                    rising_price: vec![],
                    falling_price: vec![],
                },
                degraded: res.degraded,
            })
    } else {
        // Legacy v1 path — pre-bucketed lists, kept for any older client.
        analyzer.get_trends(world_id).await
    };

    Ok(trends_response(result))
}

#[instrument(skip(market))]
pub async fn get_trends(
    State(market): State<Arc<dyn MarketClient>>,
    Path(world_name): Path<String>,
    Query(query): Query<TrendsQuery>,
) -> Result<Response, WebError> {
    market.trends(&world_name, &query).await
}

fn trends_response(result: Option<AnalyzerQueryResult<TrendsData>>) -> Response {
    let (trends, cacheable) = match result {
        Some(res) => (res.data, !res.degraded),
        None => (
            TrendsData {
                items: vec![],
                high_velocity: vec![],
                rising_price: vec![],
                falling_price: vec![],
            },
            false,
        ),
    };

    let cache_control = if cacheable {
        super::public_cache_control(60, 300, super::PUBLIC_STALE_IF_ERROR_SECS)
    } else {
        // Analyzer is warming up or ClickHouse enrichment failed; never cache
        // an empty or degraded fallback at the CDN.
        axum::http::HeaderValue::from_static("no-store")
    };

    let mut response = Json(trends).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, cache_control);
    response
}

#[cfg(test)]
mod tests {
    use super::{TrendsQuery, trends_response};
    use crate::analyzer_service::AnalyzerQueryResult;
    use axum::extract::Query;
    use axum::http::{Uri, header};
    use ultros_api_types::trends::TrendsData;

    fn extract(query: &str) -> Result<TrendsQuery, String> {
        let uri: Uri = format!("http://ultros.app/api/v1/trends/Sargatanas?{query}")
            .parse()
            .expect("test URI should parse");
        Query::<TrendsQuery>::try_from_uri(&uri)
            .map(|Query(q)| q)
            .map_err(|rejection| rejection.body_text())
    }

    /// The frontend sends `show_suspicious=0|1`, which `serde_urlencoded`
    /// rejects for a bare `Option<bool>` — and a `Query` rejection is a 400
    /// for the entire request, so the Trends page rendered nothing at all.
    #[test]
    fn numeric_show_suspicious_is_accepted() {
        let off = extract("window=30&show_suspicious=0").expect("`0` must extract");
        assert_eq!(off.window, Some(30));
        assert_eq!(off.show_suspicious, Some(false));

        let on = extract("window=30&show_suspicious=1").expect("`1` must extract");
        assert_eq!(on.show_suspicious, Some(true));
    }

    #[test]
    fn literal_show_suspicious_still_works() {
        assert_eq!(
            extract("window=7&show_suspicious=true")
                .expect("`true` must extract")
                .show_suspicious,
            Some(true)
        );
        assert_eq!(
            extract("window=90&show_suspicious=false")
                .expect("`false` must extract")
                .show_suspicious,
            Some(false)
        );
    }

    #[test]
    fn omitted_flag_defaults_to_none() {
        let q = extract("window=30").expect("omitted flag must extract");
        assert_eq!(q.show_suspicious, None);
        // The handler treats `None` as "filter suspicious rows out".
        assert!(!q.show_suspicious.unwrap_or(false));
    }

    #[test]
    fn nonsense_flag_is_still_rejected() {
        assert!(extract("window=30&show_suspicious=banana").is_err());
    }

    #[test]
    fn trends_response_sets_no_store_when_uninitialized_or_degraded() {
        let empty = TrendsData {
            items: vec![],
            high_velocity: vec![],
            rising_price: vec![],
            falling_price: vec![],
        };

        let uninitialized = trends_response(None);
        assert_eq!(
            uninitialized.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );

        let degraded = trends_response(Some(AnalyzerQueryResult {
            data: empty.clone(),
            degraded: true,
        }));
        assert_eq!(
            degraded.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );

        let healthy = trends_response(Some(AnalyzerQueryResult {
            data: empty,
            degraded: false,
        }));
        assert_eq!(
            healthy.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=60, stale-while-revalidate=300, stale-if-error=600"
        );
    }
}
