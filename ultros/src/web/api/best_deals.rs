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
use ultros_db::world_cache::WorldCache;

#[derive(Debug, Deserialize)]
pub(crate) struct BestDealsQuery {
    pub(crate) min_profit: Option<i32>,
    pub(crate) filter_sale: Option<String>, // "Day", "Week", etc.
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct ResaleStatsDto {
    pub(crate) profit: i32,
    pub(crate) item_id: i32,
    pub(crate) sold_within: String,
    pub(crate) return_on_investment: f32,
    pub(crate) world_id: i32,
    pub(crate) potential_profit_per_day: i32,
}

impl From<ResaleStats> for ResaleStatsDto {
    fn from(stats: ResaleStats) -> Self {
        Self {
            profit: stats.profit,
            item_id: stats.item_id,
            sold_within: stats.sold_within.to_string(),
            return_on_investment: stats.return_on_investment,
            world_id: stats.world_id,
            potential_profit_per_day: stats.potential_profit_per_day,
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
    };

    let stats = analyzer
        .get_best_resale(world_id, region.id, options, &world_cache)
        .await;

    let dtos = stats
        .unwrap_or_default()
        .into_iter()
        .take(50) // Limit to top 50 to avoid huge payloads
        .map(ResaleStatsDto::from)
        .collect();

    Ok(Json(dtos))
}
