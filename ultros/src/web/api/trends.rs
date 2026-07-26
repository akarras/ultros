use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use tracing::instrument;
use ultros_api_types::trends::TrendsData;
use ultros_db::world_data::world_cache::{AnySelector, WorldCache};

use crate::{analyzer_service::AnalyzerService, web::error::WebError};

#[derive(Debug, Deserialize, Default)]
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
pub async fn get_trends(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world_name): Path<String>,
    Query(query): Query<TrendsQuery>,
) -> Result<Json<TrendsData>, WebError> {
    let selector = world_cache
        .lookup_value_by_name(&world_name)
        .map_err(|_| WebError::NotFound)?;
    let selector = AnySelector::from(&selector);

    // Currently only supporting trends for specific Worlds, as AnalyzerService::get_trends takes a world_id
    // If we want DC trends, we'd need to aggregate or the AnalyzerService needs to support it.
    // For now, if it's a datacenter, we error or pick a default?
    // Let's stick to World for now, or map DC to its worlds and aggregate?
    // Aggregating is expensive. Let's just enforce World for V1.

    let world_id = match selector {
        AnySelector::World(id) => id,
        // TODO: Implement Data Center aggregation.
        // This is computationally expensive to do on-the-fly. Consider pre-aggregating or
        // caching DC trends in the background worker.
        _ => return Err(WebError::BadRequest),
    };

    // V2 path: ?window= supplied → return a flat sorted list under
    // `items`. Clamp the window to the values the rollup actually
    // produces (7/30/90); anything else falls back to 30.
    if let Some(raw_window) = query.window {
        let window_days = match raw_window {
            7 | 30 | 90 => raw_window,
            _ => 30,
        };
        let include_suspicious = query.show_suspicious.unwrap_or(false);
        let items = analyzer
            .get_trends_v2(world_id, window_days, include_suspicious)
            .await
            .unwrap_or_default();
        return Ok(Json(TrendsData {
            items,
            high_velocity: vec![],
            rising_price: vec![],
            falling_price: vec![],
        }));
    }

    // Legacy v1 path — pre-bucketed lists, kept for any older client.
    let trends = analyzer.get_trends(world_id).await.unwrap_or(TrendsData {
        items: vec![],
        high_velocity: vec![],
        rising_price: vec![],
        falling_price: vec![],
    });

    Ok(Json(trends))
}

#[cfg(test)]
mod tests {
    use super::TrendsQuery;
    use axum::extract::Query;
    use axum::http::Uri;

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
}
