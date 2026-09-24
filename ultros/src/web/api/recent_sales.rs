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
use crate::{
    analyzer_service::AnalyzerService,
    web::{error::WebError, home_feed_cache::HomeFeedCache},
};

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

/// How many newest items a datacenter/region response keeps. The ticker shows
/// eight; a per-world buffer is every recently traded item, but concatenating
/// that across a region's 30+ worlds would ship megabytes for eight rows.
const GROUP_RECENT_ITEMS: usize = 50;

pub(crate) async fn recent_sales(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    State(cache): State<HomeFeedCache>,
    Path(world): Path<String>,
    Query(query): Query<RecentSalesQuery>,
) -> Result<impl IntoResponse, WebError> {
    let scope = world_cache.lookup_value_by_name(&world)?;
    let columnar = is_columnar(query.format.as_deref());
    let ttl = Duration::from_secs(30);
    let sales: Vec<_> = match AnySelector::from(&scope) {
        selector @ AnySelector::World(_) => {
            analyzer
                .read_sale_history(&selector, |sales| {
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
                })
                .await?
        }
        // The analyzer buffers sales per world, so a datacenter or region is
        // assembled here: each world's newest sale per item, newest first,
        // one row per (item, hq), capped. Cached because the logged-out home
        // page sends every visitor in a region to the same scope.
        _ => {
            let cache_key = format!("recent_sales:{}:{columnar}", scope.get_name());
            if let Some(body) = cache.get(&cache_key) {
                return Ok(crate::web::cached_json(body, ttl));
            }
            let mut newest = Vec::new();
            for world_id in world_cache.get_all_worlds_in(&scope).unwrap_or_default() {
                let world_sales = analyzer
                    .read_sale_history(&AnySelector::World(world_id), |sales| {
                        sales
                            .item_map
                            .iter()
                            .filter_map(|(key, sales)| {
                                sales.first().map(|sale| (key.item_id, key.hq, *sale))
                            })
                            .collect::<Vec<_>>()
                    })
                    .await;
                // A world the analyzer never loaded is skipped rather than
                // failing the whole region; an uninitialized analyzer still
                // surfaces as the usual 503.
                match world_sales {
                    Ok(rows) => newest.extend(rows),
                    Err(crate::analyzer_service::AnalyzerError::NotFound) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            let sales = newest_per_item(newest, GROUP_RECENT_ITEMS);
            let body =
                serde_json::to_string(&body(sales, columnar)).map_err(anyhow::Error::from)?;
            cache.insert(cache_key, body.clone(), ttl);
            return Ok(crate::web::cached_json(body, ttl));
        }
    };
    let mut response: Response = Json(body(sales, columnar)).into_response();
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(ttl));
    Ok(response)
}

/// Newest-first, one row per `(item_id, hq)`, at most `limit` rows.
fn newest_per_item(
    mut rows: Vec<(i32, bool, crate::analyzer_service::SaleSummary)>,
    limit: usize,
) -> Vec<SaleData> {
    rows.sort_unstable_by_key(|(_, _, sale)| std::cmp::Reverse(sale.sale_date));
    let mut seen = std::collections::HashSet::new();
    rows.into_iter()
        .filter(|(item_id, hq, _)| seen.insert((*item_id, *hq)))
        .take(limit)
        .map(|(item_id, hq, sale)| SaleData {
            item_id,
            hq,
            sales: vec![Sales {
                price_per_unit: sale.price_per_item,
                sale_date: sale.sale_date,
            }],
        })
        .collect()
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

    fn sale(price: i32, unix: i64) -> crate::analyzer_service::SaleSummary {
        crate::analyzer_service::SaleSummary {
            price_per_item: price,
            sale_date: chrono::DateTime::from_timestamp(unix, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    #[test]
    fn group_rows_are_newest_first_one_per_item_and_capped() {
        // Item 1 sold on two worlds: only the newer sale survives.
        let rows = vec![
            (1, false, sale(100, 1_000)),
            (2, false, sale(200, 3_000)),
            (1, false, sale(150, 4_000)),
            (1, true, sale(900, 2_000)),
            (3, false, sale(300, 500)),
        ];
        let out = newest_per_item(rows, 3);
        let got: Vec<_> = out
            .iter()
            .map(|d| (d.item_id, d.hq, d.sales[0].price_per_unit))
            .collect();
        assert_eq!(got, vec![(1, false, 150), (2, false, 200), (1, true, 900)]);
    }
}
