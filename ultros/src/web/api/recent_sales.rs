use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use serde::{Deserialize, Serialize};
use ultros_api_types::recent_sales::{RecentSales, RecentSalesColumnar, SaleData, Sales};
use ultros_db::world_data::world_cache::{AnySelector, WorldCache};

use super::is_columnar;
use crate::{analyzer_service::AnalyzerService, web::error::WebError};

#[derive(Debug, Deserialize)]
pub(crate) struct RecentSalesQuery {
    /// `columnar` selects [`RecentSalesColumnar`]; see [`is_columnar`].
    format: Option<String>,
}

/// The response body in either wire shape. Untagged so each variant
/// serializes as its own top-level object.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum Body {
    Rows(RecentSales),
    Columnar(RecentSalesColumnar),
}

fn body(sales: Vec<SaleData>, columnar: bool) -> Body {
    let rows = RecentSales { sales };
    if columnar {
        Body::Columnar(RecentSalesColumnar::from(rows))
    } else {
        Body::Rows(rows)
    }
}

pub(crate) async fn recent_sales(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world): Path<String>,
    Query(query): Query<RecentSalesQuery>,
) -> Result<impl IntoResponse, WebError> {
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
    let mut response: Response =
        Json(body(sales, is_columnar(query.format.as_deref()))).into_response();
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(30)));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_rows_by_default_columnar_on_request() {
        let rows = vec![SaleData {
            item_id: 5,
            hq: false,
            sales: vec![Sales {
                price_per_unit: 7,
                sale_date: chrono::DateTime::from_timestamp(1_699_999_000, 0)
                    .unwrap()
                    .naive_utc(),
            }],
        }];
        let legacy = serde_json::to_string(&body(rows.clone(), false)).unwrap();
        assert!(legacy.starts_with(r#"{"sales":[{"item_id":5"#));
        let columnar = serde_json::to_string(&body(rows, true)).unwrap();
        assert_eq!(
            columnar,
            r#"{"item_id":[5],"hq":[false],"count":[1],"price":[7],"sold_unix":[1699999000]}"#
        );
    }
}
