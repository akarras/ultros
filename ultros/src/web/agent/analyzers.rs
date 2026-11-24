//! Enriched public market & gil-making query functions, HTTP handlers, and Markdown formatters.
//!
//! Each analyzer has one core query function returning an `AgentEnvelope<T>` shared by:
//! 1. The public REST API (`/api/v1/public/*`)
//! 2. The remote MCP server (`/mcp`)
//! 3. `Accept: text/markdown` / `?format=md` content negotiation on web routes

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum_extra::headers::{CacheControl, HeaderMapExt};
use chrono::Utc;
use futures::future::try_join3;
use serde::{Deserialize, Serialize};
use serde_json::json;
use ultros_api_types::cheapest_listings::CheapestListingsMap;
use ultros_api_types::recent_sales::{SaleData, Sales};
use ultros_api_types::trends::ConfidenceBand;
use ultros_api_types::world_helper::{AnyResult, WorldHelper};
use ultros_calc::analysis::{SalesStats, analyze_sales, flip_profit, sale_tax};
use ultros_calc::recipe_planner;
use ultros_crafting::cost::{
    CraftingCostOptions, EmptyOnHand, PriceSource, ShardsMode, compute_cost,
};
use ultros_db::world_data::world_cache::AnySelector as DbAnySelector;
use ultros_game_sources::{ScripType, scrip_turn_ins, venture_rewards};
use universalis::ItemId as UnivItemId;
use xiv_gen::{
    ClassJobCategoryId, ItemId, LeveId, LeveRewardItemGroupId, LeveRewardItemId, Recipe,
    RecipeLevelTableId,
};

use crate::analyzer_service::AnalyzerError;
use crate::web::api::query::optional_flag;
use crate::web::api::{BestDealsQuery, ResaleStatsDto, TrendsQuery};
use crate::web::error::WebError;
use crate::web::state::WebState;

use super::{
    AgentEnvelope, SITE_ORIGIN, craft_type_info, db_selector, decode_market_json, is_shard_item,
    item_category_name, item_name, item_url, normalize_crafter_job, recipe_url, recipes_by_output,
    resolve_item_arg, shared_heavy_semaphore, shared_result_cache, snapshot_cheapest_map,
    static_planner_graph, tool_url, vendor_price_map, world_name_by_id,
};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 50;
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

fn cached_json_bytes(body: String, secs: u64) -> Response {
    let mut resp = (StatusCode::OK, body).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    resp.headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(secs)));
    resp
}

/// Net sale proceeds after 5% market-board tax, using `ultros_calc`'s shared rule for `i32`.
pub(crate) fn post_tax_price_i32(price: i32, include_tax: bool) -> i32 {
    if include_tax {
        flip_profit(price, 0, true)
    } else {
        price
    }
}

pub(crate) fn tax_amount_i32(price: i32, include_tax: bool) -> i32 {
    if include_tax { sale_tax(price) } else { 0 }
}

/// Exact integer 5% market-board tax deduction on `i64` totals (never narrows to `i32`).
pub(crate) fn post_tax_price_i64(gross: i64, include_tax: bool) -> i64 {
    if include_tax {
        gross - (gross * 5 + 99) / 100
    } else {
        gross
    }
}

/// Whole-batch crafting cost for `total_qty` units of a recipe whose single execution yields
/// `yield_amount` units and costs `batch_cost`.
pub(crate) fn whole_batch_craft_cost(total_qty: u32, yield_amount: i32, batch_cost: i32) -> i64 {
    let yield_per_craft = yield_amount.max(1) as i64;
    let batches = (total_qty as i64 + yield_per_craft - 1) / yield_per_craft;
    batches.saturating_mul(batch_cost as i64)
}

/// Expected market gil value of a guildleve's `LeveRewardItem` groups (mirrors the frontend
/// `leve_analyzer.rs` calculation so API profit matches the linked Leve Analyzer page).
fn leve_expected_item_reward_value(
    reward_item_id: i32,
    prices: &CheapestListingsMap,
    data: &xiv_gen::Data,
) -> i32 {
    let Some(entry) = data
        .leve_reward_items
        .get(&LeveRewardItemId(reward_item_id))
    else {
        return 0;
    };
    let groups = [
        (entry.leve_reward_item_group_0, entry.probability_percent_0),
        (entry.leve_reward_item_group_1, entry.probability_percent_1),
        (entry.leve_reward_item_group_2, entry.probability_percent_2),
        (entry.leve_reward_item_group_3, entry.probability_percent_3),
        (entry.leve_reward_item_group_4, entry.probability_percent_4),
        (entry.leve_reward_item_group_5, entry.probability_percent_5),
        (entry.leve_reward_item_group_6, entry.probability_percent_6),
        (entry.leve_reward_item_group_7, entry.probability_percent_7),
    ];
    let mut expected = 0.0_f64;
    for (group_id, probability) in groups {
        if group_id == 0 || probability == 0 {
            continue;
        }
        let Some(group) = data
            .leve_reward_item_groups
            .get(&LeveRewardItemGroupId(group_id.into()))
        else {
            continue;
        };
        let group_items = [
            (group.item_0, group.count_0),
            (group.item_1, group.count_1),
            (group.item_2, group.count_2),
            (group.item_3, group.count_3),
            (group.item_4, group.count_4),
            (group.item_5, group.count_5),
            (group.item_6, group.count_6),
            (group.item_7, group.count_7),
            (group.item_8, group.count_8),
        ];
        for (item_id, count) in group_items {
            if item_id == 0 || count == 0 {
                continue;
            }
            let unit_price = prices
                .find_matching_listings(item_id.into())
                .lq
                .map(|l| l.price)
                .unwrap_or(0);
            expected += (unit_price as f64) * (count as f64) * (probability as f64 / 100.0);
        }
    }
    expected.round() as i32
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
    state: &WebState,
    world: &str,
) -> Result<HashMap<(i32, bool), SaleData>, WebError> {
    let response = state
        .market_client
        .recent_sales(world, None, &Default::default())
        .await
        .map_err(|_| WebError::TemporarilyUnavailable)?;
    let rows: ultros_api_types::recent_sales::RecentSales = decode_market_json(response).await?;
    Ok(rows
        .sales
        .into_iter()
        .map(|row| ((row.item_id, row.hq), row))
        .collect())
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
        if let Ok(cheapest_map) = snapshot_cheapest_map(state, &buy_sel).await {
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

pub(crate) fn build_flips_view_url(world_name: &str, q: &PublicFlipsQuery) -> String {
    let show_suspicious = q.show_suspicious.unwrap_or(false);
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
    tool_url("flip-finder", world_name, &params)
}

pub(crate) async fn query_flips(
    state: &WebState,
    world_arg: &str,
    q: PublicFlipsQuery,
) -> Result<AgentEnvelope<Vec<PublicFlipRow>>, WebError> {
    let resolved = resolve_single_world(&state.world_helper, world_arg)?;
    let filter_sale = q.filter_sale.as_deref().and_then(|s| {
        ["Day", "Week", "Month"]
            .into_iter()
            .find(|value| value.eq_ignore_ascii_case(s))
            .map(str::to_owned)
    });
    let show_suspicious = q.show_suspicious.unwrap_or(false);
    let limit = clamp_limit(q.limit);
    let query = BestDealsQuery {
        min_profit: q.min_profit,
        filter_sale,
        show_suspicious: Some(show_suspicious),
        min_velocity: q.min_velocity,
        min_buffer_sales: q.min_buffer_sales,
        max_roi: q.max_roi,
        limit: Some(limit as u32),
    };
    let response = state
        .market_client
        .best_deals(&resolved.world_name, &query)
        .await?;
    let total = response
        .headers()
        .get("x-total-count")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());
    let all_rows: Vec<ResaleStatsDto> = decode_market_json(response).await?;
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
                sold_within: r.sold_within,
                velocity_per_day: r.velocity_per_day.map(|v| (v * 100.0).round() / 100.0),
                recent_sales_count: r.buffer_sale_count,
                confidence_band: r.confidence_band,
                vwap_30d: r.vwap_30d,
                launder_suspicion: (r.launder_suspicion * 1000.0).round() / 1000.0,
                item_url: item_url(&resolved.world_name, r.item_id),
            }
        })
        .collect();

    let view_url = build_flips_view_url(&resolved.world_name, &q);
    let mut envelope =
        AgentEnvelope::new(view_url, Some(resolved.world_name), rows).with_filters(json!({
            "min_profit": q.min_profit,
            "filter_sale": q.filter_sale,
            "min_velocity": q.min_velocity,
            "min_buffer_sales": q.min_buffer_sales,
            "max_roi": q.max_roi,
            "show_suspicious": show_suspicious,
            "limit": limit,
        }));
    envelope.total_matches = total;
    Ok(envelope)
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

/// Build a Recipe Analyzer `view_url` matching `ultros-app/src/routes/recipe_analyzer.rs`
/// (`RECIPE_COLUMNS` sort_id + `recipe_filter_aliases`: `min-sales` filters daily sales/day,
/// `cost` sorts ascending by default, `velocity` sorts daily sales).
pub(crate) fn build_recipes_view_url(
    world_name: &str,
    job_code: Option<&str>,
    buy_scope_label: &str,
    sort_mode: &str,
    q: &PublicRecipesQuery,
) -> String {
    let use_subcrafts = q.use_subcrafts.unwrap_or(false);
    let require_hq = q.require_hq.unwrap_or(false);
    let exclude_shards = q.exclude_shards.unwrap_or(true);
    let (ui_sort, ui_dir) = match sort_mode {
        "roi" => ("roi", "desc"),
        "cost" => ("cost", "asc"),
        "price" => ("price", "desc"),
        "daily_sales" => ("velocity", "desc"),
        _ => ("profit", "desc"),
    };
    let mut params: Vec<(&str, String)> = vec![
        ("buy-scope", buy_scope_label.into()),
        ("subcrafts", use_subcrafts.to_string()),
        ("require-hq", require_hq.to_string()),
        ("shards-exclude", exclude_shards.to_string()),
        ("sort", ui_sort.into()),
        ("dir", ui_dir.into()),
    ];
    if let Some(code) = job_code {
        params.push(("job", code.into()));
    }
    if let Some(min_p) = q.min_profit {
        params.push(("profit", min_p.to_string()));
    }
    if let Some(min_r) = q.min_roi {
        params.push(("roi", min_r.to_string()));
    }
    // Note: in the frontend `min-sales` is aliased to `daily-sales >= value`, NOT buffer count.
    if let Some(min_d) = q.min_daily_sales {
        params.push(("min-sales", min_d.to_string()));
    }
    tool_url("recipe-analyzer", world_name, &params)
}

/// Lightweight intermediate row kept during full-catalog scan; full ingredient breakdowns are
/// computed ONLY for the final `limit` rows after sorting & truncation.
struct ScoredRecipe {
    recipe: &'static Recipe,
    item_name: &'static str,
    job: &'static str,
    job_name: &'static str,
    recipe_level: i8,
    hq: bool,
    crafting_cost: i32,
    sell_price: i32,
    tax: i32,
    profit: i32,
    roi_pct: i32,
    daily_sales: f32,
    profit_per_day: i64,
    recent_sales_count: usize,
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

    // Bound concurrent heavy scans so bursts never starve Tokio workers.
    let _permit = shared_heavy_semaphore()
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| AnalyzerError::Uninitialized)?;

    let (sell_map, buy_map, sales_map) = try_join3(
        snapshot_cheapest_map(state, &DbAnySelector::World(resolved.world_id)),
        snapshot_cheapest_map(state, &buy_selector),
        snapshot_world_sales_by_item(state, &resolved.world_name),
    )
    .await?;

    let world_helper = state.world_helper.clone();
    let world_name = resolved.world_name.clone();
    let min_level = q.min_level;
    let max_level = q.max_level;
    let min_profit = q.min_profit;
    let min_roi = q.min_roi;
    let min_sales = q.min_sales;
    let min_daily_sales = q.min_daily_sales;
    let sort_for_task = sort_mode.clone();

    let (rows, total) = tokio::task::spawn_blocking(move || {
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

        let mut scored: Vec<ScoredRecipe> = Vec::new();
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
            if let Some(min_l) = min_level
                && recipe_level < min_l
            {
                continue;
            }
            if let Some(max_l) = max_level
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
            if let Some(median) = median_sale
                && median > 0
                && sales_stats.total_sales > 0
                && (sell_price as i64) > (median as i64) * SUSPICIOUS_PRICE_MULTIPLE
            {
                continue;
            }

            if let Some(min_s) = min_sales
                && sales_stats.total_sales < min_s
            {
                continue;
            }
            if let Some(min_d) = min_daily_sales
                && sales_stats.daily_sales < min_d
            {
                continue;
            }

            let breakdown = compute_cost(recipe, &buy_map, recipes_map, &opts, &is_shard_item);
            if breakdown.unpriced_market_lines > 0 || breakdown.cost <= 0 {
                continue;
            }
            let unit = breakdown.per_unit(recipe.amount_result);
            if unit.cost <= 0 {
                continue;
            }

            let net_sell = post_tax_price_i32(sell_price, true);
            let tax = tax_amount_i32(sell_price, true);
            let profit = net_sell - unit.cost;
            let roi_pct = ((profit as f64 / unit.cost as f64) * 100.0).round() as i32;

            if let Some(min_p) = min_profit
                && profit < min_p
            {
                continue;
            }
            if let Some(min_r) = min_roi
                && roi_pct < min_r
            {
                continue;
            }

            let profit_per_day = if profit > 0 && sales_stats.daily_sales > 0.0 {
                (profit as f64 * sales_stats.daily_sales as f64).round() as i64
            } else {
                0
            };

            scored.push(ScoredRecipe {
                recipe,
                item_name: item.name.as_str(),
                job: job_code,
                job_name,
                recipe_level,
                hq: is_hq,
                crafting_cost: unit.cost,
                sell_price,
                tax,
                profit,
                roi_pct,
                daily_sales: (sales_stats.daily_sales * 100.0).round() / 100.0,
                profit_per_day,
                recent_sales_count: sales_stats.total_sales,
            });
        }

        match sort_for_task.as_str() {
            "roi" => scored.sort_by(|a, b| b.roi_pct.cmp(&a.roi_pct).then(b.profit.cmp(&a.profit))),
            "profit_per_day" | "ppd" => scored.sort_by(|a, b| {
                b.profit_per_day
                    .cmp(&a.profit_per_day)
                    .then(b.profit.cmp(&a.profit))
            }),
            "daily_sales" => scored.sort_by(|a, b| {
                b.daily_sales
                    .total_cmp(&a.daily_sales)
                    .then(b.profit.cmp(&a.profit))
            }),
            "cost" => scored.sort_by_key(|a| a.crafting_cost),
            "price" => scored.sort_by_key(|a| std::cmp::Reverse(a.sell_price)),
            _ => scored.sort_by(|a, b| b.profit.cmp(&a.profit).then(b.roi_pct.cmp(&a.roi_pct))),
        }

        let total = scored.len();
        scored.truncate(limit);

        // Build full ingredient breakdowns ONLY for the surviving top `limit` rows.
        let rows: Vec<PublicRecipeRow> = scored
            .into_iter()
            .map(|s| {
                let breakdown =
                    compute_cost(s.recipe, &buy_map, recipes_map, &opts, &is_shard_item);
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
                            .then(|| world_name_by_id(&world_helper, l.world_id)),
                    })
                    .collect();

                PublicRecipeRow {
                    recipe_id: s.recipe.key_id.0,
                    item_id: s.recipe.item_result,
                    item_name: s.item_name.to_string(),
                    job: s.job,
                    job_name: s.job_name,
                    recipe_level: s.recipe_level,
                    yield_amount: s.recipe.amount_result.max(1),
                    hq: s.hq,
                    crafting_cost: s.crafting_cost,
                    sell_price: s.sell_price,
                    tax: s.tax,
                    profit: s.profit,
                    roi_pct: s.roi_pct,
                    daily_sales: s.daily_sales,
                    profit_per_day: s.profit_per_day,
                    recent_sales_count: s.recent_sales_count,
                    ingredients,
                    item_url: item_url(&world_name, s.recipe.item_result),
                    recipe_url: recipe_url(s.recipe.key_id.0),
                }
            })
            .collect();

        (rows, total)
    })
    .await
    .map_err(|e| WebError::AnyhowError(anyhow::anyhow!("recipe scan task failed: {e}")))?;

    let view_url = build_recipes_view_url(
        &resolved.world_name,
        job_filter.map(|(c, _)| c),
        buy_scope_label,
        &sort_mode,
        &q,
    );
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
    let cache_key = format!(
        "recipes:{}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}",
        world.to_ascii_lowercase(),
        query.job.as_deref().map(str::to_ascii_lowercase),
        query.min_level,
        query.max_level,
        query.buy_scope.as_deref().map(str::to_ascii_lowercase),
        query.use_subcrafts,
        query.require_hq,
        query.exclude_shards,
        query.min_profit,
        query.min_roi,
        query.min_sales,
        query.min_daily_sales.map(|v| (v * 100.0).round() as i32),
        query.sort.as_deref().map(str::to_ascii_lowercase),
        clamp_limit(query.limit),
    );
    if let Some(hit) = shared_result_cache().get(&cache_key) {
        return Ok(cached_json_bytes(hit, 30));
    }
    let env = query_recipes(&state, &world, query).await?;
    let body = serde_json::to_string(&env).map_err(anyhow::Error::from)?;
    shared_result_cache().insert(cache_key, body.clone(), Duration::from_secs(30));
    Ok(cached_json_bytes(body, 30))
}

// ============================================================================
// 6. Vendor Resale Analyzer (NPC Gil Shop -> NQ Market Board)
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
    pub(crate) price_basis: &'static str,
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

    let _permit = shared_heavy_semaphore()
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| AnalyzerError::Uninitialized)?;

    let (cheapest_map, sales_map) = futures::future::try_join(
        snapshot_cheapest_map(state, &DbAnySelector::World(resolved.world_id)),
        snapshot_world_sales_by_item(state, &resolved.world_name),
    )
    .await?;

    let world_name = resolved.world_name.clone();
    let min_profit = q.min_profit;
    let min_roi = q.min_roi;
    let min_sales = q.min_sales;
    let sort_for_task = sort_mode.clone();

    let (rows, total) = tokio::task::spawn_blocking(move || {
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
            // NPC gil shops sell NQ only — never price against an HQ-only listing.
            let summary = cheapest_map.find_matching_listings(item_id);
            let Some(market_price) = summary.lq.map(|l| l.price).filter(|&p| p > 0) else {
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
            let net_sell = post_tax_price_i32(market_price, include_tax);
            let profit = net_sell - vendor_price;
            if profit <= 0 {
                continue;
            }
            let roi_pct = ((profit as f64 / vendor_price as f64) * 100.0).round() as i32;

            if let Some(min_p) = min_profit
                && profit < min_p
            {
                continue;
            }
            if let Some(min_r) = min_roi
                && roi_pct < min_r
            {
                continue;
            }
            if let Some(min_s) = min_sales
                && sales_stats.total_sales < min_s
            {
                continue;
            }

            rows.push(PublicVendorResaleRow {
                item_id,
                item_name: item.name.clone(),
                category: item_category_name(item).map(ToOwned::to_owned),
                price_basis: "nq_listing",
                vendor_price,
                market_price,
                median_sale_price: median,
                profit,
                roi_pct,
                daily_sales: (sales_stats.daily_sales * 100.0).round() / 100.0,
                recent_sales_count: sales_stats.total_sales,
                item_url: item_url(&world_name, item_id),
            });
        }

        match sort_for_task.as_str() {
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
        (rows, total)
    })
    .await
    .map_err(|e| WebError::AnyhowError(anyhow::anyhow!("vendor resale task failed: {e}")))?;

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
    metrics::counter!(
        "ultros_agent_requests_total",
        "surface" => "rest",
        "tool" => "vendor_resale"
    )
    .increment(1);
    let cache_key = format!(
        "vendor:{}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{}",
        world.to_ascii_lowercase(),
        query.min_profit,
        query.min_roi,
        query.min_sales,
        query.tax,
        query.show_suspicious,
        query.sort.as_deref().map(str::to_ascii_lowercase),
        clamp_limit(query.limit),
    );
    if let Some(hit) = shared_result_cache().get(&cache_key) {
        return Ok(cached_json_bytes(hit, 30));
    }
    let env = query_vendor_resale(&state, &world, query).await?;
    let body = serde_json::to_string(&env).map_err(anyhow::Error::from)?;
    shared_result_cache().insert(cache_key, body.clone(), Duration::from_secs(30));
    Ok(cached_json_bytes(body, 30))
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
    pub(crate) expected_item_reward_value: i32,
    pub(crate) total_revenue: i32,
    pub(crate) market_buy_cost: Option<i64>,
    pub(crate) craft_batch_cost: Option<i64>,
    pub(crate) best_cost: i64,
    pub(crate) best_method: &'static str,
    pub(crate) profit: i64,
    pub(crate) item_url: String,
}

/// Effective HQ gil reward for a single delivery: only doubles if `assume_hq && can_be_hq`.
pub(crate) fn leve_effective_gil_reward(
    base_gil: i32,
    assume_hq: bool,
    can_be_hq: bool,
) -> (bool, i32) {
    let effective_hq = assume_hq && can_be_hq;
    let gil = if effective_hq {
        base_gil.saturating_mul(2)
    } else {
        base_gil
    };
    (effective_hq, gil)
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
    // Default to NQ baseline (matching the web Leve Analyzer UI); HQ doubles reward only when
    // requested AND the turn-in item actually supports HQ.
    let assume_hq = q.hq.unwrap_or(false);
    let (buy_selector, buy_scope_label) =
        buy_scope_selector(&resolved, q.buy_scope.as_deref().unwrap_or("world"));
    let limit = clamp_limit(q.limit);
    let sort_mode = q
        .sort
        .as_deref()
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "profit".to_string());

    let _permit = shared_heavy_semaphore()
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| AnalyzerError::Uninitialized)?;

    let (prices, reward_prices) = futures::future::try_join(
        snapshot_cheapest_map(state, &buy_selector),
        snapshot_cheapest_map(state, &DbAnySelector::World(resolved.world_id)),
    )
    .await?;

    let world_name = resolved.world_name.clone();
    let min_level = q.min_level;
    let max_level = q.max_level;
    let min_profit = q.min_profit;
    let sort_for_task = sort_mode.clone();

    let (rows, total) = tokio::task::spawn_blocking(move || {
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
            if let Some(min_l) = min_level
                && level < min_l
            {
                continue;
            }
            if let Some(max_l) = max_level
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
            let Some(item) = data.items.get(&ItemId(item_id)) else {
                continue;
            };
            let total_qty = craft_leve.item_count_0 as u32;
            let (effective_hq, gil_reward) =
                leve_effective_gil_reward(leve.gil_reward, assume_hq, item.can_be_hq);
            let expected_item_reward_value =
                leve_expected_item_reward_value(leve.leve_reward_item, &reward_prices, data);
            let total_revenue = gil_reward.saturating_add(expected_item_reward_value);

            let summary = prices.find_matching_listings(item_id);
            let unit_buy = if effective_hq {
                summary.hq.map(|l| l.price)
            } else {
                summary.lq.map(|l| l.price)
            };
            let market_buy_cost = unit_buy
                .filter(|&p| p > 0)
                .map(|p| p as i64 * total_qty as i64);

            // Whole-batch crafting cost (charges for full crafts required, not prorated units).
            let craft_batch_cost = recipes_map.get(&ItemId(item_id)).and_then(|recipes| {
                recipes
                    .iter()
                    .filter_map(|recipe| {
                        let b =
                            compute_cost(recipe, &prices, recipes_map, &craft_opts, &is_shard_item);
                        if b.unpriced_market_lines > 0 || b.cost <= 0 {
                            return None;
                        }
                        Some(whole_batch_craft_cost(
                            total_qty,
                            recipe.amount_result,
                            b.cost,
                        ))
                    })
                    .min()
            });

            // For HQ turn-ins, crafting uses NQ mats as a baseline estimate (actual HQ synthesis
            // requires crafter stats), so label it transparently as `craft_hq_estimate`.
            let (best_cost, best_method) = match (market_buy_cost, craft_batch_cost) {
                (Some(b), Some(c)) if c < b => (
                    c,
                    if effective_hq {
                        "craft_hq_estimate"
                    } else {
                        "craft"
                    },
                ),
                (Some(b), _) => (b, if effective_hq { "buy_hq" } else { "buy_nq" }),
                (None, Some(c)) => (
                    c,
                    if effective_hq {
                        "craft_hq_estimate"
                    } else {
                        "craft"
                    },
                ),
                (None, None) => continue,
            };
            let profit = total_revenue as i64 - best_cost;
            if let Some(min_p) = min_profit
                && profit < min_p as i64
            {
                continue;
            }

            rows.push(PublicLeveRow {
                leve_id: craft_leve.leve,
                leve_name: leve.name.clone(),
                job: job_name.to_string(),
                level,
                item_id,
                item_name: item.name.clone(),
                quantity_required: total_qty,
                hq: effective_hq,
                gil_reward,
                expected_item_reward_value,
                total_revenue,
                market_buy_cost,
                craft_batch_cost,
                best_cost,
                best_method,
                profit,
                item_url: item_url(&world_name, item_id),
            });
        }

        match sort_for_task.as_str() {
            "level" => rows.sort_by(|a, b| b.level.cmp(&a.level).then(b.profit.cmp(&a.profit))),
            "cost" => rows.sort_by_key(|a| a.best_cost),
            _ => rows.sort_by_key(|a| std::cmp::Reverse(a.profit)),
        }
        let total = rows.len();
        rows.truncate(limit);
        (rows, total)
    })
    .await
    .map_err(|e| WebError::AnyhowError(anyhow::anyhow!("leve scan task failed: {e}")))?;

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
                "basis": "per_single_delivery_including_expected_item_rewards",
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
    let cache_key = format!(
        "leves:{}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{}",
        world.to_ascii_lowercase(),
        query.job.as_deref().map(str::to_ascii_lowercase),
        query.min_level,
        query.max_level,
        query.hq,
        query.min_profit,
        query.buy_scope.as_deref().map(str::to_ascii_lowercase),
        query.sort.as_deref().map(str::to_ascii_lowercase),
        clamp_limit(query.limit),
    );
    if let Some(hit) = shared_result_cache().get(&cache_key) {
        return Ok(cached_json_bytes(hit, 60));
    }
    let env = query_leves(&state, &world, query).await?;
    let body = serde_json::to_string(&env).map_err(anyhow::Error::from)?;
    shared_result_cache().insert(cache_key, body.clone(), Duration::from_secs(60));
    Ok(cached_json_bytes(body, 60))
}

// ============================================================================
// 8. Venture Analyzer (Retainer Ventures — NQ Listing Basis)
// ============================================================================

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PublicVenturesQuery {
    pub(crate) min_level: Option<u8>,
    pub(crate) max_level: Option<u8>,
    pub(crate) min_profit: Option<i64>,
    pub(crate) sort: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PublicVentureRow {
    pub(crate) task_id: i32,
    pub(crate) item_id: i32,
    pub(crate) item_name: String,
    pub(crate) job_category: String,
    pub(crate) price_basis: &'static str,
    pub(crate) level: u8,
    pub(crate) quantity: i32,
    pub(crate) unit_price: i32,
    pub(crate) total_value: i64,
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

    let _permit = shared_heavy_semaphore()
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| AnalyzerError::Uninitialized)?;

    let (cheapest_map, sales_map) = futures::future::try_join(
        snapshot_cheapest_map(state, &DbAnySelector::World(resolved.world_id)),
        snapshot_world_sales_by_item(state, &resolved.world_name),
    )
    .await?;

    let world_name = resolved.world_name.clone();
    let min_level = q.min_level;
    let max_level = q.max_level;
    let min_profit = q.min_profit;
    let sort_for_task = sort_mode.clone();

    let (rows, total) = tokio::task::spawn_blocking(move || {
        let data = xiv_gen_db::data();
        let mut rows = Vec::new();
        for reward in venture_rewards(data) {
            if let Some(min_l) = min_level
                && reward.level < min_l
            {
                continue;
            }
            if let Some(max_l) = max_level
                && reward.level > max_l
            {
                continue;
            }
            // Retainer fixed ventures yield NQ goods — price from NQ listings & NQ history.
            let summary = cheapest_map.find_matching_listings(reward.item.0);
            let Some(unit_price) = summary.lq.map(|l| l.price).filter(|&p| p > 0) else {
                continue;
            };
            let (sales_stats, median) =
                summarize_item_sales(&sales_map, reward.item.0, Some(false));
            if let Some(med) = median
                && med > 0
                && sales_stats.total_sales > 0
                && (unit_price as i64) > (med as i64) * SUSPICIOUS_PRICE_MULTIPLE
            {
                continue;
            }
            let gross_value = unit_price as i64 * reward.quantity as i64;
            let total_value = post_tax_price_i64(gross_value, true);
            if let Some(min_p) = min_profit
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
                price_basis: "nq_listing",
                level: reward.level,
                quantity: reward.quantity,
                unit_price,
                total_value,
                daily_sales: (sales_stats.daily_sales * 100.0).round() / 100.0,
                recent_sales_count: sales_stats.total_sales,
                item_url: item_url(&world_name, reward.item.0),
            });
        }

        match sort_for_task.as_str() {
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
        (rows, total)
    })
    .await
    .map_err(|e| WebError::AnyhowError(anyhow::anyhow!("venture scan task failed: {e}")))?;

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
    let cache_key = format!(
        "ventures:{}:{:?}:{:?}:{:?}:{:?}:{}",
        world.to_ascii_lowercase(),
        query.min_level,
        query.max_level,
        query.min_profit,
        query.sort.as_deref().map(str::to_ascii_lowercase),
        clamp_limit(query.limit),
    );
    if let Some(hit) = shared_result_cache().get(&cache_key) {
        return Ok(cached_json_bytes(hit, 60));
    }
    let env = query_ventures(&state, &world, query).await?;
    let body = serde_json::to_string(&env).map_err(anyhow::Error::from)?;
    shared_result_cache().insert(cache_key, body.clone(), Duration::from_secs(60));
    Ok(cached_json_bytes(body, 60))
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
    pub(crate) reward_tier: &'static str,
    pub(crate) acquisition: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) job: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recipe_id: Option<i32>,
    pub(crate) craft_cost: Option<i32>,
    pub(crate) gil_per_scrip: Option<f32>,
    pub(crate) item_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recipe_url: Option<String>,
}

/// Sort priced scrip turn-ins ascending by `gil_per_scrip`, placing unpriced/gathered rows LAST
/// so zero-gil gathered collectables never crowd out priced crafted turn-ins.
pub(crate) fn sort_scrip_rows(rows: &mut [PublicScripRow]) {
    rows.sort_by(|a, b| match (a.gil_per_scrip, b.gil_per_scrip) {
        (Some(ag), Some(bg)) => ag.total_cmp(&bg).then(b.scrip_amount.cmp(&a.scrip_amount)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.scrip_amount.cmp(&a.scrip_amount),
    });
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

    let _permit = shared_heavy_semaphore()
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| AnalyzerError::Uninitialized)?;

    let buy_map = snapshot_cheapest_map(state, &buy_selector).await?;
    let world_name = resolved.world_name.clone();

    let (rows, total) = tokio::task::spawn_blocking(move || {
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

            let Some(recipes) = recipes_map.get(&ItemId(turn_in.item_id)) else {
                if job_filter.is_some() {
                    continue;
                }
                if turn_in.scrip_type.is_gatherer() {
                    // Gathered collectables have no market gil outlay (effort-based);
                    // keep `gil_per_scrip: None` so priced crafted turn-ins rank first.
                    rows.push(PublicScripRow {
                        item_id: turn_in.item_id,
                        item_name: item_name(turn_in.item_id).to_string(),
                        scrip_type: scrip_key.to_string(),
                        scrip_amount: turn_in.scrip_amount,
                        reward_tier: "max_collectability",
                        acquisition: "gathered",
                        job: None,
                        recipe_id: None,
                        craft_cost: None,
                        gil_per_scrip: None,
                        item_url: item_url(&world_name, turn_in.item_id),
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
                let gps =
                    ((unit_cost as f64 / turn_in.scrip_amount as f64) * 100.0).round() / 100.0;
                rows.push(PublicScripRow {
                    item_id: turn_in.item_id,
                    item_name: item_name(turn_in.item_id).to_string(),
                    scrip_type: scrip_key.to_string(),
                    scrip_amount: turn_in.scrip_amount,
                    reward_tier: "max_collectability",
                    acquisition: "crafted",
                    job,
                    recipe_id: Some(recipe.key_id.0),
                    craft_cost: Some(unit_cost),
                    gil_per_scrip: Some(gps as f32),
                    item_url: item_url(&world_name, turn_in.item_id),
                    recipe_url: Some(recipe_url(recipe.key_id.0)),
                });
            }
        }

        sort_scrip_rows(&mut rows);
        let total = rows.len();
        rows.truncate(limit);
        (rows, total)
    })
    .await
    .map_err(|e| WebError::AnyhowError(anyhow::anyhow!("scrip scan task failed: {e}")))?;

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
    let cache_key = format!(
        "scrips:{}:{:?}:{:?}:{:?}:{:?}:{}",
        world.to_ascii_lowercase(),
        query.scrip.as_deref().map(str::to_ascii_lowercase),
        query.job.as_deref().map(str::to_ascii_lowercase),
        query.buy_scope.as_deref().map(str::to_ascii_lowercase),
        query.use_subcrafts,
        clamp_limit(query.limit),
    );
    if let Some(hit) = shared_result_cache().get(&cache_key) {
        return Ok(cached_json_bytes(hit, 60));
    }
    let env = query_scrips(&state, &world, query).await?;
    let body = serde_json::to_string(&env).map_err(anyhow::Error::from)?;
    shared_result_cache().insert(cache_key, body.clone(), Duration::from_secs(60));
    Ok(cached_json_bytes(body, 60))
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

pub(crate) fn build_trends_view_url(
    world_name: &str,
    window_days: u16,
    show_suspicious: bool,
) -> String {
    let mut params: Vec<(&str, String)> = vec![("window", window_days.to_string())];
    if show_suspicious {
        params.push(("show_suspicious", "true".into()));
    }
    tool_url("trends", world_name, &params)
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

    let query = TrendsQuery {
        window: Some(window_days),
        show_suspicious: Some(show_suspicious),
    };
    let response = state
        .market_client
        .trends(&resolved.world_name, &query)
        .await?;
    let trends = decode_market_json::<ultros_api_types::trends::TrendsData>(response).await;
    let mut items = match trends {
        Ok(trends) if !trends.items.is_empty() => trends.items,
        window_result => {
            let legacy = state
                .market_client
                .trends(&resolved.world_name, &TrendsQuery::default())
                .await?;
            match decode_market_json::<ultros_api_types::trends::TrendsData>(legacy).await {
                Ok(trends) => trends.high_velocity,
                Err(_) => window_result?.items,
            }
        }
    };

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

    let view_url = build_trends_view_url(&resolved.world_name, window_days, show_suspicious);
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
// 11. Craft Shopping List Planner (Unified Multi-Target Demand Graph)
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

const SYNTHETIC_ROOT_ITEM_ID: i32 = -1;
const SYNTHETIC_ROOT_RECIPE_ID: i32 = -1;

/// Expand a combined list of `(item_id, quantity, root_recipe_id)` targets through a single
/// synthetic root recipe so shared multi-yield subcrafts round ONCE for the combined demand,
/// recipe choices remain deterministic, and craft steps come out in exact topological order.
pub(crate) fn expand_unified_plan(
    targets: &[(i32, i64, i32)],
    recipes: &BTreeMap<i32, recipe_planner::Recipe>,
    default_choices: &BTreeMap<i32, i32>,
    use_subcrafts: bool,
    excluded: &BTreeSet<i32>,
) -> Result<Vec<recipe_planner::Material>, String> {
    // Combine duplicate target items into one demand entry before expanding.
    let mut combined_targets: BTreeMap<i32, i64> = BTreeMap::new();
    let mut choices: BTreeMap<i32, i32> = if use_subcrafts {
        default_choices.clone()
    } else {
        BTreeMap::new()
    };
    for &(item_id, qty, recipe_id) in targets {
        let clamped = qty.clamp(1, 9999);
        *combined_targets.entry(item_id).or_insert(0) += clamped;
        choices.insert(item_id, recipe_id);
    }

    let synthetic_root = recipe_planner::Recipe {
        id: SYNTHETIC_ROOT_RECIPE_ID,
        output: SYNTHETIC_ROOT_ITEM_ID,
        yield_amount: 1,
        ingredients: combined_targets.into_iter().collect(),
    };
    let mut working_recipes = recipes.clone();
    working_recipes.insert(SYNTHETIC_ROOT_RECIPE_ID, synthetic_root.clone());

    let owned = BTreeMap::new();
    let expanded = recipe_planner::expand(
        &synthetic_root,
        1,
        &working_recipes,
        &choices,
        &owned,
        excluded,
    )?;

    // Drop the synthetic root node; `expand` returns parent-first topological order,
    // so reversing yields exact bottom-up crafting order.
    Ok(expanded
        .into_iter()
        .filter(|m| m.item != SYNTHETIC_ROOT_ITEM_ID)
        .collect())
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

    // Validate and resolve every target BEFORE acquiring semaphore or reading market data.
    let recipes_by_out = recipes_by_output();
    let mut resolved_targets: Vec<(i32, i64, i32)> = Vec::with_capacity(req.items.len());
    for target in &req.items {
        if target.quantity <= 0 {
            return Err(WebError::BadRequest);
        }
        let item_id =
            resolve_item_arg(&target.item, &state.search_service).ok_or(WebError::NotFound)?;
        let root_recipe = recipes_by_out
            .get(&ItemId(item_id))
            .and_then(|v| v.first().copied())
            .ok_or(WebError::BadRequest)?;
        resolved_targets.push((item_id, target.quantity, root_recipe.key_id.0));
    }
    let first_recipe_id = resolved_targets.first().map(|t| t.2);

    let _permit = shared_heavy_semaphore()
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| AnalyzerError::Uninitialized)?;

    let buy_map = snapshot_cheapest_map(state, &buy_selector).await?;
    let world_helper = state.world_helper.clone();
    let world_name = resolved.world_name.clone();

    let (shopping_list, craft_steps, total_cost, missing_price_items) =
        tokio::task::spawn_blocking(move || -> Result<_, String> {
            let graph = static_planner_graph();
            let empty_excluded = BTreeSet::new();
            let excluded = if exclude_shards {
                &graph.shard_ids
            } else {
                &empty_excluded
            };

            let materials = expand_unified_plan(
                &resolved_targets,
                &graph.recipes,
                &graph.default_choices,
                use_subcrafts,
                excluded,
            )?;

            let data = xiv_gen_db::data();
            let vendors = vendor_price_map();
            let mut total_cost: i64 = 0;
            let mut missing_price_items: usize = 0;
            let mut shopping_list = Vec::new();
            let mut craft_steps = Vec::new();

            // `materials` is parent-first topological order; collect crafts in reverse (bottom-up).
            for mat in materials.iter().rev() {
                if let Some(rid) = mat.recipe {
                    let job = data
                        .recipes
                        .get(&xiv_gen::RecipeId(rid))
                        .and_then(|r| craft_type_info(r.craft_type))
                        .map(|(c, n)| format!("{c} ({n})"))
                        .unwrap_or_else(|| "Craft".to_string());
                    craft_steps.push(PlannedCraftStep {
                        item_id: mat.item,
                        item_name: item_name(mat.item).to_string(),
                        recipe_id: rid,
                        job,
                        crafts_needed: mat.crafts,
                        total_needed: mat.needed,
                        surplus: mat.surplus,
                        recipe_url: recipe_url(rid),
                    });
                } else if mat.remaining() > 0 {
                    let quantity = mat.remaining();
                    let vendor = vendors.get(&mat.item).copied();
                    let market = buy_map.find_matching_listings(mat.item).chosen(false);
                    let (source, unit_price, wname) = match (vendor, market) {
                        (Some(v), Some(m)) if v <= m.price => ("vendor", v, None),
                        (Some(v), None) => ("vendor", v, None),
                        (_, Some(m)) if m.price > 0 => (
                            "market",
                            m.price,
                            Some(world_name_by_id(&world_helper, m.world_id)),
                        ),
                        _ => {
                            missing_price_items += 1;
                            ("unpriced", 0, None)
                        }
                    };
                    let est_total = unit_price as i64 * quantity;
                    total_cost += est_total;
                    shopping_list.push(PlannedShoppingItem {
                        item_id: mat.item,
                        item_name: item_name(mat.item).to_string(),
                        quantity,
                        source,
                        est_unit_price: unit_price,
                        est_total_cost: est_total,
                        cheapest_world: wname,
                        item_url: item_url(&world_name, mat.item),
                    });
                }
            }

            shopping_list.sort_by_key(|a| std::cmp::Reverse(a.est_total_cost));
            Ok((shopping_list, craft_steps, total_cost, missing_price_items))
        })
        .await
        .map_err(|e| WebError::AnyhowError(anyhow::anyhow!("plan_craft task failed: {e}")))?
        .map_err(|reason| {
            tracing::debug!(reason, "plan_craft rejected");
            WebError::BadRequest
        })?;

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
        &format!("Vendor Resale (NQ) — {world}"),
        &env.view_url,
        &env.as_of,
    );
    let _ = writeln!(
        out,
        "| Item | Vendor | NQ Market | Profit | ROI % | Sales/Day |"
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
        "| Leve | Job | Lv | Turn-in Item | Qty | Gil | Item EV | Best Cost (method) | Profit |"
    );
    let _ = writeln!(out, "|---|---|---:|---|---:|---:|---:|---:|---:|");
    for r in &env.data {
        let _ = writeln!(
            out,
            "| {} | {} | {} | [{}]({}) | {} | {} | {} | {} ({}) | {} |",
            r.leve_name,
            r.job,
            r.level,
            r.item_name,
            r.item_url,
            r.quantity_required,
            r.gil_reward,
            r.expected_item_reward_value,
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
        &format!("Venture Analyzer (NQ) — {world}"),
        &env.view_url,
        &env.as_of,
    );
    let _ = writeln!(
        out,
        "| Item | Category | Lv | Qty | NQ Unit Price | Est. Net Value | Sales/Day |"
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
        "| Item | Scrip Type | Acquisition | Job | Scrips | Craft Cost | Gil/Scrip |"
    );
    let _ = writeln!(out, "|---|---|---|---|---:|---:|---:|");
    for r in &env.data {
        let _ = writeln!(
            out,
            "| [{}]({}) | {} | {} | {} | {} | {} | {} |",
            r.item_name,
            r.item_url,
            r.scrip_type,
            r.acquisition,
            r.job.unwrap_or("Gatherer"),
            r.scrip_amount,
            r.craft_cost
                .map(|c| c.to_string())
                .unwrap_or_else(|| "—".into()),
            r.gil_per_scrip
                .map(|g| format!("{g:.2}"))
                .unwrap_or_else(|| "—".into()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tax_helpers_match_ultros_calc_and_preserve_large_i64_values() {
        assert_eq!(
            post_tax_price_i32(1000, true) + tax_amount_i32(1000, true),
            1000
        );
        assert_eq!(post_tax_price_i32(1000, false), 1000);
        assert_eq!(tax_amount_i32(1000, false), 0);

        // Venture gross > i32::MAX must not clamp to i32::MAX before applying tax.
        let big_gross: i64 = 5_000_000_000;
        assert_eq!(post_tax_price_i64(big_gross, true), 4_750_000_000);
        assert_eq!(post_tax_price_i64(big_gross, false), big_gross);
    }

    #[test]
    fn leve_hq_gating_and_whole_batch_cost() {
        // Items that cannot be HQ never get the 2x gil reward even when hq=true is requested.
        assert_eq!(leve_effective_gil_reward(5000, true, false), (false, 5000));
        assert_eq!(leve_effective_gil_reward(5000, true, true), (true, 10000));
        assert_eq!(leve_effective_gil_reward(5000, false, true), (false, 5000));

        // Requiring 4 units of a 3-yield recipe requires 2 full craft batches, not 4/3 prorated.
        assert_eq!(whole_batch_craft_cost(4, 3, 1000), 2000);
        assert_eq!(whole_batch_craft_cost(3, 3, 1000), 1000);
    }

    #[test]
    fn scrip_sort_ranks_priced_crafted_rows_ahead_of_unpriced_gathered_rows() {
        let mut rows = vec![
            PublicScripRow {
                item_id: 1,
                item_name: "Gathered Item".into(),
                scrip_type: "OrangeGatherers".into(),
                scrip_amount: 200,
                reward_tier: "max_collectability",
                acquisition: "gathered",
                job: None,
                recipe_id: None,
                craft_cost: None,
                gil_per_scrip: None,
                item_url: String::new(),
                recipe_url: None,
            },
            PublicScripRow {
                item_id: 2,
                item_name: "Expensive Craft".into(),
                scrip_type: "OrangeCrafters".into(),
                scrip_amount: 100,
                reward_tier: "max_collectability",
                acquisition: "crafted",
                job: Some("CUL"),
                recipe_id: Some(10),
                craft_cost: Some(5000),
                gil_per_scrip: Some(50.0),
                item_url: String::new(),
                recipe_url: None,
            },
            PublicScripRow {
                item_id: 3,
                item_name: "Cheap Craft".into(),
                scrip_type: "OrangeCrafters".into(),
                scrip_amount: 100,
                reward_tier: "max_collectability",
                acquisition: "crafted",
                job: Some("CUL"),
                recipe_id: Some(11),
                craft_cost: Some(1200),
                gil_per_scrip: Some(12.0),
                item_url: String::new(),
                recipe_url: None,
            },
        ];
        sort_scrip_rows(&mut rows);
        let ids: Vec<i32> = rows.iter().map(|r| r.item_id).collect();
        assert_eq!(ids, vec![3, 2, 1]);
    }

    #[test]
    fn unified_craft_plan_batches_shared_multi_yield_subcrafts_once() {
        // Two targets (100, 101) each need 1 unit of intermediate 200 (yield 2 per craft).
        // Combined demand for 200 is 2 -> exactly 1 craft batch (NOT 2 separate batches!).
        let mut recipes = BTreeMap::new();
        recipes.insert(
            1,
            recipe_planner::Recipe {
                id: 1,
                output: 100,
                yield_amount: 1,
                ingredients: vec![(200, 1)],
            },
        );
        recipes.insert(
            2,
            recipe_planner::Recipe {
                id: 2,
                output: 101,
                yield_amount: 1,
                ingredients: vec![(200, 1)],
            },
        );
        recipes.insert(
            3,
            recipe_planner::Recipe {
                id: 3,
                output: 200,
                yield_amount: 2,
                ingredients: vec![(300, 5)],
            },
        );
        let choices = BTreeMap::from([(100, 1), (101, 2), (200, 3)]);
        let expanded = expand_unified_plan(
            &[(100, 1, 1), (101, 1, 2)],
            &recipes,
            &choices,
            true,
            &BTreeSet::new(),
        )
        .expect("plan expands");

        let intermediate = expanded.iter().find(|m| m.item == 200).unwrap();
        assert_eq!(intermediate.needed, 2);
        assert_eq!(intermediate.crafts, 1);
        assert_eq!(intermediate.surplus, 0);

        let raw = expanded.iter().find(|m| m.item == 300).unwrap();
        assert_eq!(raw.needed, 5);
    }

    #[test]
    fn recipe_and_trends_view_urls_match_frontend_query_contract() {
        let q = PublicRecipesQuery {
            use_subcrafts: Some(true),
            require_hq: Some(false),
            exclude_shards: Some(true),
            min_profit: Some(5000),
            min_sales: Some(10),
            min_daily_sales: Some(2.5),
            ..Default::default()
        };
        let cost_url = build_recipes_view_url("Gilgamesh", Some("CUL"), "datacenter", "cost", &q);
        assert!(cost_url.contains("sort=cost"));
        assert!(cost_url.contains("dir=asc"));
        assert!(cost_url.contains("min-sales=2.5"));
        assert!(!cost_url.contains("min-sales=10"));

        let vel_url = build_recipes_view_url(
            "Gilgamesh",
            None,
            "world",
            "daily_sales",
            &PublicRecipesQuery::default(),
        );
        assert!(vel_url.contains("sort=velocity"));
        assert!(vel_url.contains("dir=desc"));

        let trends_url = build_trends_view_url("Gilgamesh", 7, true);
        assert_eq!(
            trends_url,
            "https://ultros.app/trends/Gilgamesh?window=7&show_suspicious=true"
        );
    }
}
