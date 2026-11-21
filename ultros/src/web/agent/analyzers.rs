//! Enriched public market & gil-making query functions, HTTP handlers, and Markdown formatters.
//!
//! Each analyzer has one pure query function returning an `AgentEnvelope<T>` that is shared by:
//! 1. The public REST API (`/api/v1/public/*`)
//! 2. The remote MCP server (`/mcp`)
//! 3. `Accept: text/markdown` / `?format=md` content negotiation on web routes

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use chrono::Utc;
use futures::future::try_join3;
use serde::{Deserialize, Serialize};
use serde_json::json;
use ultros_api_types::recent_sales::{SaleData, Sales};
use ultros_api_types::trends::ConfidenceBand;
use ultros_api_types::world_helper::{AnyResult, WorldHelper};
use ultros_calc::analysis::{SalesStats, analyze_sales};
use ultros_calc::recipe_planner;
use ultros_crafting::cost::{
    CraftingCostOptions, EmptyOnHand, PriceSource, ShardsMode, compute_cost,
};
use ultros_db::world_data::world_cache::AnySelector as DbAnySelector;
use ultros_game_sources::{ScripType, scrip_turn_ins, venture_rewards};
use universalis::ItemId as UnivItemId;
use xiv_gen::{ClassJobCategoryId, ItemId, LeveId, RecipeLevelTableId};

use crate::analyzer_service::{AnalyzerService, ResaleOptions, SoldAmount, SoldWithin};
use crate::web::api::query::optional_flag;
use crate::web::error::WebError;
use crate::web::state::WebState;

use super::{
    AgentEnvelope, SITE_ORIGIN, craft_type_info, db_selector, is_shard_item, item_category_name,
    item_name, item_url, normalize_crafter_job, recipe_url, recipes_by_output, resolve_item_arg,
    snapshot_cheapest_map, tool_url, vendor_price_map, world_name_by_id,
};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 50;
const MARKET_TAX_RATE: f64 = 0.05;
const SUSPICIOUS_PRICE_MULTIPLE: i64 = 5;

fn clamp_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn json_cached<T: Serialize>(value: T, secs: u64) -> Response {
    let mut resp = Json(value).into_response();
    resp.headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(secs)));
    resp
}

fn post_tax_price(price: i32, include_tax: bool) -> i32 {
    if include_tax {
        ((price as f64) * (1.0 - MARKET_TAX_RATE)).floor() as i32
    } else {
        price
    }
}

fn tax_amount(price: i32, include_tax: bool) -> i32 {
    if include_tax {
        price - post_tax_price(price, true)
    } else {
        0
    }
}

/// Resolve a world name to a single world (`world_id`, `world_name`, `datacenter_id`, `region_id`).
struct ResolvedSingleWorld {
    world_id: i32,
    world_name: String,
    datacenter_id: i32,
    region_id: i32,
}

fn resolve_single_world(
    world_helper: &WorldHelper,
    world_name: &str,
) -> Result<ResolvedSingleWorld, WebError> {
    let found = world_helper
        .lookup_world_by_name(world_name)
        .ok_or(WebError::NotFound)?;
    let world = found.as_world().ok_or(WebError::BadRequest)?;
    let dcs = world_helper.get_datacenters(&found);
    let dc = dcs.first().copied().ok_or(WebError::NotFound)?;
    let region = world_helper.get_region(found);
    Ok(ResolvedSingleWorld {
        world_id: world.id,
        world_name: world.name.clone(),
        datacenter_id: dc.id,
        region_id: region.id,
    })
}

fn buy_scope_selector(world: &ResolvedSingleWorld, scope: &str) -> (DbAnySelector, &'static str) {
    match scope.trim().to_ascii_lowercase().as_str() {
        "world" => (DbAnySelector::World(world.world_id), "world"),
        "region" => (DbAnySelector::Region(world.region_id), "region"),
        _ => (DbAnySelector::Datacenter(world.datacenter_id), "datacenter"),
    }
}

async fn snapshot_world_sales_by_item(
    analyzer: &AnalyzerService,
    world_id: i32,
) -> Result<HashMap<(i32, bool), SaleData>, WebError> {
    let sales = analyzer
        .read_sale_history(&DbAnySelector::World(world_id), |history| {
            history
                .item_map
                .iter()
                .map(|(key, rows)| {
                    (
                        (key.item_id, key.hq),
                        SaleData {
                            item_id: key.item_id,
                            hq: key.hq,
                            sales: rows
                                .into_iter()
                                .map(|s| Sales {
                                    price_per_unit: s.price_per_item,
                                    sale_date: s.sale_date,
                                })
                                .collect(),
                        },
                    )
                })
                .collect::<HashMap<_, _>>()
        })
        .await?;
    Ok(sales)
}

fn median_sale_price(sale: &SaleData) -> Option<i32> {
    if sale.sales.is_empty() {
        return None;
    }
    let mut prices: Vec<i32> = sale.sales.iter().map(|s| s.price_per_unit).collect();
    prices.sort_unstable();
    Some(prices[prices.len() / 2])
}

fn summarize_item_sales(
    sales_map: &HashMap<(i32, bool), SaleData>,
    item_id: i32,
    hq: Option<bool>,
) -> (SalesStats, Option<i32>) {
    let mut refs: Vec<&SaleData> = Vec::new();
    match hq {
        Some(flag) => {
            if let Some(s) = sales_map.get(&(item_id, flag)) {
                refs.push(s);
            }
        }
        None => {
            if let Some(s) = sales_map.get(&(item_id, true)) {
                refs.push(s);
            }
            if let Some(s) = sales_map.get(&(item_id, false)) {
                refs.push(s);
            }
        }
    }
    if refs.is_empty() {
        return (
            SalesStats {
                daily_sales: 0.0,
                avg_price: 0,
                total_sales: 0,
            },
            None,
        );
    }
    let stats = analyze_sales(&refs, true);
    let mut all_prices: Vec<i32> = refs
        .iter()
        .flat_map(|s| s.sales.iter().map(|r| r.price_per_unit))
        .collect();
    all_prices.sort_unstable();
    let median = (!all_prices.is_empty()).then(|| all_prices[all_prices.len() / 2]);
    (stats, median)
}

// ============================================================================
// 1. Worlds
// ============================================================================

#[derive(Debug, Serialize)]
pub(crate) struct PublicDatacenter {
    pub(crate) id: i32,
    pub(crate) name: String,
    pub(crate) worlds: Vec<PublicWorld>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicWorld {
    pub(crate) id: i32,
    pub(crate) name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicRegion {
    pub(crate) id: i32,
    pub(crate) name: String,
    pub(crate) datacenters: Vec<PublicDatacenter>,
}

pub(crate) fn query_worlds(world_helper: &WorldHelper) -> AgentEnvelope<Vec<PublicRegion>> {
    let regions: Vec<PublicRegion> = world_helper
        .get_inner_data()
        .regions
        .iter()
        .map(|region| PublicRegion {
            id: region.id,
            name: region.name.clone(),
            datacenters: region
                .datacenters
                .iter()
                .map(|dc| PublicDatacenter {
                    id: dc.id,
                    name: dc.name.clone(),
                    worlds: dc
                        .worlds
                        .iter()
                        .map(|w| PublicWorld {
                            id: w.id,
                            name: w.name.clone(),
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect();
    let total = regions
        .iter()
        .flat_map(|r| &r.datacenters)
        .map(|d| d.worlds.len())
        .sum();
    AgentEnvelope::new(format!("{SITE_ORIGIN}/"), None, regions).with_total_matches(total)
}

pub(crate) async fn http_worlds(State(world_helper): State<Arc<WorldHelper>>) -> Response {
    metrics::counter!("ultros_agent_requests_total", "surface" => "rest", "tool" => "worlds")
        .increment(1);
    json_cached(query_worlds(&world_helper), 3600)
}

// ============================================================================
// 2. Search
// ============================================================================

#[derive(Debug, Deserialize)]
pub(crate) struct PublicSearchQuery {
    pub(crate) q: String,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicSearchItem {
    pub(crate) title: String,
    pub(crate) result_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) item_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) category: Option<String>,
    pub(crate) score: f32,
    pub(crate) url: String,
}

pub(crate) fn query_search(
    state: &WebState,
    q: &str,
    limit: Option<usize>,
) -> AgentEnvelope<Vec<PublicSearchItem>> {
    let limit = clamp_limit(limit.or(Some(15)));
    let raw = state.search_service.search(q);
    let total = raw.len();
    let results: Vec<PublicSearchItem> = raw
        .into_iter()
        .take(limit)
        .map(|r| {
            let item_id = if r.result_type.eq_ignore_ascii_case("item") {
                r.url
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .and_then(|s| s.parse::<i32>().ok())
            } else {
                None
            };
            let full_url = if r.url.starts_with("http://") || r.url.starts_with("https://") {
                r.url
            } else {
                format!("{SITE_ORIGIN}{}", r.url)
            };
            PublicSearchItem {
                title: r.title,
                result_type: r.result_type,
                item_id,
                category: r.category,
                score: r.score,
                url: full_url,
            }
        })
        .collect();
    let mut view_url = url::Url::parse(&format!("{SITE_ORIGIN}/items")).expect("valid url");
    if !q.trim().is_empty() {
        view_url.query_pairs_mut().append_pair("search", q.trim());
    }
    AgentEnvelope::new(view_url.to_string(), None, results)
        .with_filters(json!({ "q": q, "limit": limit }))
        .with_total_matches(total)
}

pub(crate) async fn http_search(
    State(state): State<WebState>,
    Query(query): Query<PublicSearchQuery>,
) -> Response {
    metrics::counter!("ultros_agent_requests_total", "surface" => "rest", "tool" => "search")
        .increment(1);
    json_cached(query_search(&state, &query.q, query.limit), 60)
}

// ============================================================================
// 3. Item Market Summary
// ============================================================================

#[derive(Debug, Serialize)]
pub(crate) struct PublicListingRow {
    pub(crate) price_per_unit: i32,
    pub(crate) quantity: i32,
    pub(crate) total: i64,
    pub(crate) hq: bool,
    pub(crate) world_name: String,
    pub(crate) retainer_name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicQualitySummary {
    pub(crate) cheapest_listing: Option<i32>,
    pub(crate) cheapest_world: Option<String>,
    pub(crate) listing_count: usize,
    pub(crate) recent_sales_count: usize,
    pub(crate) median_sale_price: Option<i32>,
    pub(crate) avg_sale_price: Option<i32>,
    pub(crate) daily_sales: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) vwap_30d: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) confidence_band: Option<ConfidenceBand>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) launder_suspicion: Option<f32>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicItemCraftSummary {
    pub(crate) recipe_id: i32,
    pub(crate) job: String,
    pub(crate) recipe_level: i8,
    pub(crate) yield_amount: i32,
    pub(crate) crafting_cost_per_unit: i32,
    pub(crate) recipe_url: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicItemMarket {
    pub(crate) item_id: i32,
    pub(crate) item_name: String,
    pub(crate) category: Option<String>,
    pub(crate) item_level: i32,
    pub(crate) can_be_hq: bool,
    pub(crate) vendor_price: Option<i32>,
    pub(crate) item_url: String,
    pub(crate) scope: String,
    pub(crate) nq: PublicQualitySummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) hq: Option<PublicQualitySummary>,
    pub(crate) top_listings: Vec<PublicListingRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) recipes: Vec<PublicItemCraftSummary>,
}

pub(crate) async fn query_item_market(
    state: &WebState,
    world_arg: &str,
    item_arg: &str,
) -> Result<AgentEnvelope<PublicItemMarket>, WebError> {
    let item_id = resolve_item_arg(item_arg, &state.search_service).ok_or(WebError::NotFound)?;
    let data = xiv_gen_db::data();
    let item = data.items.get(&ItemId(item_id)).ok_or(WebError::NotFound)?;

    let scope = state
        .world_helper
        .lookup_world_by_name(world_arg)
        .ok_or(WebError::NotFound)?;
    let scope_name = scope.get_name().to_string();
    let world_ids: Vec<i32> = scope.all_worlds().map(|w| w.id).collect();
    if world_ids.is_empty() {
        return Err(WebError::NotFound);
    }

    let db1 = state.db.clone();
    let db2 = state.db.clone();
    let db3 = state.db.clone();
    let (listings, sales, _last_updated) = try_join3(
        db1.get_all_listings_in_worlds_with_retainers(&world_ids, UnivItemId(item_id)),
        db2.get_sale_history_from_multiple_worlds(world_ids.iter().copied(), item_id, 100),
        db3.get_listing_last_updated_for_worlds(UnivItemId(item_id), &world_ids),
    )
    .await?;

    // Best-effort ClickHouse deep-scan stats (gracefully omitted if ClickHouse is unavailable).
    let ch_requests: Vec<(i32, u8, i32)> = world_ids
        .iter()
        .flat_map(|w| [(item_id, 0u8, *w), (item_id, 1u8, *w)])
        .collect();
    let ch_variants =
        ultros_clickhouse::queries::deep_scan_batch(&state.ch_client, 30, &ch_requests)
            .await
            .ok()
            .map(|rows| ultros_clickhouse::queries::aggregate_item_stats_variants(&rows))
            .unwrap_or_default();

    let mut listing_rows: Vec<PublicListingRow> = listings
        .into_iter()
        .filter_map(|(l, r)| {
            let retainer = r?;
            Some(PublicListingRow {
                price_per_unit: l.price_per_unit,
                quantity: l.quantity,
                total: l.price_per_unit as i64 * l.quantity as i64,
                hq: l.hq,
                world_name: world_name_by_id(&state.world_helper, l.world_id),
                retainer_name: retainer.name,
            })
        })
        .collect();
    listing_rows.sort_by_key(|r| r.price_per_unit);

    let build_quality = |is_hq: bool| -> PublicQualitySummary {
        let mut q_listings = listing_rows.iter().filter(|r| r.hq == is_hq);
        let first = q_listings.next();
        let listing_count = listing_rows.iter().filter(|r| r.hq == is_hq).count();

        let q_sales: Vec<Sales> = sales
            .iter()
            .filter(|s| s.0.hq == is_hq)
            .map(|s| Sales {
                price_per_unit: s.0.price_per_item,
                sale_date: s.0.sold_date,
            })
            .collect();
        let sale_data = SaleData {
            item_id,
            hq: is_hq,
            sales: q_sales,
        };
        let stats = if sale_data.sales.is_empty() {
            SalesStats {
                daily_sales: 0.0,
                avg_price: 0,
                total_sales: 0,
            }
        } else {
            analyze_sales(&[&sale_data], true)
        };
        let median = median_sale_price(&sale_data);
        let ch = ch_variants.iter().find(|v| v.hq == is_hq);

        PublicQualitySummary {
            cheapest_listing: first.map(|r| r.price_per_unit),
            cheapest_world: first.map(|r| r.world_name.clone()),
            listing_count,
            recent_sales_count: stats.total_sales,
            median_sale_price: median,
            avg_sale_price: (stats.total_sales > 0).then_some(stats.avg_price),
            daily_sales: (stats.daily_sales * 100.0).round() / 100.0,
            vwap_30d: ch.and_then(|v| {
                (v.vwap_30d > 0).then(|| i32::try_from(v.vwap_30d).unwrap_or(i32::MAX))
            }),
            confidence_band: ch.map(|v| v.confidence_band),
            launder_suspicion: ch.map(|v| (v.launder_suspicion * 1000.0).round() / 1000.0),
        }
    };

    let nq = build_quality(false);
    let hq = item.can_be_hq.then(|| build_quality(true));

    // Crafting cost summary if this item has recipes (uses current scope's cheapest map).
    let mut recipe_summaries = Vec::new();
    if let Some(item_recipes) = recipes_by_output().get(&ItemId(item_id)) {
        let buy_sel = match scope {
            AnyResult::World(_) => {
                let dcs = state.world_helper.get_datacenters(&scope);
                dcs.first()
                    .map(|dc| DbAnySelector::Datacenter(dc.id))
                    .unwrap_or_else(|| db_selector(&scope))
            }
            _ => db_selector(&scope),
        };
        if let Ok(cheapest_map) = snapshot_cheapest_map(&state.analyzer_service, &buy_sel).await {
            let on_hand = EmptyOnHand;
            let opts = CraftingCostOptions {
                require_hq: false,
                max_subcraft_depth: 2,
                shards: ShardsMode::ExcludeShards,
                on_hand: &on_hand,
                vendor_prices: Some(vendor_price_map()),
            };
            for recipe in item_recipes {
                let breakdown = compute_cost(
                    recipe,
                    &cheapest_map,
                    recipes_by_output(),
                    &opts,
                    &is_shard_item,
                );
                let unit = breakdown.per_unit(recipe.amount_result);
                let job = craft_type_info(recipe.craft_type)
                    .map(|(code, name)| format!("{code} ({name})"))
                    .unwrap_or_else(|| "Craft".to_string());
                let level = data
                    .recipe_level_tables
                    .get(&RecipeLevelTableId(recipe.recipe_level_table))
                    .map(|t| t.class_job_level)
                    .unwrap_or(1);
                recipe_summaries.push(PublicItemCraftSummary {
                    recipe_id: recipe.key_id.0,
                    job,
                    recipe_level: level,
                    yield_amount: recipe.amount_result.max(1),
                    crafting_cost_per_unit: unit.cost,
                    recipe_url: recipe_url(recipe.key_id.0),
                });
            }
        }
    }

    let url = item_url(&scope_name, item_id);
    let payload = PublicItemMarket {
        item_id,
        item_name: item.name.clone(),
        category: item_category_name(item).map(ToOwned::to_owned),
        item_level: item.level_item,
        can_be_hq: item.can_be_hq,
        vendor_price: vendor_price_map().get(&item_id).copied(),
        item_url: url.clone(),
        scope: scope_name.clone(),
        nq,
        hq,
        top_listings: listing_rows.into_iter().take(10).collect(),
        recipes: recipe_summaries,
    };

    Ok(AgentEnvelope::new(url, Some(scope_name), payload))
}

pub(crate) async fn http_item_market(
    State(state): State<WebState>,
    Path((world, item)): Path<(String, String)>,
) -> Result<Response, WebError> {
    metrics::counter!("ultros_agent_requests_total", "surface" => "rest", "tool" => "item")
        .increment(1);
    let env = query_item_market(&state, &world, &item).await?;
    Ok(json_cached(env, 30))
}

// ============================================================================
// 4. Flip Finder (Cross-World Arbitrage)
// ============================================================================

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PublicFlipsQuery {
    pub(crate) min_profit: Option<i32>,
    pub(crate) filter_sale: Option<String>,
    pub(crate) min_velocity: Option<f32>,
    pub(crate) min_buffer_sales: Option<u8>,
    pub(crate) max_roi: Option<f32>,
    #[serde(default, deserialize_with = "optional_flag")]
    pub(crate) show_suspicious: Option<bool>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicFlipRow {
    pub(crate) item_id: i32,
    pub(crate) item_name: String,
    pub(crate) category: Option<String>,
    pub(crate) hq: bool,
    pub(crate) buy_world: String,
    pub(crate) sell_world: String,
    pub(crate) buy_price: i32,
    pub(crate) est_sale_price: i32,
    pub(crate) profit: i32,
    pub(crate) roi_pct: f32,
    pub(crate) sold_within: String,
    pub(crate) velocity_per_day: Option<f32>,
    pub(crate) recent_sales_count: u8,
    pub(crate) confidence_band: ConfidenceBand,
    pub(crate) vwap_30d: i32,
    pub(crate) launder_suspicion: f32,
    pub(crate) item_url: String,
}

pub(crate) async fn query_flips(
    state: &WebState,
    world_arg: &str,
    q: PublicFlipsQuery,
) -> Result<AgentEnvelope<Vec<PublicFlipRow>>, WebError> {
    let resolved = resolve_single_world(&state.world_helper, world_arg)?;
    let filter_sale = match q
        .filter_sale
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("day") => Some(SoldWithin::Today(SoldAmount(1))),
        Some("week") => Some(SoldWithin::Week(SoldAmount(1))),
        Some("month") => Some(SoldWithin::Month(SoldAmount(1))),
        _ => None,
    };
    let show_suspicious = q.show_suspicious.unwrap_or(false);
    let limit = clamp_limit(q.limit);

    let options = ResaleOptions {
        minimum_profit: q.min_profit,
        filter_world: None,
        filter_datacenter: None,
        filter_sale,
        include_suspicious: show_suspicious,
        min_velocity_per_day: q.min_velocity,
        min_buffer_sales: q.min_buffer_sales,
        max_roi: q.max_roi,
    };

    let all_rows = state
        .analyzer_service
        .get_best_resale(
            resolved.world_id,
            resolved.region_id,
            options,
            &state.world_cache,
        )
        .await
        .unwrap_or_default();
    let total = all_rows.len();
    let data = xiv_gen_db::data();

    let rows: Vec<PublicFlipRow> = all_rows
        .into_iter()
        .take(limit)
        .map(|r| {
            let item = data.items.get(&ItemId(r.item_id));
            PublicFlipRow {
                item_id: r.item_id,
                item_name: item
                    .map(|i| i.name.clone())
                    .unwrap_or_else(|| item_name(r.item_id).to_string()),
                category: item.and_then(item_category_name).map(ToOwned::to_owned),
                hq: r.hq,
                buy_world: world_name_by_id(&state.world_helper, r.world_id),
                sell_world: resolved.world_name.clone(),
                buy_price: r.buy_price,
                est_sale_price: r.est_sale_price,
                profit: r.profit,
                roi_pct: (r.return_on_investment * 10.0).round() / 10.0,
                sold_within: r.sold_within.to_string(),
                velocity_per_day: r.velocity_per_day.map(|v| (v * 100.0).round() / 100.0),
                recent_sales_count: r.buffer_sale_count,
                confidence_band: r.confidence_band,
                vwap_30d: r.vwap_30d,
                launder_suspicion: (r.launder_suspicion * 1000.0).round() / 1000.0,
                item_url: item_url(&resolved.world_name, r.item_id),
            }
        })
        .collect();

    let mut params: Vec<(&str, String)> = vec![
        ("tax", "true".into()),
        ("show-suspicious", show_suspicious.to_string()),
        ("sort", "profit".into()),
        ("dir", "desc".into()),
    ];
    if let Some(min_p) = q.min_profit {
        params.push(("profit", min_p.to_string()));
    }
    if let Some(min_s) = q.min_buffer_sales {
        params.push(("sales", min_s.to_string()));
    }
    if let Some(vel) = q.min_velocity {
        params.push(("vel", vel.to_string()));
    }
    if let Some(ref fs) = q.filter_sale {
        let next = match fs.to_ascii_lowercase().as_str() {
            "day" => Some("1d"),
            "week" => Some("7d"),
            "month" => Some("1M"),
            _ => None,
        };
        if let Some(next_val) = next {
            params.push(("next-sale", next_val.into()));
        }
    }

    let view_url = tool_url("flip-finder", &resolved.world_name, &params);
    Ok(
        AgentEnvelope::new(view_url, Some(resolved.world_name), rows)
            .with_filters(json!({
                "min_profit": q.min_profit,
                "filter_sale": q.filter_sale,
                "min_velocity": q.min_velocity,
                "min_buffer_sales": q.min_buffer_sales,
                "max_roi": q.max_roi,
                "show_suspicious": show_suspicious,
                "limit": limit,
            }))
            .with_total_matches(total),
    )
}

pub(crate) async fn http_flips(
    State(state): State<WebState>,
    Path(world): Path<String>,
    Query(query): Query<PublicFlipsQuery>,
) -> Result<Response, WebError> {
    metrics::counter!("ultros_agent_requests_total", "surface" => "rest", "tool" => "flips")
        .increment(1);
    Ok(json_cached(query_flips(&state, &world, query).await?, 30))
}

// ============================================================================
// 5. Recipe Analyzer (Crafting Profit Calculator)
// ============================================================================

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PublicRecipesQuery {
    pub(crate) job: Option<String>,
    pub(crate) min_level: Option<i8>,
    pub(crate) max_level: Option<i8>,
    pub(crate) buy_scope: Option<String>,
    #[serde(default, deserialize_with = "optional_flag")]
    pub(crate) use_subcrafts: Option<bool>,
    #[serde(default, deserialize_with = "optional_flag")]
    pub(crate) require_hq: Option<bool>,
    #[serde(default, deserialize_with = "optional_flag")]
    pub(crate) exclude_shards: Option<bool>,
    pub(crate) min_profit: Option<i32>,
    pub(crate) min_roi: Option<i32>,
    pub(crate) min_sales: Option<usize>,
    pub(crate) min_daily_sales: Option<f32>,
    pub(crate) sort: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicIngredientCost {
    pub(crate) item_id: i32,
    pub(crate) item_name: String,
    pub(crate) quantity: i32,
    pub(crate) unit_price: i32,
    pub(crate) source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) world_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicRecipeRow {
    pub(crate) recipe_id: i32,
    pub(crate) item_id: i32,
    pub(crate) item_name: String,
    pub(crate) job: &'static str,
    pub(crate) job_name: &'static str,
    pub(crate) recipe_level: i8,
    pub(crate) yield_amount: i32,
    pub(crate) hq: bool,
    pub(crate) crafting_cost: i32,
    pub(crate) sell_price: i32,
    pub(crate) tax: i32,
    pub(crate) profit: i32,
    pub(crate) roi_pct: i32,
    pub(crate) daily_sales: f32,
    pub(crate) profit_per_day: i64,
    pub(crate) recent_sales_count: usize,
    pub(crate) ingredients: Vec<PublicIngredientCost>,
    pub(crate) item_url: String,
    pub(crate) recipe_url: String,
}

pub(crate) async fn query_recipes(
    state: &WebState,
    world_arg: &str,
    q: PublicRecipesQuery,
) -> Result<AgentEnvelope<Vec<PublicRecipeRow>>, WebError> {
    let resolved = resolve_single_world(&state.world_helper, world_arg)?;
    let job_filter = match q.job.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => Some(normalize_crafter_job(raw).ok_or(WebError::BadRequest)?),
        None => None,
    };
    let (buy_selector, buy_scope_label) =
        buy_scope_selector(&resolved, q.buy_scope.as_deref().unwrap_or("datacenter"));
    let use_subcrafts = q.use_subcrafts.unwrap_or(false);
    let require_hq = q.require_hq.unwrap_or(false);
    let exclude_shards = q.exclude_shards.unwrap_or(true);
    let limit = clamp_limit(q.limit);
    let sort_mode = q
        .sort
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "profit".to_string());

    let (sell_map, buy_map, sales_map) = try_join3(
        snapshot_cheapest_map(
            &state.analyzer_service,
            &DbAnySelector::World(resolved.world_id),
        ),
        snapshot_cheapest_map(&state.analyzer_service, &buy_selector),
        snapshot_world_sales_by_item(&state.analyzer_service, resolved.world_id),
    )
    .await?;

    let data = xiv_gen_db::data();
    let recipes_map = recipes_by_output();
    let vendors = vendor_price_map();
    let on_hand = EmptyOnHand;
    let opts = CraftingCostOptions {
        require_hq,
        max_subcraft_depth: if use_subcrafts { 3 } else { 0 },
        shards: if exclude_shards {
            ShardsMode::ExcludeShards
        } else {
            ShardsMode::IncludeMarket
        },
        on_hand: &on_hand,
        vendor_prices: Some(vendors),
    };

    let mut rows: Vec<PublicRecipeRow> = Vec::new();
    for recipe in data.recipes.values() {
        let Some((job_code, job_name)) = craft_type_info(recipe.craft_type) else {
            continue;
        };
        if let Some((wanted_code, _)) = job_filter
            && wanted_code != job_code
        {
            continue;
        }
        let recipe_level = data
            .recipe_level_tables
            .get(&RecipeLevelTableId(recipe.recipe_level_table))
            .map(|t| t.class_job_level)
            .unwrap_or(1);
        if let Some(min_l) = q.min_level
            && recipe_level < min_l
        {
            continue;
        }
        if let Some(max_l) = q.max_level
            && recipe_level > max_l
        {
            continue;
        }
        let Some(item) = data.items.get(&ItemId(recipe.item_result)) else {
            continue;
        };
        if item.item_search_category == 0 {
            continue;
        }

        // Sell price on home world: prefer HQ for items that can be HQ, otherwise NQ.
        let sell_summary = sell_map.find_matching_listings(recipe.item_result);
        let (sell_price, is_hq) = if item.can_be_hq {
            match (sell_summary.hq, sell_summary.lq) {
                (Some(hq), _) if hq.price > 0 => (hq.price, true),
                (_, Some(lq)) if lq.price > 0 => (lq.price, false),
                _ => continue,
            }
        } else if let Some(lq) = sell_summary.lq.filter(|l| l.price > 0) {
            (lq.price, false)
        } else {
            continue;
        };

        let (sales_stats, median_sale) =
            summarize_item_sales(&sales_map, recipe.item_result, Some(is_hq));
        // Skip troll/manipulated listings where the listing is >5x the median sale price.
        if let Some(median) = median_sale
            && median > 0
            && sales_stats.total_sales >= 2
            && (sell_price as i64) > (median as i64) * SUSPICIOUS_PRICE_MULTIPLE
        {
            continue;
        }

        if let Some(min_s) = q.min_sales
            && sales_stats.total_sales < min_s
        {
            continue;
        }
        if let Some(min_d) = q.min_daily_sales
            && sales_stats.daily_sales < min_d
        {
            continue;
        }

        let breakdown = compute_cost(recipe, &buy_map, recipes_map, &opts, &is_shard_item);
        // Drop recipes whose required market ingredients have no listings at all.
        if breakdown.unpriced_market_lines > 0 || breakdown.cost <= 0 {
            continue;
        }
        let unit = breakdown.per_unit(recipe.amount_result);
        if unit.cost <= 0 {
            continue;
        }

        let net_sell = post_tax_price(sell_price, true);
        let tax = tax_amount(sell_price, true);
        let profit = net_sell - unit.cost;
        let roi_pct = ((profit as f64 / unit.cost as f64) * 100.0).round() as i32;

        if let Some(min_p) = q.min_profit
            && profit < min_p
        {
            continue;
        }
        if let Some(min_r) = q.min_roi
            && roi_pct < min_r
        {
            continue;
        }

        let profit_per_day = if profit > 0 && sales_stats.daily_sales > 0.0 {
            (profit as f64 * sales_stats.daily_sales as f64).round() as i64
        } else {
            0
        };

        let ingredients = breakdown
            .ingredient_lines
            .iter()
            .filter(|l| !(exclude_shards && l.is_shard))
            .map(|l| PublicIngredientCost {
                item_id: l.item_id.0,
                item_name: item_name(l.item_id.0).to_string(),
                quantity: l.needed_total,
                unit_price: l.unit_price,
                source: match l.source {
                    PriceSource::Market => "market",
                    PriceSource::Vendor => "vendor",
                    PriceSource::Subcraft => "subcraft",
                },
                world_name: (l.world_id > 0)
                    .then(|| world_name_by_id(&state.world_helper, l.world_id)),
            })
            .collect();

        rows.push(PublicRecipeRow {
            recipe_id: recipe.key_id.0,
            item_id: recipe.item_result,
            item_name: item.name.clone(),
            job: job_code,
            job_name,
            recipe_level,
            yield_amount: recipe.amount_result.max(1),
            hq: is_hq,
            crafting_cost: unit.cost,
            sell_price,
            tax,
            profit,
            roi_pct,
            daily_sales: (sales_stats.daily_sales * 100.0).round() / 100.0,
            profit_per_day,
            recent_sales_count: sales_stats.total_sales,
            ingredients,
            item_url: item_url(&resolved.world_name, recipe.item_result),
            recipe_url: recipe_url(recipe.key_id.0),
        });
    }

    match sort_mode.as_str() {
        "roi" => rows.sort_by(|a, b| b.roi_pct.cmp(&a.roi_pct).then(b.profit.cmp(&a.profit))),
        "profit_per_day" | "ppd" => rows.sort_by(|a, b| {
            b.profit_per_day
                .cmp(&a.profit_per_day)
                .then(b.profit.cmp(&a.profit))
        }),
        "daily_sales" => rows.sort_by(|a, b| {
            b.daily_sales
                .total_cmp(&a.daily_sales)
                .then(b.profit.cmp(&a.profit))
        }),
        "cost" => rows.sort_by_key(|a| a.crafting_cost),
        "price" => rows.sort_by_key(|a| std::cmp::Reverse(a.sell_price)),
        _ => rows.sort_by(|a, b| b.profit.cmp(&a.profit).then(b.roi_pct.cmp(&a.roi_pct))),
    }

    let total = rows.len();
    rows.truncate(limit);

    let mut params: Vec<(&str, String)> = vec![
        ("buy-scope", buy_scope_label.into()),
        ("subcrafts", use_subcrafts.to_string()),
        ("require-hq", require_hq.to_string()),
        ("shards-exclude", exclude_shards.to_string()),
        (
            "sort",
            match sort_mode.as_str() {
                "roi" => "roi",
                "cost" => "cost",
                "price" => "price",
                _ => "profit",
            }
            .into(),
        ),
        ("dir", "desc".into()),
    ];
    if let Some((code, _)) = job_filter {
        params.push(("job", code.into()));
    }
    if let Some(min_p) = q.min_profit {
        params.push(("profit", min_p.to_string()));
    }
    if let Some(min_r) = q.min_roi {
        params.push(("roi", min_r.to_string()));
    }
    if let Some(min_s) = q.min_sales {
        params.push(("min-sales", min_s.to_string()));
    }

    let view_url = tool_url("recipe-analyzer", &resolved.world_name, &params);
    Ok(
        AgentEnvelope::new(view_url, Some(resolved.world_name), rows)
            .with_filters(json!({
                "job": job_filter.map(|(c, _)| c),
                "min_level": q.min_level,
                "max_level": q.max_level,
                "buy_scope": buy_scope_label,
                "use_subcrafts": use_subcrafts,
                "require_hq": require_hq,
                "exclude_shards": exclude_shards,
                "min_profit": q.min_profit,
                "min_roi": q.min_roi,
                "min_sales": q.min_sales,
                "min_daily_sales": q.min_daily_sales,
                "sort": sort_mode,
                "limit": limit,
            }))
            .with_total_matches(total),
    )
}

pub(crate) async fn http_recipes(
    State(state): State<WebState>,
    Path(world): Path<String>,
    Query(query): Query<PublicRecipesQuery>,
) -> Result<Response, WebError> {
    metrics::counter!("ultros_agent_requests_total", "surface" => "rest", "tool" => "recipes")
        .increment(1);
    Ok(json_cached(query_recipes(&state, &world, query).await?, 30))
}

// ============================================================================
// 6. Vendor Resale Analyzer (NPC Gil Shop -> Market Board)
// ============================================================================

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PublicVendorResaleQuery {
    pub(crate) min_profit: Option<i32>,
    pub(crate) min_roi: Option<i32>,
    pub(crate) min_sales: Option<usize>,
    #[serde(default, deserialize_with = "optional_flag")]
    pub(crate) tax: Option<bool>,
    #[serde(default, deserialize_with = "optional_flag")]
    pub(crate) show_suspicious: Option<bool>,
    pub(crate) sort: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicVendorResaleRow {
    pub(crate) item_id: i32,
    pub(crate) item_name: String,
    pub(crate) category: Option<String>,
    pub(crate) vendor_price: i32,
    pub(crate) market_price: i32,
    pub(crate) median_sale_price: Option<i32>,
    pub(crate) profit: i32,
    pub(crate) roi_pct: i32,
    pub(crate) daily_sales: f32,
    pub(crate) recent_sales_count: usize,
    pub(crate) item_url: String,
}

pub(crate) async fn query_vendor_resale(
    state: &WebState,
    world_arg: &str,
    q: PublicVendorResaleQuery,
) -> Result<AgentEnvelope<Vec<PublicVendorResaleRow>>, WebError> {
    let resolved = resolve_single_world(&state.world_helper, world_arg)?;
    let include_tax = q.tax.unwrap_or(true);
    let show_suspicious = q.show_suspicious.unwrap_or(false);
    let limit = clamp_limit(q.limit);
    let sort_mode = q
        .sort
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "profit".to_string());

    let (cheapest_map, sales_map) = futures::future::try_join(
        snapshot_cheapest_map(
            &state.analyzer_service,
            &DbAnySelector::World(resolved.world_id),
        ),
        snapshot_world_sales_by_item(&state.analyzer_service, resolved.world_id),
    )
    .await?;

    let data = xiv_gen_db::data();
    let mut rows = Vec::new();
    for (&item_id, &vendor_price) in vendor_price_map() {
        if vendor_price <= 0 {
            continue;
        }
        let Some(item) = data.items.get(&ItemId(item_id)) else {
            continue;
        };
        if item.item_search_category == 0 {
            continue;
        }
        let summary = cheapest_map.find_matching_listings(item_id);
        let Some(market_price) = summary.lowest_gil().filter(|&p| p > 0) else {
            continue;
        };
        let (sales_stats, median) = summarize_item_sales(&sales_map, item_id, Some(false));
        if !show_suspicious
            && let Some(med) = median
            && med > 0
            && sales_stats.total_sales > 0
            && (market_price as i64) > (med as i64) * SUSPICIOUS_PRICE_MULTIPLE
        {
            continue;
        }
        let net_sell = post_tax_price(market_price, include_tax);
        let profit = net_sell - vendor_price;
        if profit <= 0 {
            continue;
        }
        let roi_pct = ((profit as f64 / vendor_price as f64) * 100.0).round() as i32;

        if let Some(min_p) = q.min_profit
            && profit < min_p
        {
            continue;
        }
        if let Some(min_r) = q.min_roi
            && roi_pct < min_r
        {
            continue;
        }
        if let Some(min_s) = q.min_sales
            && sales_stats.total_sales < min_s
        {
            continue;
        }

        rows.push(PublicVendorResaleRow {
            item_id,
            item_name: item.name.clone(),
            category: item_category_name(item).map(ToOwned::to_owned),
            vendor_price,
            market_price,
            median_sale_price: median,
            profit,
            roi_pct,
            daily_sales: (sales_stats.daily_sales * 100.0).round() / 100.0,
            recent_sales_count: sales_stats.total_sales,
            item_url: item_url(&resolved.world_name, item_id),
        });
    }

    match sort_mode.as_str() {
        "roi" => rows.sort_by(|a, b| b.roi_pct.cmp(&a.roi_pct).then(b.profit.cmp(&a.profit))),
        "daily_sales" => rows.sort_by(|a, b| {
            b.daily_sales
                .total_cmp(&a.daily_sales)
                .then(b.profit.cmp(&a.profit))
        }),
        _ => rows.sort_by(|a, b| b.profit.cmp(&a.profit).then(b.roi_pct.cmp(&a.roi_pct))),
    }
    let total = rows.len();
    rows.truncate(limit);

    let mut params: Vec<(&str, String)> = vec![
        ("tax", include_tax.to_string()),
        ("show-suspicious", show_suspicious.to_string()),
        (
            "sort",
            if sort_mode == "roi" { "roi" } else { "profit" }.into(),
        ),
    ];
    if let Some(min_p) = q.min_profit {
        params.push(("profit", min_p.to_string()));
    }
    if let Some(min_r) = q.min_roi {
        params.push(("roi", min_r.to_string()));
    }
    if let Some(min_s) = q.min_sales {
        params.push(("sales", min_s.to_string()));
    }

    let view_url = tool_url("vendor-resale", &resolved.world_name, &params);
    Ok(
        AgentEnvelope::new(view_url, Some(resolved.world_name), rows)
            .with_filters(json!({
                "min_profit": q.min_profit,
                "min_roi": q.min_roi,
                "min_sales": q.min_sales,
                "tax": include_tax,
                "show_suspicious": show_suspicious,
                "sort": sort_mode,
                "limit": limit,
            }))
            .with_total_matches(total),
    )
}

pub(crate) async fn http_vendor_resale(
    State(state): State<WebState>,
    Path(world): Path<String>,
    Query(query): Query<PublicVendorResaleQuery>,
) -> Result<Response, WebError> {
    metrics::counter!("ultros_agent_requests_total", "surface" => "rest", "tool" => "vendor_resale")
        .increment(1);
    Ok(json_cached(
        query_vendor_resale(&state, &world, query).await?,
        30,
    ))
}

// ============================================================================
// 7. Leve Analyzer (Guildleve Turn-ins)
// ============================================================================

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PublicLevesQuery {
    pub(crate) job: Option<String>,
    pub(crate) min_level: Option<u16>,
    pub(crate) max_level: Option<u16>,
    #[serde(default, deserialize_with = "optional_flag")]
    pub(crate) hq: Option<bool>,
    pub(crate) min_profit: Option<i32>,
    pub(crate) buy_scope: Option<String>,
    pub(crate) sort: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicLeveRow {
    pub(crate) leve_id: i32,
    pub(crate) leve_name: String,
    pub(crate) job: String,
    pub(crate) level: u16,
    pub(crate) item_id: i32,
    pub(crate) item_name: String,
    pub(crate) quantity_required: u32,
    pub(crate) hq: bool,
    pub(crate) gil_reward: i32,
    pub(crate) market_buy_cost: Option<i32>,
    pub(crate) craft_cost: Option<i32>,
    pub(crate) best_cost: i32,
    pub(crate) best_method: &'static str,
    pub(crate) profit: i32,
    pub(crate) item_url: String,
}

pub(crate) async fn query_leves(
    state: &WebState,
    world_arg: &str,
    q: PublicLevesQuery,
) -> Result<AgentEnvelope<Vec<PublicLeveRow>>, WebError> {
    let resolved = resolve_single_world(&state.world_helper, world_arg)?;
    let job_filter = match q.job.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => Some(normalize_crafter_job(raw).ok_or(WebError::BadRequest)?),
        None => None,
    };
    let assume_hq = q.hq.unwrap_or(true);
    let (buy_selector, buy_scope_label) =
        buy_scope_selector(&resolved, q.buy_scope.as_deref().unwrap_or("world"));
    let limit = clamp_limit(q.limit);
    let sort_mode = q
        .sort
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "profit".to_string());

    let prices = snapshot_cheapest_map(&state.analyzer_service, &buy_selector).await?;
    let data = xiv_gen_db::data();
    let recipes_map = recipes_by_output();
    let on_hand = EmptyOnHand;
    let craft_opts = CraftingCostOptions {
        require_hq: false,
        max_subcraft_depth: 1,
        shards: ShardsMode::ExcludeShards,
        on_hand: &on_hand,
        vendor_prices: Some(vendor_price_map()),
    };

    let mut rows = Vec::new();
    for craft_leve in data.craft_leves.values() {
        if craft_leve.leve == 0 || craft_leve.item_0 == 0 || craft_leve.item_count_0 == 0 {
            continue;
        }
        let Some(leve) = data.leves.get(&LeveId(craft_leve.leve)) else {
            continue;
        };
        let level = u16::try_from(leve.class_job_level).unwrap_or(1);
        if let Some(min_l) = q.min_level
            && level < min_l
        {
            continue;
        }
        if let Some(max_l) = q.max_level
            && level > max_l
        {
            continue;
        }
        let job_name = data
            .class_job_categorys
            .get(&ClassJobCategoryId(leve.class_job_category))
            .map(|c| c.name.as_str())
            .unwrap_or("");
        if let Some((code, full)) = job_filter
            && !job_name.contains(full)
            && !job_name.contains(code)
        {
            continue;
        }

        let item_id = craft_leve.item_0;
        let total_qty = craft_leve.item_count_0 as u32;
        let base_gil = leve.gil_reward as i64;
        let gil_reward = (if assume_hq { base_gil * 2 } else { base_gil }) as i32;

        let summary = prices.find_matching_listings(item_id);
        let unit_buy = if assume_hq {
            summary.hq.map(|l| l.price)
        } else {
            summary.lowest_gil()
        };
        let market_buy_cost = unit_buy
            .filter(|&p| p > 0)
            .map(|p| (p as i64 * total_qty as i64).min(i32::MAX as i64) as i32);

        let craft_cost = recipes_map.get(&ItemId(item_id)).and_then(|recipes| {
            recipes
                .iter()
                .filter_map(|recipe| {
                    let b = compute_cost(recipe, &prices, recipes_map, &craft_opts, &is_shard_item);
                    if b.unpriced_market_lines > 0 || b.cost <= 0 {
                        return None;
                    }
                    let unit = b.per_unit(recipe.amount_result).cost;
                    (unit > 0).then(|| (unit as i64 * total_qty as i64).min(i32::MAX as i64) as i32)
                })
                .min()
        });

        let (best_cost, best_method) = match (market_buy_cost, craft_cost) {
            (Some(b), Some(c)) if c < b => (c, "craft"),
            (Some(b), _) => (b, "buy"),
            (None, Some(c)) => (c, "craft"),
            (None, None) => continue,
        };
        let profit = gil_reward - best_cost;
        if let Some(min_p) = q.min_profit
            && profit < min_p
        {
            continue;
        }

        rows.push(PublicLeveRow {
            leve_id: craft_leve.leve,
            leve_name: leve.name.clone(),
            job: job_name.to_string(),
            level,
            item_id,
            item_name: item_name(item_id).to_string(),
            quantity_required: total_qty,
            hq: assume_hq,
            gil_reward,
            market_buy_cost,
            craft_cost,
            best_cost,
            best_method,
            profit,
            item_url: item_url(&resolved.world_name, item_id),
        });
    }

    match sort_mode.as_str() {
        "level" => rows.sort_by(|a, b| b.level.cmp(&a.level).then(b.profit.cmp(&a.profit))),
        "cost" => rows.sort_by_key(|a| a.best_cost),
        _ => rows.sort_by_key(|a| std::cmp::Reverse(a.profit)),
    }
    let total = rows.len();
    rows.truncate(limit);

    let mut params: Vec<(&str, String)> = vec![(
        "sort",
        match sort_mode.as_str() {
            "level" => "level",
            "cost" => "cost",
            _ => "profit",
        }
        .into(),
    )];
    if let Some((_, full)) = job_filter {
        params.push(("job", full.into()));
    }
    if let Some(min_p) = q.min_profit {
        params.push(("profit", min_p.to_string()));
    }

    let view_url = tool_url("leve-analyzer", &resolved.world_name, &params);
    Ok(
        AgentEnvelope::new(view_url, Some(resolved.world_name), rows)
            .with_filters(json!({
                "job": job_filter.map(|(_, full)| full),
                "min_level": q.min_level,
                "max_level": q.max_level,
                "hq": assume_hq,
                "min_profit": q.min_profit,
                "buy_scope": buy_scope_label,
                "sort": sort_mode,
                "limit": limit,
            }))
            .with_total_matches(total),
    )
}

pub(crate) async fn http_leves(
    State(state): State<WebState>,
    Path(world): Path<String>,
    Query(query): Query<PublicLevesQuery>,
) -> Result<Response, WebError> {
    metrics::counter!("ultros_agent_requests_total", "surface" => "rest", "tool" => "leves")
        .increment(1);
    Ok(json_cached(query_leves(&state, &world, query).await?, 60))
}

// ============================================================================
// 8. Venture Analyzer (Retainer Ventures)
// ============================================================================

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PublicVenturesQuery {
    pub(crate) min_level: Option<u8>,
    pub(crate) max_level: Option<u8>,
    pub(crate) min_profit: Option<i32>,
    pub(crate) sort: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicVentureRow {
    pub(crate) task_id: i32,
    pub(crate) item_id: i32,
    pub(crate) item_name: String,
    pub(crate) job_category: String,
    pub(crate) level: u8,
    pub(crate) quantity: i32,
    pub(crate) unit_price: i32,
    pub(crate) total_value: i32,
    pub(crate) daily_sales: f32,
    pub(crate) recent_sales_count: usize,
    pub(crate) item_url: String,
}

pub(crate) async fn query_ventures(
    state: &WebState,
    world_arg: &str,
    q: PublicVenturesQuery,
) -> Result<AgentEnvelope<Vec<PublicVentureRow>>, WebError> {
    let resolved = resolve_single_world(&state.world_helper, world_arg)?;
    let limit = clamp_limit(q.limit);
    let sort_mode = q
        .sort
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "profit".to_string());

    let (cheapest_map, sales_map) = futures::future::try_join(
        snapshot_cheapest_map(
            &state.analyzer_service,
            &DbAnySelector::World(resolved.world_id),
        ),
        snapshot_world_sales_by_item(&state.analyzer_service, resolved.world_id),
    )
    .await?;

    let data = xiv_gen_db::data();
    let mut rows = Vec::new();
    for reward in venture_rewards(data) {
        if let Some(min_l) = q.min_level
            && reward.level < min_l
        {
            continue;
        }
        if let Some(max_l) = q.max_level
            && reward.level > max_l
        {
            continue;
        }
        let summary = cheapest_map.find_matching_listings(reward.item.0);
        let Some(unit_price) = summary.lowest_gil().filter(|&p| p > 0) else {
            continue;
        };
        let (sales_stats, median) = summarize_item_sales(&sales_map, reward.item.0, None);
        if let Some(med) = median
            && med > 0
            && sales_stats.total_sales > 0
            && (unit_price as i64) > (med as i64) * SUSPICIOUS_PRICE_MULTIPLE
        {
            continue;
        }
        let total_value = post_tax_price(
            (unit_price as i64 * reward.quantity as i64).min(i32::MAX as i64) as i32,
            true,
        );
        if let Some(min_p) = q.min_profit
            && total_value < min_p
        {
            continue;
        }
        let job_category = data
            .class_job_categorys
            .get(&reward.class_job_category)
            .map(|c| c.name.clone())
            .unwrap_or_default();

        rows.push(PublicVentureRow {
            task_id: reward.task.0,
            item_id: reward.item.0,
            item_name: item_name(reward.item.0).to_string(),
            job_category,
            level: reward.level,
            quantity: reward.quantity,
            unit_price,
            total_value,
            daily_sales: (sales_stats.daily_sales * 100.0).round() / 100.0,
            recent_sales_count: sales_stats.total_sales,
            item_url: item_url(&resolved.world_name, reward.item.0),
        });
    }

    match sort_mode.as_str() {
        "daily_sales" => rows.sort_by(|a, b| {
            b.daily_sales
                .total_cmp(&a.daily_sales)
                .then(b.total_value.cmp(&a.total_value))
        }),
        "level" => rows.sort_by(|a, b| {
            b.level
                .cmp(&a.level)
                .then(b.total_value.cmp(&a.total_value))
        }),
        _ => rows.sort_by_key(|a| std::cmp::Reverse(a.total_value)),
    }
    let total = rows.len();
    rows.truncate(limit);

    let mut params: Vec<(&str, String)> = vec![(
        "sort",
        match sort_mode.as_str() {
            "daily_sales" => "daily-sales",
            "level" => "level",
            _ => "profit",
        }
        .into(),
    )];
    if let Some(min_p) = q.min_profit {
        params.push(("profit", min_p.to_string()));
    }

    let view_url = tool_url("venture-analyzer", &resolved.world_name, &params);
    Ok(
        AgentEnvelope::new(view_url, Some(resolved.world_name), rows)
            .with_filters(json!({
                "min_level": q.min_level,
                "max_level": q.max_level,
                "min_profit": q.min_profit,
                "sort": sort_mode,
                "limit": limit,
            }))
            .with_total_matches(total),
    )
}

pub(crate) async fn http_ventures(
    State(state): State<WebState>,
    Path(world): Path<String>,
    Query(query): Query<PublicVenturesQuery>,
) -> Result<Response, WebError> {
    metrics::counter!("ultros_agent_requests_total", "surface" => "rest", "tool" => "ventures")
        .increment(1);
    Ok(json_cached(
        query_ventures(&state, &world, query).await?,
        60,
    ))
}

// ============================================================================
// 9. Scrip Sources Analyzer (Collectables Turn-ins)
// ============================================================================

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PublicScripsQuery {
    pub(crate) scrip: Option<String>,
    pub(crate) job: Option<String>,
    pub(crate) buy_scope: Option<String>,
    #[serde(default, deserialize_with = "optional_flag")]
    pub(crate) use_subcrafts: Option<bool>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicScripRow {
    pub(crate) item_id: i32,
    pub(crate) item_name: String,
    pub(crate) scrip_type: String,
    pub(crate) scrip_amount: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) job: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recipe_id: Option<i32>,
    pub(crate) craft_cost: i32,
    pub(crate) gil_per_scrip: f32,
    pub(crate) item_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recipe_url: Option<String>,
}

fn parse_scrip_filter(raw: &str) -> Option<ScripType> {
    if let Some(exact) = ScripType::from_filter_key(raw) {
        return Some(exact);
    }
    match raw.trim().to_ascii_lowercase().as_str() {
        "orange-crafters" | "orangecrafters" | "orange_crafters" => Some(ScripType::OrangeCrafters),
        "purple-crafters" | "purplecrafters" | "purple_crafters" => Some(ScripType::PurpleCrafters),
        "orange-gatherers" | "orangegatherers" | "orange_gatherers" => {
            Some(ScripType::OrangeGatherers)
        }
        "purple-gatherers" | "purplegatherers" | "purple_gatherers" => {
            Some(ScripType::PurpleGatherers)
        }
        _ => None,
    }
}

pub(crate) async fn query_scrips(
    state: &WebState,
    world_arg: &str,
    q: PublicScripsQuery,
) -> Result<AgentEnvelope<Vec<PublicScripRow>>, WebError> {
    let resolved = resolve_single_world(&state.world_helper, world_arg)?;
    let scrip_filter = match q.scrip.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => Some(parse_scrip_filter(raw).ok_or(WebError::BadRequest)?),
        None => None,
    };
    let job_filter = match q.job.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => Some(normalize_crafter_job(raw).ok_or(WebError::BadRequest)?),
        None => None,
    };
    let (buy_selector, buy_scope_label) =
        buy_scope_selector(&resolved, q.buy_scope.as_deref().unwrap_or("datacenter"));
    let use_subcrafts = q.use_subcrafts.unwrap_or(false);
    let limit = clamp_limit(q.limit);

    let buy_map = snapshot_cheapest_map(&state.analyzer_service, &buy_selector).await?;
    let data = xiv_gen_db::data();
    let recipes_map = recipes_by_output();
    let on_hand = EmptyOnHand;
    let opts = CraftingCostOptions {
        require_hq: false,
        max_subcraft_depth: if use_subcrafts { 2 } else { 0 },
        shards: ShardsMode::ExcludeShards,
        on_hand: &on_hand,
        vendor_prices: Some(vendor_price_map()),
    };

    let mut rows = Vec::new();
    for turn_in in scrip_turn_ins(data) {
        if turn_in.scrip_amount == 0 {
            continue;
        }
        if let Some(wanted) = scrip_filter
            && turn_in.scrip_type != wanted
        {
            continue;
        }
        let Some(scrip_key) = turn_in.scrip_type.filter_key() else {
            continue;
        };

        // Crafted collectables are priced via their recipe cost; gathered collectables have no craft cost.
        let Some(recipes) = recipes_map.get(&ItemId(turn_in.item_id)) else {
            if job_filter.is_some() {
                continue;
            }
            if turn_in.scrip_type.is_gatherer() {
                rows.push(PublicScripRow {
                    item_id: turn_in.item_id,
                    item_name: item_name(turn_in.item_id).to_string(),
                    scrip_type: scrip_key.to_string(),
                    scrip_amount: turn_in.scrip_amount,
                    job: None,
                    recipe_id: None,
                    craft_cost: 0,
                    gil_per_scrip: 0.0,
                    item_url: item_url(&resolved.world_name, turn_in.item_id),
                    recipe_url: None,
                });
            }
            continue;
        };

        for recipe in recipes {
            let job = craft_type_info(recipe.craft_type).map(|(c, _)| c);
            if let Some((wanted_code, _)) = job_filter
                && job != Some(wanted_code)
            {
                continue;
            }
            let breakdown = compute_cost(recipe, &buy_map, recipes_map, &opts, &is_shard_item);
            if breakdown.unpriced_market_lines > 0 || breakdown.cost <= 0 {
                continue;
            }
            let unit_cost = breakdown.per_unit(recipe.amount_result).cost;
            if unit_cost <= 0 {
                continue;
            }
            let gps = ((unit_cost as f64 / turn_in.scrip_amount as f64) * 100.0).round() / 100.0;
            rows.push(PublicScripRow {
                item_id: turn_in.item_id,
                item_name: item_name(turn_in.item_id).to_string(),
                scrip_type: scrip_key.to_string(),
                scrip_amount: turn_in.scrip_amount,
                job,
                recipe_id: Some(recipe.key_id.0),
                craft_cost: unit_cost,
                gil_per_scrip: gps as f32,
                item_url: item_url(&resolved.world_name, turn_in.item_id),
                recipe_url: Some(recipe_url(recipe.key_id.0)),
            });
        }
    }

    rows.sort_by(|a, b| {
        a.gil_per_scrip
            .total_cmp(&b.gil_per_scrip)
            .then(b.scrip_amount.cmp(&a.scrip_amount))
    });
    let total = rows.len();
    rows.truncate(limit);

    let mut params: Vec<(&str, String)> = Vec::new();
    if let Some(s) = scrip_filter.and_then(ScripType::filter_key) {
        params.push(("scrip", s.into()));
    }
    if let Some((code, _)) = job_filter {
        params.push(("job", code.into()));
    }
    let view_url = tool_url("scrip-sources", &resolved.world_name, &params);
    Ok(
        AgentEnvelope::new(view_url, Some(resolved.world_name), rows)
            .with_filters(json!({
                "scrip": scrip_filter.and_then(ScripType::filter_key),
                "job": job_filter.map(|(c, _)| c),
                "buy_scope": buy_scope_label,
                "use_subcrafts": use_subcrafts,
                "limit": limit,
            }))
            .with_total_matches(total),
    )
}

pub(crate) async fn http_scrips(
    State(state): State<WebState>,
    Path(world): Path<String>,
    Query(query): Query<PublicScripsQuery>,
) -> Result<Response, WebError> {
    metrics::counter!("ultros_agent_requests_total", "surface" => "rest", "tool" => "scrips")
        .increment(1);
    Ok(json_cached(query_scrips(&state, &world, query).await?, 60))
}

// ============================================================================
// 10. Market Trends
// ============================================================================

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PublicTrendsQuery {
    pub(crate) window: Option<u16>,
    #[serde(default, deserialize_with = "optional_flag")]
    pub(crate) show_suspicious: Option<bool>,
    pub(crate) sort: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicTrendRow {
    pub(crate) item_id: i32,
    pub(crate) item_name: String,
    pub(crate) category: Option<String>,
    pub(crate) hq: bool,
    pub(crate) current_price: i32,
    pub(crate) vwap_window: i32,
    pub(crate) pct_change_window: f32,
    pub(crate) sales_per_day: f32,
    pub(crate) unit_volume_window: u64,
    pub(crate) gil_volume_window: u64,
    pub(crate) confidence_band: ConfidenceBand,
    pub(crate) item_url: String,
}

pub(crate) async fn query_trends(
    state: &WebState,
    world_arg: &str,
    q: PublicTrendsQuery,
) -> Result<AgentEnvelope<Vec<PublicTrendRow>>, WebError> {
    let resolved = resolve_single_world(&state.world_helper, world_arg)?;
    let window_days = match q.window {
        Some(7) => 7,
        Some(90) => 90,
        _ => 30,
    };
    let show_suspicious = q.show_suspicious.unwrap_or(false);
    let limit = clamp_limit(q.limit);
    let sort_mode = q
        .sort
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "gil_volume".to_string());

    let mut items = state
        .analyzer_service
        .get_trends_v2(resolved.world_id, window_days, show_suspicious)
        .await
        .unwrap_or_default();

    // Fall back to v1 in-memory trends when ClickHouse rollup has no rows yet.
    if items.is_empty()
        && let Some(v1) = state.analyzer_service.get_trends(resolved.world_id).await
    {
        items = v1.high_velocity;
    }

    match sort_mode.as_str() {
        "sales_per_day" => items.sort_by(|a, b| b.sales_per_day.total_cmp(&a.sales_per_day)),
        "pct_change" => items.sort_by(|a, b| b.pct_change_window.total_cmp(&a.pct_change_window)),
        "unit_volume" => items.sort_by_key(|a| std::cmp::Reverse(a.unit_volume_window)),
        _ => items.sort_by_key(|a| std::cmp::Reverse(a.gil_volume_window)),
    }

    let total = items.len();
    let data = xiv_gen_db::data();
    let rows: Vec<PublicTrendRow> = items
        .into_iter()
        .take(limit)
        .map(|t| {
            let item = data.items.get(&ItemId(t.item_id));
            PublicTrendRow {
                item_id: t.item_id,
                item_name: item
                    .map(|i| i.name.clone())
                    .unwrap_or_else(|| item_name(t.item_id).to_string()),
                category: item.and_then(item_category_name).map(ToOwned::to_owned),
                hq: t.hq,
                current_price: t.price,
                vwap_window: if t.vwap_window > 0 {
                    t.vwap_window
                } else {
                    t.vwap_30d
                },
                pct_change_window: (t.pct_change_window * 10.0).round() / 10.0,
                sales_per_day: (t.sales_per_day * 100.0).round() / 100.0,
                unit_volume_window: t.unit_volume_window,
                gil_volume_window: t.gil_volume_window,
                confidence_band: t.confidence_band,
                item_url: item_url(&resolved.world_name, t.item_id),
            }
        })
        .collect();

    let view_url = tool_url("trends", &resolved.world_name, &[]);
    Ok(
        AgentEnvelope::new(view_url, Some(resolved.world_name), rows)
            .with_filters(json!({
                "window": window_days,
                "show_suspicious": show_suspicious,
                "sort": sort_mode,
                "limit": limit,
            }))
            .with_total_matches(total),
    )
}

pub(crate) async fn http_trends(
    State(state): State<WebState>,
    Path(world): Path<String>,
    Query(query): Query<PublicTrendsQuery>,
) -> Result<Response, WebError> {
    metrics::counter!("ultros_agent_requests_total", "surface" => "rest", "tool" => "trends")
        .increment(1);
    Ok(json_cached(query_trends(&state, &world, query).await?, 60))
}

// ============================================================================
// 11. Craft Shopping List Planner (`ultros_calc::recipe_planner::expand`)
// ============================================================================

#[derive(Debug, Deserialize)]
pub(crate) struct PlanCraftTarget {
    pub(crate) item: String,
    pub(crate) quantity: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PlanCraftRequest {
    pub(crate) world: String,
    pub(crate) buy_scope: Option<String>,
    pub(crate) use_subcrafts: Option<bool>,
    pub(crate) exclude_shards: Option<bool>,
    pub(crate) items: Vec<PlanCraftTarget>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PlannedShoppingItem {
    pub(crate) item_id: i32,
    pub(crate) item_name: String,
    pub(crate) quantity: i64,
    pub(crate) source: &'static str,
    pub(crate) est_unit_price: i32,
    pub(crate) est_total_cost: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cheapest_world: Option<String>,
    pub(crate) item_url: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct PlannedCraftStep {
    pub(crate) item_id: i32,
    pub(crate) item_name: String,
    pub(crate) recipe_id: i32,
    pub(crate) job: String,
    pub(crate) crafts_needed: i64,
    pub(crate) total_needed: i64,
    pub(crate) surplus: i64,
    pub(crate) recipe_url: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct PlanCraftResponse {
    pub(crate) buy_scope: String,
    pub(crate) estimated_total_cost: i64,
    pub(crate) missing_price_items: usize,
    pub(crate) shopping_list: Vec<PlannedShoppingItem>,
    pub(crate) craft_steps: Vec<PlannedCraftStep>,
}

pub(crate) async fn query_plan_craft(
    state: &WebState,
    req: PlanCraftRequest,
) -> Result<AgentEnvelope<PlanCraftResponse>, WebError> {
    if req.items.is_empty() || req.items.len() > 20 {
        return Err(WebError::BadRequest);
    }
    let resolved = resolve_single_world(&state.world_helper, &req.world)?;
    let (buy_selector, buy_scope_label) =
        buy_scope_selector(&resolved, req.buy_scope.as_deref().unwrap_or("datacenter"));
    let use_subcrafts = req.use_subcrafts.unwrap_or(true);
    let exclude_shards = req.exclude_shards.unwrap_or(true);

    let buy_map = snapshot_cheapest_map(&state.analyzer_service, &buy_selector).await?;
    let data = xiv_gen_db::data();
    let recipes_by_out = recipes_by_output();
    let vendors = vendor_price_map();

    // Build `ultros_calc::recipe_planner` recipe table for all recipes.
    let mut planner_recipes: BTreeMap<i32, recipe_planner::Recipe> = BTreeMap::new();
    let mut default_choices: BTreeMap<i32, i32> = BTreeMap::new();
    for recipe in data.recipes.values() {
        if recipe.item_result <= 0 {
            continue;
        }
        let ingredients: Vec<(i32, i64)> = recipe
            .ingredient
            .iter()
            .zip(recipe.amount_ingredient.iter())
            .filter_map(|(&id, &amt)| (id > 0 && amt > 0).then_some((id, amt as i64)))
            .collect();
        planner_recipes.insert(
            recipe.key_id.0,
            recipe_planner::Recipe {
                id: recipe.key_id.0,
                output: recipe.item_result,
                yield_amount: recipe.amount_result.max(1) as i64,
                ingredients,
            },
        );
        if use_subcrafts {
            default_choices
                .entry(recipe.item_result)
                .or_insert(recipe.key_id.0);
        }
    }

    let excluded: BTreeSet<i32> = if exclude_shards {
        data.items
            .iter()
            .filter(|(_, item)| {
                item.item_search_category == ultros_crafting::cost::CRYSTAL_SEARCH_CATEGORY
            })
            .map(|(id, _)| id.0)
            .collect()
    } else {
        BTreeSet::new()
    };

    let mut aggregated_buy: BTreeMap<i32, i64> = BTreeMap::new();
    let mut aggregated_crafts: BTreeMap<i32, (i32, i64, i64, i64, usize)> = BTreeMap::new();
    let owned = BTreeMap::new();
    let mut first_recipe_id: Option<i32> = None;

    for target in &req.items {
        let item_id =
            resolve_item_arg(&target.item, &state.search_service).ok_or(WebError::NotFound)?;
        let qty = target.quantity.clamp(1, 9999);
        let root_recipe = recipes_by_out
            .get(&ItemId(item_id))
            .and_then(|v| v.first().copied())
            .ok_or(WebError::BadRequest)?;
        if first_recipe_id.is_none() {
            first_recipe_id = Some(root_recipe.key_id.0);
        }
        let planner_root = planner_recipes
            .get(&root_recipe.key_id.0)
            .ok_or(WebError::BadRequest)?;
        let choices = if use_subcrafts {
            &default_choices
        } else {
            // Only expand the requested root item; buy all direct ingredients.
            &BTreeMap::new()
        };
        let materials = recipe_planner::expand(
            planner_root,
            qty,
            &planner_recipes,
            choices,
            &owned,
            &excluded,
        )
        .map_err(|_| WebError::BadRequest)?;

        for mat in materials {
            if let Some(rid) = mat.recipe {
                let entry = aggregated_crafts
                    .entry(mat.item)
                    .or_insert((rid, 0, 0, 0, mat.depth));
                entry.1 += mat.crafts;
                entry.2 += mat.needed;
                entry.3 += mat.surplus;
                entry.4 = entry.4.max(mat.depth);
            } else {
                *aggregated_buy.entry(mat.item).or_insert(0) += mat.remaining();
            }
        }
    }

    let mut total_cost: i64 = 0;
    let mut missing_price_items: usize = 0;
    let mut shopping_list = Vec::new();
    for (item_id, quantity) in aggregated_buy {
        if quantity <= 0 {
            continue;
        }
        let vendor = vendors.get(&item_id).copied();
        let market = buy_map.find_matching_listings(item_id).chosen(false);
        let (source, unit_price, world_name) = match (vendor, market) {
            (Some(v), Some(m)) if v <= m.price => ("vendor", v, None),
            (Some(v), None) => ("vendor", v, None),
            (_, Some(m)) if m.price > 0 => (
                "market",
                m.price,
                Some(world_name_by_id(&state.world_helper, m.world_id)),
            ),
            _ => {
                missing_price_items += 1;
                ("unpriced", 0, None)
            }
        };
        let est_total = unit_price as i64 * quantity;
        total_cost += est_total;
        shopping_list.push(PlannedShoppingItem {
            item_id,
            item_name: item_name(item_id).to_string(),
            quantity,
            source,
            est_unit_price: unit_price,
            est_total_cost: est_total,
            cheapest_world: world_name,
            item_url: item_url(&resolved.world_name, item_id),
        });
    }
    shopping_list.sort_by_key(|a| std::cmp::Reverse(a.est_total_cost));

    let mut craft_steps_raw: Vec<(usize, PlannedCraftStep)> = aggregated_crafts
        .into_iter()
        .map(|(item_id, (rid, crafts, needed, surplus, depth))| {
            let job = data
                .recipes
                .get(&xiv_gen::RecipeId(rid))
                .and_then(|r| craft_type_info(r.craft_type))
                .map(|(c, n)| format!("{c} ({n})"))
                .unwrap_or_else(|| "Craft".to_string());
            (
                depth,
                PlannedCraftStep {
                    item_id,
                    item_name: item_name(item_id).to_string(),
                    recipe_id: rid,
                    job,
                    crafts_needed: crafts,
                    total_needed: needed,
                    surplus,
                    recipe_url: recipe_url(rid),
                },
            )
        })
        .collect();
    // Deepest subcrafts first so the player crafts in bottom-up order.
    craft_steps_raw.sort_by_key(|a| std::cmp::Reverse(a.0));
    let craft_steps: Vec<PlannedCraftStep> = craft_steps_raw.into_iter().map(|(_, s)| s).collect();

    let view_url = first_recipe_id
        .map(recipe_url)
        .unwrap_or_else(|| tool_url("recipe-analyzer", &resolved.world_name, &[]));

    Ok(AgentEnvelope::new(
        view_url,
        Some(resolved.world_name),
        PlanCraftResponse {
            buy_scope: buy_scope_label.to_string(),
            estimated_total_cost: total_cost,
            missing_price_items,
            shopping_list,
            craft_steps,
        },
    ))
}

pub(crate) async fn http_plan_craft(
    State(state): State<WebState>,
    Json(req): Json<PlanCraftRequest>,
) -> Result<Response, WebError> {
    metrics::counter!("ultros_agent_requests_total", "surface" => "rest", "tool" => "plan_craft")
        .increment(1);
    Ok(json_cached(query_plan_craft(&state, req).await?, 15))
}

// ============================================================================
// Markdown Formatters (shared by `Accept: text/markdown` and MCP tool outputs)
// ============================================================================

fn md_header(out: &mut String, title: &str, view_url: &str, as_of: &chrono::DateTime<Utc>) {
    let _ = writeln!(out, "# {title}\n");
    let _ = writeln!(
        out,
        "> Source: **Ultros** ({view_url}) · Snapshot: {}",
        as_of.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    );
    let _ = writeln!(
        out,
        "> Open the Ultros link above for live WebSocket prices, charts, in-game copy tools, and full filters.\n"
    );
}

pub(crate) fn format_item_market_md(env: &AgentEnvelope<PublicItemMarket>) -> String {
    let d = &env.data;
    let mut out = String::new();
    md_header(
        &mut out,
        &format!("{} — {} Market", d.item_name, d.scope),
        &env.view_url,
        &env.as_of,
    );
    let _ = writeln!(
        out,
        "- **Item ID**: {} | **Category**: {} | **iLvl**: {} | **Can be HQ**: {}",
        d.item_id,
        d.category.as_deref().unwrap_or("—"),
        d.item_level,
        if d.can_be_hq { "Yes" } else { "No" }
    );
    if let Some(vp) = d.vendor_price {
        let _ = writeln!(out, "- **NPC Vendor Price**: {vp} gil");
    }
    let _ = writeln!(out, "\n## Price Summary\n");
    let _ = writeln!(
        out,
        "| Quality | Cheapest Listing | Cheapest World | Median Sale | Sales/Day | 30d VWAP | Confidence |"
    );
    let _ = writeln!(out, "|---|---:|---|---:|---:|---:|---|");
    for (label, q) in [("NQ", Some(&d.nq)), ("HQ", d.hq.as_ref())] {
        let Some(q) = q else { continue };
        let _ = writeln!(
            out,
            "| {label} | {} | {} | {} | {:.2} | {} | {:?} |",
            q.cheapest_listing
                .map(|p| format!("{p} gil"))
                .unwrap_or_else(|| "—".into()),
            q.cheapest_world.as_deref().unwrap_or("—"),
            q.median_sale_price
                .map(|p| format!("{p} gil"))
                .unwrap_or_else(|| "—".into()),
            q.daily_sales,
            q.vwap_30d
                .map(|p| format!("{p} gil"))
                .unwrap_or_else(|| "—".into()),
            q.confidence_band.unwrap_or(ConfidenceBand::Unknown),
        );
    }
    if !d.recipes.is_empty() {
        let _ = writeln!(out, "\n## Crafting Cost\n");
        for r in &d.recipes {
            let _ = writeln!(
                out,
                "- **{}** (Lv {}, yields {}): ~{} gil/unit — [Recipe]({})",
                r.job, r.recipe_level, r.yield_amount, r.crafting_cost_per_unit, r.recipe_url
            );
        }
    }
    if !d.top_listings.is_empty() {
        let _ = writeln!(out, "\n## Cheapest Active Listings\n");
        let _ = writeln!(out, "| Quality | Price | Qty | Total | World |");
        let _ = writeln!(out, "|---|---:|---:|---:|---|");
        for l in &d.top_listings {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} |",
                if l.hq { "HQ" } else { "NQ" },
                l.price_per_unit,
                l.quantity,
                l.total,
                l.world_name
            );
        }
    }
    out
}

pub(crate) fn format_flips_md(env: &AgentEnvelope<Vec<PublicFlipRow>>) -> String {
    let mut out = String::new();
    let world = env.world.as_deref().unwrap_or("World");
    md_header(
        &mut out,
        &format!("Flip Finder — {world}"),
        &env.view_url,
        &env.as_of,
    );
    let _ = writeln!(
        out,
        "Showing {} of {} matching flips.\n",
        env.data.len(),
        env.total_matches.unwrap_or(env.data.len())
    );
    let _ = writeln!(
        out,
        "| Item | Quality | Buy World | Buy | Est. Sell | Profit (post-tax) | ROI % | Sales/Day | Confidence |"
    );
    let _ = writeln!(out, "|---|---|---|---:|---:|---:|---:|---:|---|");
    for r in &env.data {
        let _ = writeln!(
            out,
            "| [{}]({}) | {} | {} | {} | {} | {} | {:.1}% | {} | {:?} |",
            r.item_name,
            r.item_url,
            if r.hq { "HQ" } else { "NQ" },
            r.buy_world,
            r.buy_price,
            r.est_sale_price,
            r.profit,
            r.roi_pct,
            r.velocity_per_day
                .map(|v| format!("{v:.2}"))
                .unwrap_or_else(|| "—".into()),
            r.confidence_band,
        );
    }
    out
}

pub(crate) fn format_recipes_md(env: &AgentEnvelope<Vec<PublicRecipeRow>>) -> String {
    let mut out = String::new();
    let world = env.world.as_deref().unwrap_or("World");
    md_header(
        &mut out,
        &format!("Recipe Analyzer — {world}"),
        &env.view_url,
        &env.as_of,
    );
    let _ = writeln!(
        out,
        "Showing {} of {} matching profitable recipes.\n",
        env.data.len(),
        env.total_matches.unwrap_or(env.data.len())
    );
    let _ = writeln!(
        out,
        "| Item | Job | Lv | Quality | Cost | Sell | Profit (post-tax) | ROI % | Sales/Day | Profit/Day |"
    );
    let _ = writeln!(out, "|---|---|---:|---|---:|---:|---:|---:|---:|---:|");
    for r in &env.data {
        let _ = writeln!(
            out,
            "| [{}]({}) | {} | {} | {} | {} | {} | {} | {}% | {:.2} | {} |",
            r.item_name,
            r.item_url,
            r.job,
            r.recipe_level,
            if r.hq { "HQ" } else { "NQ" },
            r.crafting_cost,
            r.sell_price,
            r.profit,
            r.roi_pct,
            r.daily_sales,
            r.profit_per_day,
        );
    }
    out
}

pub(crate) fn format_vendor_resale_md(env: &AgentEnvelope<Vec<PublicVendorResaleRow>>) -> String {
    let mut out = String::new();
    let world = env.world.as_deref().unwrap_or("World");
    md_header(
        &mut out,
        &format!("Vendor Resale — {world}"),
        &env.view_url,
        &env.as_of,
    );
    let _ = writeln!(
        out,
        "| Item | Vendor | Market | Profit | ROI % | Sales/Day |"
    );
    let _ = writeln!(out, "|---|---:|---:|---:|---:|---:|");
    for r in &env.data {
        let _ = writeln!(
            out,
            "| [{}]({}) | {} | {} | {} | {}% | {:.2} |",
            r.item_name,
            r.item_url,
            r.vendor_price,
            r.market_price,
            r.profit,
            r.roi_pct,
            r.daily_sales
        );
    }
    out
}

pub(crate) fn format_leves_md(env: &AgentEnvelope<Vec<PublicLeveRow>>) -> String {
    let mut out = String::new();
    let world = env.world.as_deref().unwrap_or("World");
    md_header(
        &mut out,
        &format!("Leve Analyzer — {world}"),
        &env.view_url,
        &env.as_of,
    );
    let _ = writeln!(
        out,
        "| Leve | Job | Lv | Turn-in Item | Qty | Gil Reward | Best Cost (method) | Profit |"
    );
    let _ = writeln!(out, "|---|---|---:|---|---:|---:|---:|---:|");
    for r in &env.data {
        let _ = writeln!(
            out,
            "| {} | {} | {} | [{}]({}) | {} | {} | {} ({}) | {} |",
            r.leve_name,
            r.job,
            r.level,
            r.item_name,
            r.item_url,
            r.quantity_required,
            r.gil_reward,
            r.best_cost,
            r.best_method,
            r.profit
        );
    }
    out
}

pub(crate) fn format_ventures_md(env: &AgentEnvelope<Vec<PublicVentureRow>>) -> String {
    let mut out = String::new();
    let world = env.world.as_deref().unwrap_or("World");
    md_header(
        &mut out,
        &format!("Venture Analyzer — {world}"),
        &env.view_url,
        &env.as_of,
    );
    let _ = writeln!(
        out,
        "| Item | Category | Lv | Qty | Unit Price | Est. Net Value | Sales/Day |"
    );
    let _ = writeln!(out, "|---|---|---:|---:|---:|---:|---:|");
    for r in &env.data {
        let _ = writeln!(
            out,
            "| [{}]({}) | {} | {} | {} | {} | {} | {:.2} |",
            r.item_name,
            r.item_url,
            r.job_category,
            r.level,
            r.quantity,
            r.unit_price,
            r.total_value,
            r.daily_sales
        );
    }
    out
}

pub(crate) fn format_scrips_md(env: &AgentEnvelope<Vec<PublicScripRow>>) -> String {
    let mut out = String::new();
    let world = env.world.as_deref().unwrap_or("World");
    md_header(
        &mut out,
        &format!("Scrip Sources — {world}"),
        &env.view_url,
        &env.as_of,
    );
    let _ = writeln!(
        out,
        "| Item | Scrip Type | Job | Scrips | Craft Cost | Gil/Scrip |"
    );
    let _ = writeln!(out, "|---|---|---|---:|---:|---:|");
    for r in &env.data {
        let _ = writeln!(
            out,
            "| [{}]({}) | {} | {} | {} | {} | {:.2} |",
            r.item_name,
            r.item_url,
            r.scrip_type,
            r.job.unwrap_or("Gatherer"),
            r.scrip_amount,
            r.craft_cost,
            r.gil_per_scrip
        );
    }
    out
}

pub(crate) fn format_trends_md(env: &AgentEnvelope<Vec<PublicTrendRow>>) -> String {
    let mut out = String::new();
    let world = env.world.as_deref().unwrap_or("World");
    md_header(
        &mut out,
        &format!("Market Trends — {world}"),
        &env.view_url,
        &env.as_of,
    );
    let _ = writeln!(
        out,
        "| Item | Quality | Current | VWAP | Change % | Sales/Day | Gil Volume |"
    );
    let _ = writeln!(out, "|---|---|---:|---:|---:|---:|---:|");
    for r in &env.data {
        let _ = writeln!(
            out,
            "| [{}]({}) | {} | {} | {} | {:+.1}% | {:.2} | {} |",
            r.item_name,
            r.item_url,
            if r.hq { "HQ" } else { "NQ" },
            r.current_price,
            r.vwap_window,
            r.pct_change_window,
            r.sales_per_day,
            r.gil_volume_window
        );
    }
    out
}

pub(crate) fn format_plan_craft_md(env: &AgentEnvelope<PlanCraftResponse>) -> String {
    let d = &env.data;
    let mut out = String::new();
    let world = env.world.as_deref().unwrap_or("World");
    md_header(
        &mut out,
        &format!("Craft Plan — {world} (Buy Scope: {})", d.buy_scope),
        &env.view_url,
        &env.as_of,
    );
    let _ = writeln!(
        out,
        "**Estimated Total Material Cost**: {} gil (unpriced items: {})\n",
        d.estimated_total_cost, d.missing_price_items
    );
    let _ = writeln!(out, "## Shopping List\n");
    let _ = writeln!(
        out,
        "| Material | Qty | Source | Unit Price | Total | Cheapest World |"
    );
    let _ = writeln!(out, "|---|---:|---|---:|---:|---|");
    for s in &d.shopping_list {
        let _ = writeln!(
            out,
            "| [{}]({}) | {} | {} | {} | {} | {} |",
            s.item_name,
            s.item_url,
            s.quantity,
            s.source,
            s.est_unit_price,
            s.est_total_cost,
            s.cheapest_world.as_deref().unwrap_or("—")
        );
    }
    if !d.craft_steps.is_empty() {
        let _ = writeln!(out, "\n## Craft Order (Bottom-Up)\n");
        for (idx, step) in d.craft_steps.iter().enumerate() {
            let _ = writeln!(
                out,
                "{}. **{}** ×{} crafts (need {}, surplus {}) — {} · [Recipe]({})",
                idx + 1,
                step.item_name,
                step.crafts_needed,
                step.total_needed,
                step.surplus,
                step.job,
                step.recipe_url
            );
        }
    }
    out
}
