use std::{sync::Arc, time::Duration};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use ultros_api_types::cheapest_listings::CheapestListingsColumnar;
use ultros_db::world_data::world_cache::{AnySelector, WorldCache};

use crate::{
    analyzer_service::{AnalyzerService, CheapestListings},
    web::{
        error::{SharedWebError, WebError},
        json_cache::{JsonCache, SwrDisposition, SwrLoad},
    },
};

/// Short-TTL pre-compressed SWR cache for `/api/v1/cheapest/{world}` bulk
/// market payloads, keyed by `(selector, is_columnar)`.
pub(crate) type CheapestCache = JsonCache<(AnySelector, bool)>;

pub(crate) fn new_cheapest_cache() -> CheapestCache {
    JsonCache::with_swr_budget(256, 64 * 1024 * 1024, 32)
}

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
    State(cache): State<CheapestCache>,
    headers: HeaderMap,
    Path(world): Path<String>,
    Query(format): Query<CheapestFormat>,
) -> Result<impl IntoResponse, SharedWebError> {
    let value = world_cache
        .lookup_value_by_name(&world)
        .map_err(WebError::from)?;
    let selector = AnySelector::from(&value);
    let columnar = format.is_columnar();

    // 5s fresh / 15s stale SWR window: absorbs bursty traffic for ~1.5 MB
    // region payloads without repeating `serde_json` or compression per
    // request while staying fresher than the 15s public `max-age`.
    let cached = cache
        .get_or_load_swr(
            (selector, columnar),
            Duration::from_secs(5),
            Duration::from_secs(15),
            move || async move {
                let bytes = if columnar {
                    let payload = analyzer
                        .read_cheapest_items(&selector, columnar_from_map)
                        .await?;
                    serde_json::to_vec(&payload).map_err(anyhow::Error::from)?
                } else {
                    let payload = analyzer
                        .read_cheapest_items(&selector, rows_from_map)
                        .await?;
                    serde_json::to_vec(&payload).map_err(anyhow::Error::from)?
                };
                Ok(SwrLoad {
                    body: Bytes::from(bytes),
                    cacheable: true,
                })
            },
        )
        .await?;

    metrics::counter!(
        "ultros_cheapest_cache_total",
        "disposition" => cached.disposition.as_str()
    )
    .increment(1);
    let is_stale = cached.disposition == SwrDisposition::Stale;
    let stored_at = cached.stored_at;
    let stale_until = cached.stale_until;
    Ok(cached
        .body
        .into_json_response_with_cache_control(
            &headers,
            move || {
                let now = std::time::Instant::now();
                super::public_cache_control_for_age(
                    Duration::from_secs(15),
                    Duration::from_secs(60),
                    super::PUBLIC_STALE_IF_ERROR_SECS,
                    is_stale,
                    now.saturating_duration_since(stored_at),
                    stale_until.saturating_duration_since(now),
                )
            },
            &[],
        )
        .await)
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
