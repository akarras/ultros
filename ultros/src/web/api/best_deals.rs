use crate::{
    analyzer_service::{AnalyzerService, ResaleOptions, ResaleStats, SoldWithin},
    web::error::WebError,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use ultros_db::world_data::world_cache::WorldCache;

#[derive(Debug, Deserialize)]
pub(crate) struct BestDealsQuery {
    pub(crate) min_profit: Option<i32>,
    pub(crate) filter_sale: Option<String>, // "Day", "Week", etc.
    /// `1` or `true` skips the cross-cutting `ResaleQualityFilter` so
    /// suspicious rows (Unusable / high-launder) appear with a chip.
    /// Default false — these are the rows the Flip Finder used to surface
    /// gil-trader laundering on (e.g. Copper Wristlets 3 → 18.9M).
    #[serde(default, deserialize_with = "super::query::optional_flag")]
    pub(crate) show_suspicious: Option<bool>,
    /// Cap on returned rows. Default 50, clamped to [1, 200].
    pub(crate) limit: Option<u32>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct ResaleStatsDto {
    /// Post-tax gil: the estimated sale price net of the 5% marketboard cut,
    /// minus the buy price. Same math as the Flip Finder with its `include_tax`
    /// toggle in the default (on) position, so the home page's Top
    /// Opportunities card and the analyzer table agree on a given flip.
    pub(crate) profit: i32,
    pub(crate) item_id: i32,
    pub(crate) hq: bool,
    pub(crate) sold_within: String,
    /// `profit / buy_price * 100`, so it is post-tax too. Always finite:
    /// nonpositive-cost rows are dropped upstream and the value is clamped to
    /// ±100000 to match the frontend's display ceiling.
    pub(crate) return_on_investment: f32,
    /// Gil paid — the cheapest listing in the region. Sent explicitly so
    /// clients don't have to back-solve it out of `profit / roi`, which the
    /// ROI clamp makes wrong for very cheap buys.
    pub(crate) buy_price: i32,
    /// Gil to *list* at, before the marketboard's cut. Note that
    /// `buy_price + profit` is the post-tax take, which is a smaller number.
    pub(crate) est_sale_price: i32,
    pub(crate) world_id: i32,
    // Phase 2 deep-scan enrichment. All default to "unknown/zero" when the
    // ClickHouse rollup is missing — older API consumers ignore these
    // fields gracefully (Serde tolerates unknown fields by default).
    pub(crate) confidence_band: ultros_api_types::trends::ConfidenceBand,
    pub(crate) vwap_30d: i32,
    pub(crate) sample_size_30d: u32,
    pub(crate) launder_suspicion: f32,
}

impl From<ResaleStats> for ResaleStatsDto {
    fn from(stats: ResaleStats) -> Self {
        Self {
            profit: stats.profit,
            item_id: stats.item_id,
            hq: stats.hq,
            sold_within: stats.sold_within.to_string(),
            return_on_investment: stats.return_on_investment,
            buy_price: stats.buy_price,
            est_sale_price: stats.est_sale_price,
            world_id: stats.world_id,
            confidence_band: stats.confidence_band,
            vwap_30d: stats.vwap_30d,
            sample_size_30d: stats.sample_size_30d,
            launder_suspicion: stats.launder_suspicion,
        }
    }
}

pub(crate) async fn get_best_deals(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world_name): Path<String>,
    Query(query): Query<BestDealsQuery>,
) -> Result<Json<Vec<ResaleStatsDto>>, WebError> {
    let world = world_cache.lookup_value_by_name(&world_name)?;
    let world_id = world.as_world()?.id;
    let region = world_cache
        .get_region(&world)
        .ok_or_else(|| anyhow::anyhow!("Region not found for world {}", world_name))?;

    let filter_sale = match query.filter_sale.as_deref() {
        Some("Day") => Some(SoldWithin::Today(crate::analyzer_service::SoldAmount(1))),
        Some("Week") => Some(SoldWithin::Week(crate::analyzer_service::SoldAmount(1))),
        Some("Month") => Some(SoldWithin::Month(crate::analyzer_service::SoldAmount(1))),
        _ => None,
    };

    let options = ResaleOptions {
        minimum_profit: query.min_profit,
        filter_world: None,
        filter_datacenter: None,
        filter_sale,
        include_suspicious: query.show_suspicious.unwrap_or(false),
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 200) as usize;

    let stats = analyzer
        .get_best_resale(world_id, region.id, options, &world_cache)
        .await;

    let dtos = stats
        .unwrap_or_default()
        .into_iter()
        .take(limit)
        .map(ResaleStatsDto::from)
        .collect();

    Ok(Json(dtos))
}

#[cfg(test)]
mod tests {
    use super::BestDealsQuery;
    use axum::extract::Query;
    use axum::http::Uri;

    fn extract(query: &str) -> Result<BestDealsQuery, String> {
        let uri: Uri = format!("http://ultros.app/api/v1/best_deals/Sargatanas?{query}")
            .parse()
            .expect("test URI should parse");
        Query::<BestDealsQuery>::try_from_uri(&uri)
            .map(|Query(q)| q)
            .map_err(|rejection| rejection.body_text())
    }

    /// `TopOpportunity` always sends `show_suspicious=0` alongside the other
    /// params; before the flexible flag parser that 400'd the request and the
    /// component silently rendered nothing.
    #[test]
    fn numeric_show_suspicious_is_accepted_with_other_params() {
        let q = extract("min_profit=1000&filter_sale=Week&limit=5&show_suspicious=0")
            .expect("`0` must extract");
        assert_eq!(q.min_profit, Some(1000));
        assert_eq!(q.filter_sale.as_deref(), Some("Week"));
        assert_eq!(q.limit, Some(5));
        assert_eq!(q.show_suspicious, Some(false));
    }

    #[test]
    fn literal_show_suspicious_still_works() {
        assert_eq!(
            extract("show_suspicious=true")
                .expect("`true` must extract")
                .show_suspicious,
            Some(true)
        );
    }

    #[test]
    fn omitted_flag_defaults_to_none() {
        assert_eq!(
            extract("limit=10")
                .expect("omitted flag must extract")
                .show_suspicious,
            None
        );
    }
}
