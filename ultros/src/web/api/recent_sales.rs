use std::sync::Arc;

use axum::{
    extract::{Path, State},
    Json,
};
use ultros_api_types::recent_sales::{RecentSales, SaleData, Sales};
use ultros_db::world_cache::{AnySelector, WorldCache};

use crate::{analyzer_service::AnalyzerService, web::error::WebError};

pub(crate) async fn recent_sales(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world): Path<String>,
) -> Result<Json<RecentSales>, WebError> {
    let sales: Vec<_> = analyzer
        .read_sale_history(
            &AnySelector::from(&world_cache.lookup_value_by_name(&world)?),
            |sales| {
                sales
                    .item_map
                    .iter()
                    .map(|(key, sales)| SaleData {
                        item_id: key.item_id,
                        hq: key.hq,
                        sales: sales
                            .into_iter()
                            .map(|sale| Sales {
                                price_per_unit: sale.price_per_item,
                                sale_date: sale.sale_date,
                            })
                            .collect(),
                    })
                    .collect()
            },
        )
        .await?;
    Ok(Json(RecentSales { sales }))
}
