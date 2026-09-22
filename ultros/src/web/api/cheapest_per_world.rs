use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use serde::{Deserialize, Serialize};
use ultros_api_types::cheapest_listings::CheapestListingsColumnar;
use ultros_db::world_data::world_cache::{AnySelector, WorldCache};

use crate::{
    analyzer_service::{AnalyzerService, CheapestListings},
    web::error::WebError,
};

#[derive(Serialize, Debug)]
struct CheapestListingData {
    item_id: i32,
    hq: bool,
    cheapest_price: i32,
    world_id: i32,
}

#[derive(Debug, Serialize)]
pub(crate) struct CheapestPerWorld {
    cheapest_listings: Vec<CheapestListingData>,
}

/// `?format=columnar` selects the struct-of-arrays shape
/// ([`CheapestListingsColumnar`]) the site fetches. Anything else — including
/// no `format` at all — keeps the legacy row-of-objects shape for outside
/// consumers of the public endpoint.
#[derive(Debug, Deserialize)]
pub(crate) struct CheapestFormat {
    pub(crate) format: Option<String>,
}

impl CheapestFormat {
    fn is_columnar(&self) -> bool {
        super::is_columnar(self.format.as_deref())
    }
}

/// Four equal-length columns in `item_map` iteration order — `(item_id, hq)`
/// ascending, since `ItemKey` is the `BTreeMap` key. Sorted ids are what let
/// the columns compress ~2x better than the row shape.
fn columnar_from_map(listings: &CheapestListings) -> CheapestListingsColumnar {
    let n = listings.item_map.len();
    let mut out = CheapestListingsColumnar {
        item_id: Vec::with_capacity(n),
        hq: Vec::with_capacity(n),
        price: Vec::with_capacity(n),
        world_id: Vec::with_capacity(n),
    };
    for (key, value) in listings.item_map.iter() {
        out.item_id.push(key.item_id);
        out.hq.push(key.hq);
        out.price.push(value.price);
        out.world_id.push(value.world_id);
    }
    out
}

fn rows_from_map(listings: &CheapestListings) -> CheapestPerWorld {
    CheapestPerWorld {
        cheapest_listings: listings
            .item_map
            .iter()
            .map(|(i, v)| CheapestListingData {
                item_id: i.item_id,
                hq: i.hq,
                cheapest_price: v.price,
                world_id: v.world_id,
            })
            .collect(),
    }
}

pub(crate) async fn cheapest_per_world(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(world): Path<String>,
    Query(format): Query<CheapestFormat>,
) -> Result<impl IntoResponse, WebError> {
    let value = world_cache.lookup_value_by_name(&world)?;
    let selector = AnySelector::from(&value);
    let mut response: Response = if format.is_columnar() {
        let columnar = analyzer
            .read_cheapest_items(&selector, columnar_from_map)
            .await?;
        Json(columnar).into_response()
    } else {
        let rows = analyzer
            .read_cheapest_items(&selector, rows_from_map)
            .await?;
        Json(rows).into_response()
    };
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(15)));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer_service::{CheapestListingValue, ItemKey};

    #[test]
    fn columnar_from_map_emits_equal_length_columns_in_key_order() {
        let mut listings = CheapestListings::default();
        for (item_id, hq, price, world_id) in
            [(5, false, 7, 34), (2, true, 300, 74), (2, false, 12, 73)]
        {
            listings.item_map.insert(
                ItemKey { item_id, hq },
                CheapestListingValue { price, world_id },
            );
        }
        let columnar = columnar_from_map(&listings);
        assert_eq!(columnar.item_id, vec![2, 2, 5]);
        assert_eq!(columnar.hq, vec![false, true, false]);
        assert_eq!(columnar.price, vec![12, 300, 7]);
        assert_eq!(columnar.world_id, vec![73, 74, 34]);
    }

    #[test]
    fn format_query_only_matches_columnar() {
        assert!(
            CheapestFormat {
                format: Some("columnar".into())
            }
            .is_columnar()
        );
        assert!(
            !CheapestFormat {
                format: Some("Columnar".into())
            }
            .is_columnar()
        );
        assert!(
            !CheapestFormat {
                format: Some("json".into())
            }
            .is_columnar()
        );
        assert!(!CheapestFormat { format: None }.is_columnar());
    }
}
