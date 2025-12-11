use std::{sync::Arc, time::Duration};

use aide::axum::IntoApiResponse;
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use ultros_api_types::recent_sales::RecentSales;
use ultros_db::world_cache::{AnySelector, WorldCache};

use crate::{analyzer_service::AnalyzerService, web::error::ApiError};

pub(crate) async fn recent_sales(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world): Path<String>,
) -> Result<impl IntoApiResponse, ApiError> {
    let sales = analyzer
        .read_sale_history(
            &AnySelector::from(&world_cache.lookup_value_by_name(&world)?),
            |sales| sales.clone(),
        )
        .await?;
    let mut response = Json(sales).into_response();
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(30)));
    Ok(response)
}
