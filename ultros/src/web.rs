mod alerts_websocket;
pub(crate) mod api;
pub(crate) mod country_code_decoder;
pub(crate) mod error;
pub(crate) mod item_card;
pub(crate) mod list_permission;
pub(crate) mod oauth;
pub(crate) mod price_series_cache;
pub(crate) mod sale_stats_cache;
pub(crate) mod sitemap;
pub(crate) mod state;
pub(crate) mod static_files;

use anyhow::Error;
use axum::extract::{Path, Query, State};
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{delete, get, post};
use axum::{Json, Router, middleware};
use axum_extra::extract::CookieJar;
use axum_extra::headers::{CacheControl, HeaderMapExt};
use futures::future::{try_join_all, try_join3};
use hyper::header;
use itertools::Itertools;
use leptos::prelude::provide_context;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tower::ServiceBuilder;
use tower_http::classify::ServerErrorsFailureClass;
use tower_http::compression::predicate::{NotForContentType, SizeAbove};
use tower_http::compression::{CompressionLayer, Predicate};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing::{Span, debug, warn};
use ultros_api_types::list::{
    CreateInvite, CreateList, List, ListActivity, ListActivityKind, ListInvite, ListItem,
    ListSharedGroup, ListSharedUser, ListWithPermission, ShareListGroup, ShareListUser,
};
use ultros_api_types::price_series::{
    HqFilter, PriceBucket, PriceSeries, PriceSeriesEntry, SeriesGroup,
};
use ultros_api_types::retainer::RetainerListings;
use ultros_api_types::user::group::{
    CreateGroup, CreateGroupFromGuild, CreateGroupInvite, DiscordManageableGuild, GroupInvite,
    UserGroup, UserGroupMember,
};
use ultros_api_types::user::{
    AssignRetainerCharacter, OwnedRetainer, UserData, UserRetainerListings, UserRetainers,
};
use ultros_api_types::websocket::{ListEventData, ListingEventData};
use ultros_api_types::world::WorldData;
use ultros_api_types::{
    ActiveListing, CompactSale, CurrentlyShownItem, ExtendedSaleHistory, FfxivCharacter, Retainer,
    WorldItemLastUpdated,
};
use ultros_app::{LocalWorldData, shell};
use ultros_charts::data::buckets::{
    bucket_seconds_for_span, narrow_bucket_for_actual_span, snap_bucket_seconds, widen_bucket,
};
use ultros_clickhouse::ClickHouseClient;
use ultros_clickhouse::queries::PriceSeriesRow;
use ultros_db::ActiveValue;
use ultros_db::world_data::world_cache::{AnyResult, AnySelector};
use ultros_db::{UltrosDb, world_data::world_cache::WorldCache};
use universalis::{ItemId, ListingView, UniversalisClient, WorldId};

use crate::character_claim::CharacterClaimService;

use self::country_code_decoder::Region;
use self::error::{ApiError, WebError};
use self::oauth::{AuthDiscordUser, AuthUserCache};
use crate::alerts::price_alert_tracker::resolve_item_name;
use crate::event::{EventSenders, EventType};
use crate::leptos::create_leptos_app;
use crate::search_service::SearchService;
use crate::web::api::alerts::{
    create_alert, delete_alert, list_alert_events, list_alerts, resend_alert_event, update_alert,
};
use crate::web::api::endpoints::{
    create_endpoint, delete_endpoint, list_discord_writable_guilds, list_endpoints, test_endpoint,
    update_endpoint,
};
use crate::web::api::real_time_data::real_time_data;
use crate::web::api::{
    cheapest_per_world, get_best_deals, get_item_stats, get_market_heat, get_market_pulse,
    get_movers, get_sale_stats, get_trends, post_resale_quality, post_sparklines, recent_sales,
};
use crate::web::sitemap::{generic_pages_sitemap, item_sitemap, sitemap_index};
use crate::web::{
    alerts_websocket::connect_websocket,
    item_card::item_card,
    oauth::{begin_login, logout},
};
use crate::web_metrics::{start_metrics_server, track_metrics};

fn legacy_book_help_path(path: &str) -> &'static str {
    match path.trim_end_matches(".html").trim_end_matches('/') {
        "" | "/" | "/intro/intro" | "/intro/homeworld" => "/help/getting-started",
        "/search/search" | "/item_explorer" => "/help/getting-started",
        "/retainers/retainers"
        | "/retainers/managing"
        | "/retainers/viewing"
        | "/retainers/alerts"
        | "/characters/characters"
        | "/characters/add_character" => "/help/lists-alerts-retainers",
        "/lists/lists" | "/lists/import_makeplace" => "/help/lists-alerts-retainers",
        "/analyzer/analyzer" => "/help/flip-finder",
        "/analyzer/recipe" => "/help/recipe-analyzer",
        "/analyzer/leve" => "/help/leve-analyzer",
        "/currency/exchange" => "/help/scrip-sources",
        _ => "/help",
    }
}

/// Send a list event; log at warn level if delivery fails. Send errors
/// here are best-effort — they only matter for observability, so they
/// must never propagate into handler results.
fn send_list_event(
    senders: &EventSenders,
    event: crate::event::EventType<std::sync::Arc<ultros_api_types::websocket::ListEventData>>,
) {
    if let Err(e) = senders.lists.send(event) {
        warn!(error = %e, "failed to broadcast list event");
    }
}

#[allow(clippy::too_many_arguments)]
async fn record_list_activity(
    db: &UltrosDb,
    senders: &EventSenders,
    list_id: i32,
    user: &AuthDiscordUser,
    kind: ListActivityKind,
    list_item_id: Option<i32>,
    item_id: Option<i32>,
    payload: serde_json::Value,
    message: String,
) -> Result<ListActivity, ApiError> {
    db.get_or_create_discord_user(user.id, user.name.clone())
        .await?;
    let activity = db
        .record_list_activity(
            list_id,
            user.id as i64,
            user.name.clone(),
            kind,
            list_item_id,
            item_id,
            payload,
            message,
        )
        .await?;
    let activity = ListActivity::from(activity);
    send_list_event(
        senders,
        EventType::added(ListEventData::Activity(activity.clone())),
    );
    Ok(activity)
}

fn item_change_payload(
    before: &ultros_db::entity::list_item::Model,
    after: &ultros_db::entity::list_item::Model,
) -> serde_json::Value {
    let mut changes = serde_json::Map::new();
    if before.hq != after.hq {
        changes.insert("hq".to_string(), serde_json::json!([before.hq, after.hq]));
    }
    if before.quantity != after.quantity {
        changes.insert(
            "quantity".to_string(),
            serde_json::json!([before.quantity, after.quantity]),
        );
    }
    if before.acquired != after.acquired {
        changes.insert(
            "acquired".to_string(),
            serde_json::json!([before.acquired, after.acquired]),
        );
    }
    if before.target_price != after.target_price {
        changes.insert(
            "target_price".to_string(),
            serde_json::json!([before.target_price, after.target_price]),
        );
    }
    serde_json::Value::Object(changes)
}

async fn redirect_legacy_book_host(
    req: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let is_book_host = req
        .headers()
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .map(|host| host.split(':').next().unwrap_or(host))
        .map(|host| host.eq_ignore_ascii_case("book.ultros.app"))
        .unwrap_or(false);

    if is_book_host {
        let target = legacy_book_help_path(req.uri().path());
        Redirect::permanent(&format!("https://ultros.app{target}")).into_response()
    } else {
        next.run(req).await
    }
}

async fn add_retainer(
    State(db): State<UltrosDb>,
    current_user: AuthDiscordUser,
    Path(retainer_id): Path<i32>,
) -> Result<Redirect, ApiError> {
    let _register_retainer = db
        .register_retainer(retainer_id, current_user.id, current_user.name)
        .await?;
    Ok(Redirect::to("/retainers/edit"))
}

async fn remove_owned_retainer(
    State(db): State<UltrosDb>,
    current_user: AuthDiscordUser,
    Path(retainer_id): Path<i32>,
) -> Result<Redirect, WebError> {
    db.remove_owned_retainer(current_user.id, retainer_id)
        .await?;
    Ok(Redirect::to("/retainers/edit"))
}

#[tracing::instrument(skip(db, world_cache))]
async fn world_item_listings(
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    Path((world, item_id)): Path<(String, i32)>,
) -> Result<axum::Json<CurrentlyShownItem>, WebError> {
    let selected_value = world_cache.lookup_value_by_name(&world)?;
    let worlds = world_cache
        .get_all_worlds_in(&selected_value)
        .ok_or_else(|| Error::msg("Unable to get worlds"))?;
    let db_clone = db.clone();
    let db_clone_2 = db.clone();
    let world_iter = worlds.iter().copied();
    let (listings, sales, last_updated) = try_join3(
        db_clone.get_all_listings_in_worlds_with_retainers(&worlds, ItemId(item_id)),
        db.get_sale_history_from_multiple_worlds(world_iter, item_id, 200),
        db_clone_2.get_listing_last_updated_for_worlds(ItemId(item_id), &worlds),
    )
    .await
    .inspect_err(|e| tracing::error!(error = ?e, "Error getting listings"))?;
    let currently_shown = CurrentlyShownItem {
        listings: listings
            .into_iter()
            .flat_map(|(l, r)| r.map(|r| (l.into(), r.into())))
            .collect(),
        sales: sales.into_iter().map(|s| s.into()).collect(),
        last_updated: last_updated
            .into_iter()
            .map(|updated| WorldItemLastUpdated {
                world_id: updated.world_id,
                updated_at: updated.date_time,
            })
            .collect(),
    };
    Ok(axum::Json(currently_shown))
}

/// Compact extended sale history for charting. Returns up to `limit` rows (default
/// 1000, capped at 10000) of price/quantity/timestamp/world/hq — no buyer metadata.
/// Auto-loaded by the price chart on the client after hydration.
#[tracing::instrument(skip(db, world_cache))]
async fn extended_sale_history(
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    Path((world, item_id)): Path<(String, i32)>,
    axum::extract::Query(query): axum::extract::Query<ExtendedHistoryQuery>,
) -> Result<axum::Json<ExtendedSaleHistory>, WebError> {
    const DEFAULT_LIMIT: u64 = 1_000;
    const MAX_LIMIT: u64 = 10_000;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let selected_value = world_cache.lookup_value_by_name(&world)?;
    let worlds = world_cache
        .get_all_worlds_in(&selected_value)
        .ok_or_else(|| Error::msg("Unable to get worlds"))?;
    let sales = db
        .get_compact_sale_history(worlds.iter().copied(), item_id, limit)
        .await
        .inspect_err(|e| tracing::error!(error = ?e, "Error getting extended sales"))?;
    let response = ExtendedSaleHistory {
        sales: sales
            .into_iter()
            .map(|s| CompactSale {
                quantity: s.quantity,
                price_per_item: s.price_per_item,
                hq: s.hq,
                sold_date: s.sold_date,
                world_id: s.world_id,
            })
            .collect(),
    };
    Ok(axum::Json(response))
}

#[derive(serde::Deserialize, Debug)]
struct ExtendedHistoryQuery {
    limit: Option<u64>,
}

#[derive(serde::Deserialize, Debug)]
struct PriceSeriesQuery {
    from: Option<i64>,
    to: Option<i64>,
    bucket: Option<i64>,
    group: Option<String>,
    hq: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
struct PriceDensityQuery {
    from: Option<i64>,
    to: Option<i64>,
    bucket: Option<i64>,
    hq: Option<String>,
    price_bins: Option<u16>,
}

/// Above this many sales in the window we stop shipping raw rows and the
/// chart draws buckets only. Raw dots become a zoomed-in affordance rather
/// than a default — which is the entire point of this endpoint.
const RAW_SALE_LIMIT: u64 = 2_000;

/// Target ceiling on buckets across all series in one response. Exceeding it
/// widens the bucket a ladder step and retries rather than truncating —
/// truncation would silently drop the oldest data, which is the data the
/// caller asked for. This is enforced only while the widening ladder has
/// room: once `widen_bucket` can't widen any further (the widest step is
/// already in use), the loop ships the oversized response as-is instead of
/// truncating or erroring. Reaching that point needs a pathological request
/// — decades of range fanned out across many series — but when it happens,
/// a large response beats 400ing a legitimate request.
const MAX_BUCKETS: usize = 20_000;

/// Map every world in scope to its series key at `group`.
///
/// `WorldCache::get_all_worlds_in` yields bare world ids, so coarser groupings
/// resolve each one through the cache. Worlds that fail to resolve are dropped
/// rather than mapped to a sentinel — a sentinel would silently merge them into
/// one bogus series. This list must stay in sync with the `world_id IN (...)`
/// filter the query builds from it; see the invariant note on `group_expr` in
/// `ultros_clickhouse::queries`.
fn world_group_map(
    world_cache: &WorldCache,
    world_ids: &[i32],
    group: SeriesGroup,
) -> Vec<(i32, i32)> {
    if group == SeriesGroup::World {
        return world_ids.iter().map(|&id| (id, id)).collect();
    }
    world_ids
        .iter()
        .filter_map(|&world_id| {
            let result = world_cache
                .lookup_selector(&AnySelector::World(world_id))
                .ok()?;
            let world = result.as_world().ok()?;
            let key = match group {
                SeriesGroup::World => world.id,
                SeriesGroup::Datacenter => world.datacenter_id,
                SeriesGroup::Region => {
                    match world_cache
                        .lookup_selector(&AnySelector::Datacenter(world.datacenter_id))
                        .ok()?
                    {
                        AnyResult::Datacenter(dc) => dc.region_id,
                        // `lookup_selector(&AnySelector::Datacenter(_))` always
                        // returns `AnyResult::Datacenter` or an `Err` (handled
                        // by the `?` above); this arm is unreachable but kept
                        // exhaustive rather than matched with an `unwrap`.
                        _ => return None,
                    }
                }
            };
            Some((world_id, key))
        })
        .collect()
}

/// Groups consecutive rows sharing `series_id` into one [`PriceSeriesEntry`]
/// each.
///
/// This relies on `ultros_clickhouse::queries::price_series` returning rows
/// ordered by `(series_id, bucket)` — a single linear pass appending to the
/// last entry is only correct because rows for the same series are
/// guaranteed contiguous. It does **not** re-sort or group by a `HashMap`.
/// If the query's `ORDER BY` is ever changed, this function must change
/// with it (or be rewritten to group defensively), otherwise interleaved
/// rows for one series would silently split into multiple entries.
fn fold_price_series_rows(rows: &[PriceSeriesRow]) -> Vec<PriceSeriesEntry> {
    let mut entries: Vec<PriceSeriesEntry> = Vec::new();
    for row in rows {
        let bucket = PriceBucket {
            ts: row.bucket.naive_utc(),
            open: row.open as i32,
            high: row.high as i32,
            low: row.low as i32,
            close: row.close as i32,
            gil: row.gil as i64,
            units: row.units as i64,
            sales: row.sales as u32,
            p25: row.p25 as i32,
            p50: row.p50 as i32,
            p75: row.p75 as i32,
        };
        match entries.last_mut() {
            Some(entry) if entry.id == row.series_id => entry.buckets.push(bucket),
            _ => entries.push(PriceSeriesEntry {
                id: row.series_id,
                buckets: vec![bucket],
            }),
        }
    }
    entries
}

/// Resolve the bucket width to query at: an explicit, valid request wins
/// (snapped onto the ladder), otherwise the ladder picks one from the span.
/// Shared by the handler (which needs this *before* the widening loop, to
/// build a stable cache key) and [`build_price_series`] (which needs it as
/// the loop's starting point) so the two can't drift apart.
fn resolve_bucket_seconds(bucket: Option<i64>, span_secs: i64) -> i64 {
    match bucket {
        Some(requested) if requested > 0 => snap_bucket_seconds(requested),
        _ => bucket_seconds_for_span(span_secs),
    }
}

/// How long a cached response stays servable, and — for an open-ended window
/// — the grain [`open_window_cache_stamp`] quantizes its cache key onto.
/// Deriving both from one place means exactly one entry per item/scope is live
/// at a time: the key rolls over on the same schedule the entry expires on.
///
/// Capped at an hour so an open window is never served staler than that, and
/// floored at a minute so a hypothetical sub-minute bucket couldn't turn the
/// cache into a no-op. A closed window is immutable, so it just takes the cap.
fn cache_ttl_secs(closed: bool, bucket_seconds: i64) -> u64 {
    if closed {
        3_600
    } else {
        (bucket_seconds as u64).clamp(60, 3_600)
    }
}

/// Quantize an open-ended window's end onto a `grain`-second grid, for use in
/// the **cache key only** — never for the window actually queried.
///
/// An open-ended request ends at "now", so feeding that raw timestamp into the
/// cache key mints a fresh entry every second and the cache never hits.
/// Rounding it onto the same grid as the entry's TTL keeps one live entry per
/// item/scope, which is all the quantization was ever for.
///
/// This deliberately moves the *key* and not the queried window. Flooring the
/// window itself — which both handlers used to do, at `bucket_seconds`
/// granularity — drags the query's exclusive upper bound backwards, excluding
/// every sale after the boundary. An open-ended "full history" request
/// resolves to a 12-year span, the ladder duly picks its widest step (30
/// days), and so the newest 0–30 days of sales silently vanished from every
/// chart. Serving a slightly stale snapshot is the cache's job and is bounded
/// by the TTL; narrowing the window is data loss and is not.
fn open_window_cache_stamp(to_ts: i64, grain: i64) -> i64 {
    let grain = grain.max(1);
    to_ts - to_ts.rem_euclid(grain)
}

/// Uniform bin height covering `[lo, hi]` inclusive in `bins` steps, floored
/// at 1 gil so degenerate windows (every sale at one price) still bin sanely.
fn density_bin_width(lo: u32, hi: u32, bins: u16) -> f64 {
    (((hi - lo) as f64 + 1.0) / bins as f64).max(1.0)
}

/// Request shape for [`build_price_series`], bundled into one struct so the
/// function stays under clippy's argument-count lint — `ch`/`world_cache`
/// stay separate since they're handles, not request data.
pub(crate) struct PriceSeriesArgs<'a> {
    /// A world, datacenter, or region name; expanded to its constituent
    /// worlds via `world_cache`.
    pub world: &'a str,
    pub item_id: i32,
    pub from: chrono::DateTime<chrono::Utc>,
    pub to: chrono::DateTime<chrono::Utc>,
    pub group: SeriesGroup,
    pub hq: HqFilter,
    /// Mirrors the JSON endpoint's `bucket` query param: `Some(n)` with
    /// `n > 0` snaps `n` onto the ladder and starts the widening loop there;
    /// anything else (including `None`) lets [`bucket_seconds_for_span`]
    /// pick from `[from, to)`.
    pub bucket: Option<i64>,
}

/// Resolves a `PriceSeries` for `args.item_id` within `[args.from, args.to)`
/// at `args.group` granularity, filtered by `args.hq`, scoped to the worlds
/// `args.world` (a world, datacenter, or region name) expands to.
///
/// Shared by the `/api/v1/price_series` JSON endpoint and the item-card PNG
/// so the two can never disagree about what the chart shows: same world
/// resolution, same bucket-widening ladder, same raw-sale cutoff
/// ([`RAW_SALE_LIMIT`]), same response-domain calculation.
pub(crate) async fn build_price_series(
    ch: &ClickHouseClient,
    world_cache: &WorldCache,
    args: PriceSeriesArgs<'_>,
) -> Result<PriceSeries, WebError> {
    let PriceSeriesArgs {
        world,
        item_id,
        from,
        to,
        group,
        hq,
        bucket,
    } = args;
    if from >= to {
        return Err(WebError::BadRequest);
    }

    let selected_value = world_cache.lookup_value_by_name(world)?;
    let worlds = world_cache
        .get_all_worlds_in(&selected_value)
        .ok_or_else(|| Error::msg("Unable to get worlds"))?;

    let world_to_group = world_group_map(world_cache, &worlds, group);

    let span_secs = (to - from).num_seconds().max(1);
    let mut bucket_seconds = resolve_bucket_seconds(bucket, span_secs);

    // The starting width is derived from the *requested* span, which for an
    // open-ended "full history" request is years — while the data may only
    // cover months. At that mismatch the ladder picks 30-day buckets and the
    // whole history collapses into one or two points. So after the first
    // pass, re-derive the width from the span the rows actually cover and
    // re-query once if the ladder picks a narrower step (`may_narrow` keeps
    // this to a single extra query; the inner loop still widens whenever a
    // response would exceed MAX_BUCKETS).
    let mut may_narrow = true;
    let rows = loop {
        let rows = loop {
            let rows = ultros_clickhouse::queries::price_series(
                ch,
                item_id,
                &world_to_group,
                group,
                hq,
                from,
                to,
                bucket_seconds,
            )
            .await
            .map_err(|e| {
                tracing::warn!(error = ?e, item_id, "price_series CH query failed");
                crate::web::error::ClickHouseQueryError::new("price_series", e)
            })?;

            if rows.len() <= MAX_BUCKETS {
                break rows;
            }
            match widen_bucket(bucket_seconds) {
                Some(wider) => bucket_seconds = wider,
                // Already at the top of the ladder: ship what we have rather
                // than looping forever.
                None => break rows,
            }
        };

        if may_narrow {
            may_narrow = false;
            let first = rows.iter().map(|r| r.bucket).min();
            let last = rows.iter().map(|r| r.bucket).max();
            if let (Some(first), Some(last)) = (first, last) {
                // Bucket timestamps are starts, so the last bucket extends
                // one width past its own ts.
                let actual_span = (last - first).num_seconds() + bucket_seconds;
                if let Some(narrower) = narrow_bucket_for_actual_span(actual_span, bucket_seconds) {
                    bucket_seconds = narrower;
                    continue;
                }
            }
        }
        break rows;
    };

    let total_sales: u64 = rows.iter().map(|r| r.sales).sum();
    let series = fold_price_series_rows(&rows);

    let raw = if total_sales > 0 && total_sales <= RAW_SALE_LIMIT {
        // Sourced from ClickHouse, not `UltrosDb::get_compact_sale_history`:
        // that Postgres query has no date bound (most recent `limit` sales
        // per world *as of now*), so filtering it to an arbitrary [from, to)
        // window client-side silently comes back empty whenever `to` isn't
        // near "now" — exactly what this endpoint's `from`/`to` query params
        // support. `raw_sales` uses the identical WHERE shape as the bucket
        // query above (same item/worlds/window/hq), so the dots can never
        // disagree with the buckets about what's in the window. This also
        // means `hq` now filters raw sales too, matching the buckets (it
        // previously didn't).
        let sales = ultros_clickhouse::queries::raw_sales(
            ch,
            item_id,
            &worlds,
            hq,
            from,
            to,
            RAW_SALE_LIMIT,
        )
        .await
        .map_err(|e| {
            tracing::warn!(error = ?e, item_id, "price_series raw_sales CH query failed");
            crate::web::error::ClickHouseQueryError::new("raw_sales", e)
        })?;
        Some(
            sales
                .into_iter()
                .map(|s| CompactSale {
                    quantity: s.quantity as i32,
                    price_per_item: s.price_per_item as i32,
                    hq: s.hq != 0,
                    sold_date: s.sold_date.naive_utc(),
                    world_id: s.world_id,
                })
                .collect(),
        )
    } else {
        None
    };

    // The response domain is the actual data span, not the requested one —
    // falls back to the requested bounds when there are no rows at all.
    let domain_from = series
        .iter()
        .flat_map(|e| e.buckets.iter())
        .map(|b| b.ts)
        .min()
        .unwrap_or_else(|| from.naive_utc());
    let domain_to = series
        .iter()
        .flat_map(|e| e.buckets.iter())
        .map(|b| b.ts)
        .max()
        .unwrap_or_else(|| to.naive_utc());

    Ok(PriceSeries {
        bucket_seconds,
        group,
        from: domain_from,
        to: domain_to,
        series,
        raw,
    })
}

/// `GET /api/v1/price_series/{world}/{itemid}` — server-bucketed price/volume
/// series backing the item page chart, replacing the browser-side bucketing
/// of up to 10,000 raw rows from `extended_sale_history`.
///
/// Named `price_series` like the query function it wraps
/// (`ultros_clickhouse::queries::price_series`); calls into that function are
/// fully qualified to disambiguate.
///
/// Caching and serialization live here; the actual series construction is
/// [`build_price_series`], shared with the item-card PNG handler.
async fn price_series(
    State(world_cache): State<Arc<WorldCache>>,
    State(ch): State<ClickHouseClient>,
    State(cache): State<crate::web::price_series_cache::PriceSeriesCache>,
    Path((world, item_id)): Path<(String, i32)>,
    axum::extract::Query(query): axum::extract::Query<PriceSeriesQuery>,
) -> Result<axum::response::Response, WebError> {
    let group = match query.group.as_deref() {
        Some("region") => SeriesGroup::Region,
        Some("datacenter") => SeriesGroup::Datacenter,
        _ => SeriesGroup::World,
    };
    let hq = match query.hq.as_deref() {
        Some("hq") => HqFilter::Hq,
        Some("nq") => HqFilter::Nq,
        _ => HqFilter::Any,
    };

    let now = chrono::Utc::now();
    let to = query
        .to
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .unwrap_or(now);
    let from = query
        .from
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .unwrap_or_else(|| now - chrono::Duration::days(365 * 12));
    if from >= to {
        return Err(WebError::BadRequest);
    }

    let span_secs = (to - from).num_seconds().max(1);
    let bucket_seconds = resolve_bucket_seconds(query.bucket, span_secs);

    // A closed window is immutable; an open one is a snapshot of "now" and
    // stays servable until its TTL expires.
    let ttl_secs = cache_ttl_secs(query.to.is_some(), bucket_seconds);
    let ttl = std::time::Duration::from_secs(ttl_secs);

    // `to` itself is left at `now`: only the cache key is quantized, so live
    // views still share an entry without the query window losing its newest
    // sales. See [`open_window_cache_stamp`] for why flooring `to` is a bug.
    let cache_to = if query.to.is_none() {
        open_window_cache_stamp(to.timestamp(), ttl_secs as i64)
    } else {
        to.timestamp()
    };

    // The cache key is built from the *pre-widening* `bucket_seconds` — the
    // value resolved above from the request, before `build_price_series`'s
    // internal loop potentially widens (or narrows) it in response to how
    // much data comes back. This is deliberate: checking the cache has to happen before
    // running the query at all (that's the entire point — skip the CH scan
    // on a hit), and the widened bucket is only known *after* the query
    // runs. Building the key post-query would mean always querying first,
    // defeating the cache.
    //
    // The tradeoff: if a client takes the `bucket_seconds` reported in a
    // widened response and re-requests with that as an explicit `bucket=`
    // query param, it computes a different cache key than the original
    // request and misses even though the same rows are cached under the
    // pre-widen key. This is a pure cache miss, not a correctness bug — the
    // re-request still runs the same query, widens to the same bucket, and
    // produces the same (correct) answer, just without benefiting from the
    // cache. Given widening only triggers on pathologically large requests
    // (see `MAX_BUCKETS`), this is rare enough not to be worth doubling the
    // number of cache entries written per request to cover it.
    let cache_key = crate::web::price_series_cache::CacheKey {
        item_id,
        scope: world.clone(),
        from: from.timestamp(),
        to: cache_to,
        bucket: bucket_seconds,
        group: group.as_str(),
        hq: hq.as_str(),
        bins: 0,
    };
    if let Some(hit) = cache.get(&cache_key) {
        return Ok(cached_json(hit, ttl));
    }

    let payload = build_price_series(
        &ch,
        &world_cache,
        PriceSeriesArgs {
            world: &world,
            item_id,
            from,
            to,
            group,
            hq,
            bucket: Some(bucket_seconds),
        },
    )
    .await?;

    let body = serde_json::to_string(&payload).map_err(anyhow::Error::from)?;
    cache.insert(cache_key, body.clone(), ttl);
    Ok(cached_json(body, ttl))
}

/// JSON response carrying a `Cache-Control` matching the in-process TTL, so
/// the browser and any CDN absorb repeats too.
fn cached_json(body: String, ttl: std::time::Duration) -> axum::response::Response {
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/json".to_string(),
            ),
            (
                axum::http::header::CACHE_CONTROL,
                format!("public, max-age={}", ttl.as_secs()),
            ),
        ],
        body,
    )
        .into_response()
}

#[derive(serde::Deserialize, Debug)]
struct GameHistoryQuery {
    track: Option<String>,
}

/// `GET /api/v1/game-history` — the patch/expansion release calendar
/// backing the chart's milestone bands. The WASM chart reads the seed table
/// directly from `ultros_api_types::game_history` (no round trip); this
/// endpoint exists for external consumers and as the future seam where a
/// Postgres-backed table could override the seed. A few KB, changes ~4
/// times a year, hence the day-long `Cache-Control`.
async fn game_history(
    axum::extract::Query(query): axum::extract::Query<GameHistoryQuery>,
) -> Result<axum::response::Response, WebError> {
    use ultros_api_types::game_history::{GAME_PATCHES, PatchTrack};
    let track = match query.track.as_deref() {
        Some("global") => Some(PatchTrack::Global),
        Some("china") => Some(PatchTrack::China),
        Some("korea") => Some(PatchTrack::Korea),
        Some(_) => return Err(WebError::BadRequest),
        None => None,
    };
    let patches: Vec<_> = GAME_PATCHES
        .iter()
        .filter(|p| track.is_none_or(|t| p.track == t))
        .collect();
    let body = serde_json::to_string(&patches).map_err(anyhow::Error::from)?;
    Ok(cached_json(body, std::time::Duration::from_secs(86_400)))
}

/// `GET /api/v1/price_density/{world}/{itemid}` — sale counts on a
/// time × price grid for the chart's density mode. Same window/HQ semantics,
/// bucket ladder, cache, and `Cache-Control` plumbing as [`price_series`];
/// the payload is bounded by `buckets × price_bins` regardless of volume.
///
/// Named `price_density` like the query function it wraps; calls into
/// `ultros_clickhouse::queries` are fully qualified to disambiguate.
async fn price_density(
    State(world_cache): State<Arc<WorldCache>>,
    State(ch): State<ClickHouseClient>,
    State(cache): State<crate::web::price_series_cache::PriceSeriesCache>,
    Path((world, item_id)): Path<(String, i32)>,
    axum::extract::Query(query): axum::extract::Query<PriceDensityQuery>,
) -> Result<axum::response::Response, WebError> {
    let hq = match query.hq.as_deref() {
        Some("hq") => HqFilter::Hq,
        Some("nq") => HqFilter::Nq,
        _ => HqFilter::Any,
    };
    let bins = query.price_bins.unwrap_or(32).clamp(8, 96);

    let now = chrono::Utc::now();
    let to = query
        .to
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .unwrap_or(now);
    let from = query
        .from
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .unwrap_or_else(|| now - chrono::Duration::days(365 * 12));
    if from >= to {
        return Err(WebError::BadRequest);
    }

    let span_secs = (to - from).num_seconds().max(1);
    let mut bucket_seconds = resolve_bucket_seconds(query.bucket, span_secs);
    // Unlike price_series there is no post-query widening loop: the grid's
    // time-axis bucket count is exactly span / width, known up front, so
    // widen arithmetically until it fits under MAX_BUCKETS.
    while span_secs / bucket_seconds > MAX_BUCKETS as i64 {
        match widen_bucket(bucket_seconds) {
            Some(wider) => bucket_seconds = wider,
            None => break,
        }
    }

    // Quantize an open-ended `to` for the cache key only — same rationale, and
    // same data-loss trap, as price_series.
    let ttl_secs = cache_ttl_secs(query.to.is_some(), bucket_seconds);
    let ttl = std::time::Duration::from_secs(ttl_secs);
    let cache_to = if query.to.is_none() {
        open_window_cache_stamp(to.timestamp(), ttl_secs as i64)
    } else {
        to.timestamp()
    };

    let cache_key = crate::web::price_series_cache::CacheKey {
        item_id,
        scope: world.clone(),
        from: from.timestamp(),
        to: cache_to,
        bucket: bucket_seconds,
        group: "density",
        hq: hq.as_str(),
        bins,
    };
    if let Some(hit) = cache.get(&cache_key) {
        return Ok(cached_json(hit, ttl));
    }

    let selected_value = world_cache.lookup_value_by_name(&world)?;
    let worlds = world_cache
        .get_all_worlds_in(&selected_value)
        .ok_or_else(|| Error::msg("Unable to get worlds"))?;

    let extent = ultros_clickhouse::queries::price_min_max(&ch, item_id, &worlds, hq, from, to)
        .await
        .map_err(|e| {
            tracing::warn!(error = ?e, item_id, "price_density min_max CH query failed");
            crate::web::error::ClickHouseQueryError::new("price_min_max", e)
        })?;

    let payload = match extent {
        None => ultros_api_types::price_density::PriceDensity {
            bucket_seconds,
            from: from.naive_utc(),
            to: to.naive_utc(),
            price_lo: 0,
            bin_width: 1.0,
            price_bins: bins,
            cells: Vec::new(),
        },
        Some((lo, hi)) => {
            let bin_width = density_bin_width(lo, hi, bins);
            let rows = ultros_clickhouse::queries::price_density(
                &ch,
                item_id,
                &worlds,
                hq,
                from,
                to,
                bucket_seconds,
                lo,
                bin_width,
                bins,
            )
            .await
            .map_err(|e| {
                tracing::warn!(error = ?e, item_id, "price_density CH query failed");
                crate::web::error::ClickHouseQueryError::new("price_density", e)
            })?;
            ultros_api_types::price_density::PriceDensity {
                bucket_seconds,
                from: from.naive_utc(),
                to: to.naive_utc(),
                price_lo: lo as i32,
                bin_width,
                price_bins: bins,
                cells: rows
                    .into_iter()
                    .map(|r| ultros_api_types::price_density::DensityCell {
                        ts: r.bucket.naive_utc(),
                        bin: r.price_bin,
                        n: u32::try_from(r.n).unwrap_or(u32::MAX),
                    })
                    .collect(),
            }
        }
    };

    let body = serde_json::to_string(&payload).map_err(anyhow::Error::from)?;
    cache.insert(cache_key, body.clone(), ttl);
    Ok(cached_json(body, ttl))
}

/// How loudly `TraceLayer`'s `on_failure` should report a failed response.
///
/// `Error` is what the `sentry_tracing` layer turns into a GlitchTip issue, so
/// this decides what lands in the backlog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureReportLevel {
    Debug,
    Warn,
    Error,
}

/// `on_failure` fires *in addition to* whatever produced the response, so a
/// 5xx that came from a [`WebError`]/[`ApiError`] has already been reported —
/// with its error type, its typed title, and its breadcrumbs
/// ([`error::report_title`]). This layer only sees a bare status code and a
/// latency, so re-reporting it at `error!` buys nothing and costs double:
/// every incident lands in the backlog twice, once as an actionable issue and
/// once as a content-free `"response failed"`.
///
/// The 2026-08-23 ClickHouse outage is the worked example — each failing item
/// card produced a `"Returning web error"` *and* a `"response failed"` under
/// the same trace id, and because the reporter groups by request URL, one
/// outage splintered into dozens of count-1 issues of both kinds.
///
/// So:
/// - **503** stays at `debug` — the analyzer's warm-up window is a transient
///   startup state, not a bug (issues 5033/5034).
/// - Any other **status code** drops to `warn`: still in the logs, no longer a
///   duplicate issue.
/// - A [`ServerErrorsFailureClass::Error`] stays at `error`. That class is a
///   transport- or body-level failure with no response behind it, so *nothing
///   else reports it* — this layer is the only witness.
fn failure_report_level(class: &ServerErrorsFailureClass) -> FailureReportLevel {
    match class {
        ServerErrorsFailureClass::StatusCode(status)
            if *status == hyper::StatusCode::SERVICE_UNAVAILABLE =>
        {
            FailureReportLevel::Debug
        }
        ServerErrorsFailureClass::StatusCode(_) => FailureReportLevel::Warn,
        ServerErrorsFailureClass::Error(_) => FailureReportLevel::Error,
    }
}

#[cfg(test)]
mod failure_report_level_tests {
    use super::*;

    /// Warm-up 503s never reach the backlog.
    #[test]
    fn service_unavailable_stays_quiet() {
        assert_eq!(
            failure_report_level(&ServerErrorsFailureClass::StatusCode(
                hyper::StatusCode::SERVICE_UNAVAILABLE
            )),
            FailureReportLevel::Debug
        );
    }

    /// Regression test for the duplicate reporting the 2026-08-23 ClickHouse
    /// outage exposed: the 500 is already reported by `WebError`, so this
    /// layer must not report it a second time.
    #[test]
    fn internal_server_error_is_not_reported_twice() {
        assert_eq!(
            failure_report_level(&ServerErrorsFailureClass::StatusCode(
                hyper::StatusCode::INTERNAL_SERVER_ERROR
            )),
            FailureReportLevel::Warn,
            "the error type already reported this one with a typed title"
        );
    }

    /// A transport/body failure has no response behind it, so no error type
    /// reported it — this layer is the only place it can surface.
    #[test]
    fn transport_failures_are_still_reported() {
        assert_eq!(
            failure_report_level(&ServerErrorsFailureClass::Error(
                "connection reset".to_string()
            )),
            FailureReportLevel::Error
        );
    }
}

#[cfg(test)]
mod price_series_tests {
    use super::*;

    #[test]
    fn density_bin_width_covers_the_inclusive_range() {
        // [100, 400] over 4 bins -> width 75.25 (301 distinct prices).
        assert_eq!(density_bin_width(100, 400, 4), 301.0 / 4.0);
        // Degenerate flat price: floor at 1.0 so floor((p-lo)/w) stays 0.
        assert_eq!(density_bin_width(100, 100, 32), 1.0);
    }

    /// 2026-08-01T12:00:00Z — an arbitrary but fixed "now" so these tests
    /// don't depend on when they run.
    const NOW: i64 = 1_785_585_600;

    /// The whole point of quantizing: requests seconds apart must land on one
    /// cache entry rather than minting a key each.
    #[test]
    fn cache_stamp_is_stable_across_the_grain() {
        let grain = cache_ttl_secs(false, 30 * 86_400) as i64;
        let base = open_window_cache_stamp(NOW, grain);
        for offset in [0, 1, 59, 600, grain - 1] {
            assert_eq!(
                open_window_cache_stamp(NOW + offset, grain),
                base,
                "+{offset}s should still hit the same cache entry"
            );
        }
        assert_ne!(
            open_window_cache_stamp(NOW + grain, grain),
            base,
            "the key must roll over once the entry expires"
        );
    }

    /// Regression, and the reason this function exists at all.
    ///
    /// An open-ended "full history" request (the item page's default — no
    /// `from`, no `to`) resolves `from` to 12 years back, which puts the
    /// bucket ladder at its widest step. Both handlers used to floor the
    /// *queried* window's exclusive upper bound onto that step, so every sale
    /// in the current bucket — up to a month of the newest data — was
    /// excluded from the response. Pin that the quantization applied now is
    /// bounded by the TTL instead of the bucket width, at every ladder step.
    #[test]
    fn cache_stamp_never_discards_more_than_the_ttl() {
        let span_secs = 365 * 12 * 86_400;
        assert_eq!(
            resolve_bucket_seconds(None, span_secs),
            30 * 86_400,
            "full history sits on the widest rung — the old floor's grain"
        );

        // What the old code did to the window itself, at that rung.
        let floored = NOW - NOW.rem_euclid(30 * 86_400);
        assert!(
            NOW - floored > 26 * 86_400,
            "the old floor dropped {} days of the newest sales",
            (NOW - floored) / 86_400
        );

        // What the fix does: bounded by the TTL, whatever the bucket width.
        for step in ultros_charts::data::buckets::BUCKET_LADDER {
            let grain = cache_ttl_secs(false, step) as i64;
            let stamp = open_window_cache_stamp(NOW, grain);
            assert!(
                grain <= 3_600 && NOW - stamp < 3_600,
                "at a {step}s bucket the stamp discarded {}s",
                NOW - stamp
            );
        }
    }

    /// A grain of zero (or negative) must not panic on `rem_euclid`.
    #[test]
    fn cache_stamp_tolerates_a_degenerate_grain() {
        assert_eq!(open_window_cache_stamp(NOW, 0), NOW);
        assert_eq!(open_window_cache_stamp(NOW, -5), NOW);
    }

    // `world_group_map` at `SeriesGroup::World` is intentionally not tested
    // here: it short-circuits before touching `world_cache` (see the
    // `if group == SeriesGroup::World` early return), so an identity-pairs
    // test wouldn't exercise the cache at all — but the only way to build a
    // real `&WorldCache` is `WorldCache::new(&UltrosDb)`, which is async and
    // needs a live database connection. Contorting the handler to accept a
    // trait object or a fake cache just to cover a function that ignores its
    // argument isn't worth it; the fold logic below is where the real risk
    // (silent data corruption) lives, so it gets the coverage instead.

    fn row(series_id: i32, bucket_offset_secs: i64) -> PriceSeriesRow {
        PriceSeriesRow {
            series_id,
            bucket: chrono::DateTime::from_timestamp(bucket_offset_secs, 0).unwrap(),
            open: 100,
            high: 110,
            low: 90,
            close: 105,
            gil: 1_000,
            units: 10,
            sales: 1,
            p25: 95,
            p50: 100,
            p75: 105,
        }
    }

    /// A single series with many buckets folds into one entry carrying every
    /// bucket, in the order the rows arrived.
    #[test]
    fn fold_groups_many_buckets_of_one_series_into_one_entry() {
        let rows: Vec<PriceSeriesRow> = (0..50).map(|i| row(7, i * 3_600)).collect();
        let entries = fold_price_series_rows(&rows);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, 7);
        assert_eq!(entries[0].buckets.len(), 50);
        // Order is preserved, not re-sorted.
        for (i, bucket) in entries[0].buckets.iter().enumerate() {
            let expected = chrono::DateTime::from_timestamp(i as i64 * 3_600, 0)
                .unwrap()
                .naive_utc();
            assert_eq!(bucket.ts, expected);
        }
    }

    /// Two series, each internally contiguous (the shape the query's
    /// `ORDER BY (series_id, bucket)` actually produces), fold into exactly
    /// two entries with the right buckets in each.
    #[test]
    fn fold_splits_contiguous_series_into_separate_entries() {
        let rows = vec![
            row(1, 0),
            row(1, 3_600),
            row(1, 7_200),
            row(2, 0),
            row(2, 3_600),
        ];
        let entries = fold_price_series_rows(&rows);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, 1);
        assert_eq!(entries[0].buckets.len(), 3);
        assert_eq!(entries[1].id, 2);
        assert_eq!(entries[1].buckets.len(), 2);
    }

    /// Documents the fold's reliance on the query's `ORDER BY (series_id,
    /// bucket)`: it does a single linear pass and only merges a row into the
    /// *last* entry when the series id matches, so out-of-order (interleaved)
    /// rows for the same series split into multiple entries instead of
    /// merging. This is intentional — see the doc comment on
    /// `fold_price_series_rows` — but is pinned here so a future change to
    /// either the query's ORDER BY or this fold doesn't silently start
    /// producing merged results (or silently start relying on merging).
    #[test]
    fn fold_does_not_merge_interleaved_rows_of_the_same_series() {
        let rows = vec![row(1, 0), row(2, 0), row(1, 3_600)];
        let entries = fold_price_series_rows(&rows);
        assert_eq!(entries.len(), 3, "interleaved rows are not re-grouped");
        assert_eq!(entries[0].id, 1);
        assert_eq!(entries[1].id, 2);
        assert_eq!(entries[2].id, 1);
    }
}

async fn refresh_world_item_listings(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    Path((world, item_id)): Path<(String, i32)>,
    State(world_cache): State<Arc<WorldCache>>,
    State(universalis): State<UniversalisClient>,
) -> Result<Redirect, WebError> {
    let lookup = world_cache.lookup_value_by_name(&world)?;
    let all_worlds = world_cache
        .get_all_worlds_in(&lookup)
        .ok_or_else(|| anyhow::Error::msg("Unable to get worlds"))?;
    let world_clone = world.clone();
    let future = tokio::spawn(async move {
        let current_data = universalis
            .marketboard_current_data(&world_clone, &[item_id])
            .await?;
        // we can potentially get listings from multiple worlds from this call so we should group listings by world
        let listings = match current_data {
            universalis::MarketView::SingleView(v) => v.listings,
            universalis::MarketView::MultiView(_) => {
                return Result::<_, anyhow::Error>::Err(anyhow::Error::msg(
                    "multiple listings returned?",
                ));
            }
        };

        // now ensure we insert all worlds into the map to account for empty worlds
        let listings_by_world: HashMap<u16, Vec<ListingView>> =
            all_worlds.into_iter().map(|w| (w as u16, vec![])).collect();
        let first_key = if listings_by_world.len() == 1 {
            listings_by_world.keys().next().copied()
        } else {
            None
        };
        let listings_by_world = listings
            .into_iter()
            .flat_map(|l| {
                if let Some(key) = first_key {
                    Some((key, l))
                } else {
                    l.world_id.map(|w| (w, l))
                }
            })
            .fold(listings_by_world, |mut m, (w, l)| {
                m.entry(w).or_default().push(l);
                m
            });
        debug!("manually refreshed worlds: {listings_by_world:?}");
        for (world_id, listings) in listings_by_world {
            let (added, removed) = db
                .update_listings(listings, ItemId(item_id), WorldId(world_id as i32))
                .await?;
            senders
                .listings
                .send(EventType::Add(Arc::new(ListingEventData {
                    item_id,
                    world_id: world_id.into(),
                    listings: added,
                })))?;
            senders
                .listings
                .send(EventType::Remove(Arc::new(ListingEventData {
                    item_id,
                    world_id: world_id.into(),
                    listings: removed,
                })))?;
        }
        Ok(())
    });
    let _ = timeout(Duration::from_secs(1), future).await?;
    Ok(Redirect::to(&format!("/item/{world}/{item_id}")))
}

pub(crate) use self::state::WebState;
use self::static_files::{
    fallback_item_icon, favicon, get_item_icon, robots, service_worker_js, static_path,
};

pub(crate) async fn invite() -> Redirect {
    let client_id = std::env::var("DISCORD_CLIENT_ID").expect("Unable to get DISCORD_CLIENT_ID");
    Redirect::to(&format!(
        "https://discord.com/oauth2/authorize?client_id={client_id}&scope=bot&permissions=2147483648"
    ))
}

pub(crate) async fn world_data(State(world_cache): State<Arc<WorldCache>>) -> impl IntoResponse {
    static ONCE: OnceLock<WorldData> = OnceLock::new();
    let world_data = ONCE.get_or_init(move || WorldData::from(world_cache.as_ref()));
    let mut response = Json(world_data).into_response();
    response
        .headers_mut()
        .typed_insert(CacheControl::new().with_max_age(Duration::from_secs(60 * 60 * 24)));
    response
}

pub(crate) async fn current_user(user: AuthDiscordUser) -> Json<UserData> {
    Json(UserData {
        id: user.id,
        username: user.name,
        avatar: user.avatar_url,
    })
}

pub(crate) async fn retainer_listings(
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
) -> Result<Json<RetainerListings>, ApiError> {
    let (retainer, listings) = db.get_retainer_listings(id).await?;
    let listings = RetainerListings {
        retainer: retainer.into(),
        listings: listings.into_iter().map(ActiveListing::from).collect(),
    };
    Ok(Json(listings))
}

pub(crate) async fn user_retainers(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<UserRetainers>, ApiError> {
    // load the retainer/character details from the database and then extract it into the shared API types.
    let retainers = UserRetainers {
        retainers: db
            .get_all_owned_retainers_and_character(user.id)
            .await?
            .into_iter()
            .map(|(character, retainers)| {
                (
                    character.map(FfxivCharacter::from),
                    retainers
                        .into_iter()
                        .map(|(owned, retainer)| {
                            (OwnedRetainer::from(owned), Retainer::from(retainer))
                        })
                        .collect(),
                )
            })
            .collect(),
    };
    Ok(Json(retainers))
}

pub(crate) async fn user_retainer_listings(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<UserRetainerListings>, ApiError> {
    let db = &db;
    // Get a list of all the user's retainers, convert them to the appropriate type for our API call, and get listings for each retainer
    let retainers = db.get_all_owned_retainers_and_character(user.id).await?;
    let listings_iter = retainers
        .into_iter()
        .map(|(character, retainers)| async move {
            // collect intermediate results with try_join_all to cancel early if there's an error
            let retainers_with_listings =
                try_join_all(retainers.into_iter().map(|(_owned, retainer)| async move {
                    let listings = db.get_retainer_listings(retainer.id).await;
                    listings.map(|(_retainer, listings)| {
                        (
                            Retainer::from(retainer),
                            listings
                                .into_iter()
                                .map(ActiveListing::from)
                                .collect::<Vec<_>>(),
                        )
                    })
                }))
                .await;
            retainers_with_listings.map(|r| (character.map(FfxivCharacter::from), r))
        });
    let listings = try_join_all(listings_iter).await?;
    let retainers = UserRetainerListings {
        retainers: listings,
    };
    Ok(Json(retainers))
}

pub(crate) async fn retainer_search(
    State(db): State<UltrosDb>,
    Path(retainer_name): Path<String>,
) -> Result<Json<Vec<Retainer>>, ApiError> {
    let retainers = db.search_retainers(&retainer_name).await?;
    Ok(Json(retainers))
}

pub(crate) async fn claim_retainer(
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<(), ApiError> {
    db.register_retainer(id, user.id, user.name).await?;
    Ok(())
}

pub(crate) async fn unclaim_retainer(
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<(), ApiError> {
    db.remove_owned_retainer(user.id, id).await?;
    Ok(())
}

pub(crate) async fn get_lists(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<ListWithPermission>>, ApiError> {
    let lists = try_join_all(
        db.get_lists_for_user(user.id as i64)
            .await?
            .into_iter()
            .map(|(list, owner_name)| {
                let db = db.clone();
                let user_id = user.id as i64;
                async move {
                    let permission = db.get_permission(list.id, user_id).await?;
                    Ok::<_, ApiError>(ListWithPermission {
                        list: List::try_from(list)?,
                        permission,
                        owner_name,
                    })
                }
            }),
    )
    .await?;
    Ok(Json(lists))
}

pub(crate) async fn get_list(
    State(db): State<UltrosDb>,
    perm: crate::web::list_permission::RequireListPermission<{ crate::web::list_permission::READ }>,
) -> Result<Json<(ListWithPermission, Vec<ListItem>)>, ApiError> {
    let ((list, owner_name), list_items) = futures::future::try_join(
        db.get_list(perm.list_id, perm.user_id),
        db.get_list_items(perm.list_id, perm.user_id),
    )
    .await?;
    let list_items = list_items
        .into_iter()
        .map(ListItem::from)
        .collect::<Vec<_>>();
    let list = ListWithPermission {
        list: List::try_from(list)?,
        permission: perm.permission,
        owner_name: Some(owner_name),
    };
    Ok(Json((list, list_items)))
}

pub(crate) async fn get_list_with_listings(
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<(ListWithPermission, Vec<(ListItem, Vec<ActiveListing>)>)>, ApiError> {
    let ((list, owner_name), list_items) = futures::future::try_join(
        db.get_list(id, user.id as i64),
        db.get_list_items(id, user.id as i64),
    )
    .await?;
    let permission = db.get_permission(id, user.id as i64).await?;
    // tbd: probably don't need to send clients all listings, but for now keep it this way.
    let selector = AnySelector::try_from(&list)?;
    let world = world_cache.lookup_selector(&selector)?;
    let world_ids = world_cache
        .get_all_worlds_in(&world)
        .ok_or(anyhow::anyhow!("Bad world id"))?;
    let item_ids: Vec<_> = list_items.iter().map(|i| i.item_id).collect();
    let listings = db
        .get_listings_for_items_in_worlds(&world_ids, &item_ids)
        .await?;
    let mut listings_map: HashMap<i32, Vec<ActiveListing>> = HashMap::new();
    for listing in listings {
        listings_map
            .entry(listing.item_id)
            .or_default()
            .push(listing.into());
    }

    let list_items = list_items
        .into_iter()
        .map(|list| {
            let listings = listings_map.get(&list.item_id).cloned().unwrap_or_default();
            (ListItem::from(list), listings)
        })
        .collect();

    Ok(Json((
        ListWithPermission {
            list: List::try_from(list)?,
            permission,
            owner_name: Some(owner_name),
        },
        list_items,
    )))
}

#[derive(Deserialize)]
pub(crate) struct ListActivityQuery {
    limit: Option<u64>,
    before: Option<i64>,
}

pub(crate) async fn get_list_activity(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
    Query(query): Query<ListActivityQuery>,
) -> Result<Json<Vec<ListActivity>>, ApiError> {
    let activity = db
        .get_list_activity(id, user.id as i64, query.limit.unwrap_or(50), query.before)
        .await?;
    Ok(Json(activity.into_iter().map(ListActivity::from).collect()))
}

pub(crate) async fn delete_list(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    perm: crate::web::list_permission::RequireListPermission<
        { crate::web::list_permission::OWNER },
    >,
) -> Result<Json<()>, ApiError> {
    let (list, _) = db.get_list(perm.list_id, perm.user_id).await?;
    db.delete_list(perm.list_id, perm.user_id).await?;
    send_list_event(
        &senders,
        EventType::removed(ListEventData::List(List::try_from(list)?)),
    );
    Ok(Json(()))
}

pub(crate) async fn create_list(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Json(list): Json<CreateList>,
) -> Result<Json<()>, ApiError> {
    let discord_user = db
        .get_or_create_discord_user(user.id, user.name.clone())
        .await?;
    let list = db
        .create_list(discord_user, list.name, Some(list.wdr_filter.into()))
        .await?;
    send_list_event(
        &senders,
        EventType::added(ListEventData::List(List::try_from(list.clone())?)),
    );
    record_list_activity(
        &db,
        &senders,
        list.id,
        &user,
        ListActivityKind::ListCreated,
        None,
        None,
        serde_json::json!({ "name": list.name.clone() }),
        format!("{} created list {}", user.name, list.name),
    )
    .await?;
    Ok(Json(()))
}

pub(crate) async fn edit_list(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Json(list): Json<List>,
) -> Result<Json<()>, ApiError> {
    let list = db
        .update_list(list.id, user.id as i64, |ulist| {
            use ultros_api_types::world_helper::AnySelector;
            let (datacenter_id, region_id, world_id) = match list.wdr_filter {
                AnySelector::Datacenter(dc) => (Some(dc), None, None),
                AnySelector::Region(region) => (None, Some(region), None),
                AnySelector::World(world) => (None, None, Some(world)),
            };
            ulist.datacenter_id = ActiveValue::Set(datacenter_id);
            ulist.region_id = ActiveValue::Set(region_id);
            ulist.world_id = ActiveValue::Set(world_id);
            ulist.name = ActiveValue::Set(list.name);
        })
        .await?;
    send_list_event(
        &senders,
        EventType::updated(ListEventData::List(List::try_from(list.clone())?)),
    );
    record_list_activity(
        &db,
        &senders,
        list.id,
        &user,
        ListActivityKind::ListUpdated,
        None,
        None,
        serde_json::json!({ "name": list.name.clone() }),
        format!("{} updated list {}", user.name, list.name),
    )
    .await?;
    Ok(Json(()))
}

pub(crate) async fn post_item_to_list(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    perm: crate::web::list_permission::RequireListPermission<
        { crate::web::list_permission::WRITE },
    >,
    Json(item): Json<ListItem>,
) -> Result<Json<()>, ApiError> {
    let (list, _) = db.get_list(perm.list_id, perm.user_id).await?;
    let ListItem {
        item_id,
        hq,
        quantity,
        acquired,
        ..
    } = item;
    let item = db
        .add_item_to_list(&list, perm.user_id, item_id, hq, quantity, acquired)
        .await?;
    send_list_event(
        &senders,
        EventType::added(ListEventData::ListItem(item.clone().into())),
    );
    let item_name = resolve_item_name(item.item_id);
    record_list_activity(
        &db,
        &senders,
        item.list_id,
        &user,
        ListActivityKind::ItemAdded,
        Some(item.id),
        Some(item.item_id),
        serde_json::json!({
            "quantity": item.quantity,
            "acquired": item.acquired,
            "hq": item.hq,
            "target_price": item.target_price,
        }),
        format!("{} added {}", user.name.clone(), item_name),
    )
    .await?;
    Ok(Json(()))
}

pub(crate) async fn post_items_to_list(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
    Json(items): Json<Vec<ListItem>>,
) -> Result<Json<()>, ApiError> {
    let (list, _) = db.get_list(id, user.id as i64).await?;

    let _list = db
        .add_items_to_list(&list, user.id as i64, items.into_iter().map(|i| i.into()))
        .await?;
    // For bulk add, we might want to send a "refresh" event or all items.
    // Given the current structure, maybe just sending a list update is enough if we want to be simple,
    // but the task says synchronize buying.
    // For now, let's just trigger a refetch by sending the List update.
    send_list_event(
        &senders,
        EventType::updated(ListEventData::List(List::try_from(list.clone())?)),
    );
    record_list_activity(
        &db,
        &senders,
        list.id,
        &user,
        ListActivityKind::ItemAdded,
        None,
        None,
        serde_json::json!({ "bulk": true }),
        format!("{} imported items into {}", user.name, list.name),
    )
    .await?;
    Ok(Json(()))
}

pub(crate) async fn edit_list_item(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Json(item): Json<ListItem>,
) -> Result<Json<()>, ApiError> {
    let before = db.get_list_item(item.id, user.id as i64).await?;
    let item = item.into();
    let item = db.update_list_item(item, user.id as i64).await?;
    send_list_event(
        &senders,
        EventType::updated(ListEventData::ListItem(item.clone().into())),
    );
    let item_name = resolve_item_name(item.item_id);
    let before_acquired = before.acquired.unwrap_or(0);
    let after_acquired = item.acquired.unwrap_or(0);
    let quantity = item.quantity.unwrap_or(1);
    let kind = if after_acquired >= quantity && before_acquired < quantity {
        ListActivityKind::ItemAcquired
    } else {
        ListActivityKind::ItemUpdated
    };
    let message = if kind == ListActivityKind::ItemAcquired {
        format!("{} got {}", user.name, item_name)
    } else {
        format!("{} updated {}", user.name, item_name)
    };
    record_list_activity(
        &db,
        &senders,
        item.list_id,
        &user,
        kind,
        Some(item.id),
        Some(item.item_id),
        item_change_payload(&before, &item),
        message,
    )
    .await?;
    Ok(Json(()))
}

pub(crate) async fn delete_list_item(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<()>, ApiError> {
    let item = db.remove_item_from_list(user.id as i64, id).await?;
    send_list_event(
        &senders,
        EventType::removed(ListEventData::ListItem(item.clone().into())),
    );
    let item_name = resolve_item_name(item.item_id);
    record_list_activity(
        &db,
        &senders,
        item.list_id,
        &user,
        ListActivityKind::ItemRemoved,
        Some(item.id),
        Some(item.item_id),
        serde_json::json!({
            "quantity": item.quantity,
            "acquired": item.acquired,
            "hq": item.hq,
            "target_price": item.target_price,
        }),
        format!("{} removed {}", user.name, item_name),
    )
    .await?;
    Ok(Json(()))
}

#[derive(Deserialize)]
pub(crate) struct BulkHqUpdate {
    pub(crate) ids: Vec<i32>,
    pub(crate) hq: Option<bool>,
}

pub(crate) async fn bulk_edit_list_items_hq(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Json(data): Json<BulkHqUpdate>,
) -> Result<Json<()>, ApiError> {
    let list_ids = db
        .set_list_items_hq(user.id as i64, &data.ids, data.hq)
        .await?;

    for list_id in list_ids {
        if let Ok((list, _)) = db.get_list(list_id, user.id as i64).await {
            send_list_event(
                &senders,
                EventType::updated(ListEventData::List(List::try_from(list)?)),
            );
            let _ = record_list_activity(
                &db,
                &senders,
                list_id,
                &user,
                ListActivityKind::ItemUpdated,
                None,
                None,
                serde_json::json!({ "bulk_hq": data.hq, "count": data.ids.len() }),
                format!("{} bulk updated HQ for {} items", user.name, data.ids.len()),
            )
            .await;
        }
    }

    Ok(Json(()))
}

pub(crate) async fn delete_multiple_list_items(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Json(ids): Json<Vec<i32>>,
) -> Result<Json<()>, ApiError> {
    let deleted_items = try_join_all(
        ids.into_iter()
            .map(|id| db.remove_item_from_list(user.id as i64, id)),
    )
    .await?;
    let deleted_count = deleted_items.len();
    let list_id = deleted_items.first().map(|item| item.list_id);
    for item in deleted_items {
        send_list_event(
            &senders,
            EventType::removed(ListEventData::ListItem(item.into())),
        );
    }
    if let Some(list_id) = list_id {
        record_list_activity(
            &db,
            &senders,
            list_id,
            &user,
            ListActivityKind::ItemsRemoved,
            None,
            None,
            serde_json::json!({ "count": deleted_count }),
            format!("{} removed {deleted_count} items", user.name),
        )
        .await?;
    }
    Ok(Json(()))
}

/// Does a bulk lookup of item listings. Will not preserve order.
pub(crate) async fn bulk_item_listings(
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    Path((world, item_ids)): Path<(String, String)>,
) -> Result<Json<HashMap<i32, Vec<(ActiveListing, Option<Retainer>)>>>, ApiError> {
    let world_lookup = world_cache.lookup_value_by_name(&world)?;
    // borrow our worlds list & db now so it can be shared into the lookup futures
    let worlds = &world_cache
        .get_all_worlds_in(&world_lookup)
        .ok_or(anyhow::anyhow!("Invalid world"))?;
    // get item ids
    let item_ids: HashSet<i32> = item_ids.split(',').map(|id| id.parse()).try_collect()?;
    let item_vec: Vec<i32> = item_ids.iter().cloned().collect();
    // now perform lookups for all the listings for each world/item pair
    let mut listings_map = db.get_listings_for_items(worlds, &item_vec).await?;

    // now convert the database models to API types.
    let listings = item_ids
        .into_iter()
        .map(|id| {
            let l = listings_map.remove(&id).unwrap_or_default();
            (
                id,
                l.into_iter()
                    .map(|(listing, retainer)| {
                        (ActiveListing::from(listing), retainer.map(Retainer::from))
                    })
                    .collect(),
            )
        })
        .collect();
    Ok(Json(listings))
}

// #[debug_handler(state = WebState)]
async fn user_characters(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<FfxivCharacter>>, ApiError> {
    let characters = db
        .get_all_characters_for_discord_user(user.id as i64)
        .await?;
    // we can now strip the owned final fantasy character tag and convert to the API version
    Ok(Json(
        characters
            .into_iter()
            .flat_map(|(_, character)| character.map(|c| c.into()))
            .collect::<Vec<_>>(),
    ))
}

async fn character_search(
    _user: AuthDiscordUser, // user required just to prevent this endpoint from being abused.
    Path(name): Path<String>,
    State(cache): State<Arc<WorldCache>>,
) -> Result<Json<Vec<FfxivCharacter>>, ApiError> {
    let builder = lodestone::search::SearchBuilder::new().character(&name);
    // if let Some(world) = query.world {
    //     let world = cache.lookup_selector(&AnySelector::World(world))?;
    //     let world_name = world.get_name();
    //     builder = builder.server(Server::from_str(world_name)?);
    // }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    let search_results = builder.send_async(&client).await?;

    let characters = search_results
        .into_iter()
        .flat_map(|r| {
            // world comes back as World [Datacenter], so strip the datacenter and parse the world
            let (world, _) = r.world.split_once(' ')?;
            let world = cache.lookup_value_by_name(world).ok()?;
            let (first_name, last_name) = r.name.split_once(' ')?;
            Some(FfxivCharacter {
                id: r.user_id as i32,
                first_name: first_name.to_string(),
                last_name: last_name.to_string(),
                world_id: world.as_world().ok()?.id,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(characters))
}

/// Claims a character for the logged-in user.
///
/// There's no verification step: the Discord login already says who the user
/// is, and a claim only groups their retainers. Several users may hold the same
/// character.
async fn claim_character(
    user: AuthDiscordUser,
    Path(character_id): Path<u32>,
    State(claim): State<CharacterClaimService>,
) -> Result<Json<FfxivCharacter>, ApiError> {
    let character = claim.claim_character(character_id, user.id as i64).await?;
    Ok(Json(character.into()))
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn search(
    State(service): State<SearchService>,
    Query(query): Query<SearchQuery>,
) -> Json<Vec<ultros_api_types::search::SearchResult>> {
    Json(service.search(&query.q))
}

// #[debug_handler(state = WebState)]
async fn unclaim_character(
    user: AuthDiscordUser,
    Path(character_id): Path<i32>,
    State(db): State<UltrosDb>,
) -> Result<Json<()>, ApiError> {
    db.delete_owned_character(user.id as i64, character_id)
        .await?;
    Ok(Json(()))
}

// --- Group management ---

pub(crate) async fn get_groups(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<UserGroup>>, ApiError> {
    let groups = db.get_groups_for_user(user.id as i64).await?;
    Ok(Json(groups.into_iter().map(UserGroup::from).collect()))
}

pub(crate) async fn create_group(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(group): Json<CreateGroup>,
) -> Result<Json<UserGroup>, ApiError> {
    let group = db.create_group(group.name, user.id as i64).await?;
    Ok(Json(UserGroup::from(group)))
}

/// Discord servers the user could turn into a group, annotated with whether a
/// group already exists for each.
pub(crate) async fn get_group_discord_guilds(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<DiscordManageableGuild>>, ApiError> {
    let ctx = crate::alerts::delivery::get_serenity_ctx().ok_or_else(|| {
        ApiError::from(anyhow::anyhow!(
            "Discord bot is not connected; cannot load your servers right now"
        ))
    })?;
    let guilds =
        crate::web::api::discord_lookup::manageable_guilds_for_user(&ctx, user.id as i64).await?;

    let guild_ids: Vec<i64> = guilds.iter().map(|(id, _, _)| *id).collect();
    let existing = db.group_ids_for_guilds(&guild_ids).await?;

    Ok(Json(
        guilds
            .into_iter()
            .map(|(id, name, icon_url)| DiscordManageableGuild {
                id,
                name,
                icon_url,
                existing_group_id: existing.get(&id).copied(),
            })
            .collect(),
    ))
}

pub(crate) async fn create_group_from_guild(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(CreateGroupFromGuild { guild_id }): Json<CreateGroupFromGuild>,
) -> Result<Json<UserGroup>, ApiError> {
    let ctx = crate::alerts::delivery::get_serenity_ctx().ok_or_else(|| {
        ApiError::from(anyhow::anyhow!(
            "Discord bot is not connected; cannot create a group from a server right now"
        ))
    })?;

    // Re-check against Discord rather than trusting the picker: the guild id
    // arrives from the client, and the user's roles may have changed since the
    // list was rendered. This also proves the bot is in the guild, and hands
    // back the name and icon so a group can't claim to be a server it isn't.
    let guild =
        crate::web::api::discord_lookup::require_manageable_guild(&ctx, guild_id, user.id as i64)
            .await?;

    let group = db
        .create_group_from_guild(
            guild.name.clone(),
            user.id as i64,
            guild_id,
            guild.icon_url(),
        )
        .await?;
    Ok(Json(UserGroup::from(group)))
}

pub(crate) async fn delete_group(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
) -> Result<Json<()>, ApiError> {
    db.delete_group(id, user.id as i64).await?;
    Ok(Json(()))
}

pub(crate) async fn get_group_members(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
) -> Result<Json<Vec<UserGroupMember>>, ApiError> {
    let members = db.get_group_members(id, user.id as i64).await?;
    Ok(Json(
        members.into_iter().map(UserGroupMember::from).collect(),
    ))
}

pub(crate) async fn add_group_member(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path((group_id, member_id)): Path<(i32, i64)>,
) -> Result<Json<()>, ApiError> {
    db.add_group_member(group_id, user.id as i64, member_id)
        .await?;
    Ok(Json(()))
}

pub(crate) async fn remove_group_member(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path((group_id, member_id)): Path<(i32, i64)>,
) -> Result<Json<()>, ApiError> {
    db.remove_group_member(group_id, user.id as i64, member_id)
        .await?;
    Ok(Json(()))
}

pub(crate) async fn get_group_invites(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
) -> Result<Json<Vec<GroupInvite>>, ApiError> {
    let invites = db.get_group_invites(id, user.id as i64).await?;
    Ok(Json(invites.into_iter().map(GroupInvite::from).collect()))
}

pub(crate) async fn create_group_invite(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
    Json(CreateGroupInvite { max_uses }): Json<CreateGroupInvite>,
) -> Result<Json<GroupInvite>, ApiError> {
    let invite = db.create_group_invite(id, user.id as i64, max_uses).await?;
    Ok(Json(GroupInvite::from(invite)))
}

/// Redeem an invite and return the group joined, so the client can navigate
/// straight to it. Redeeming an invite you've already used is a success.
pub(crate) async fn use_group_invite(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<String>,
) -> Result<Json<i32>, ApiError> {
    let group_id = db.use_group_invite(id, user.id as i64).await?;
    Ok(Json(group_id))
}

pub(crate) async fn delete_group_invite(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<String>,
) -> Result<Json<()>, ApiError> {
    db.delete_group_invite(id, user.id as i64).await?;
    Ok(Json(()))
}

// --- List sharing ---

pub(crate) async fn get_list_shares(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
) -> Result<Json<(Vec<ListSharedUser>, Vec<ListSharedGroup>)>, ApiError> {
    let (users, groups) = futures::future::try_join(
        db.get_list_shared_users(id, user.id as i64),
        db.get_list_shared_groups(id, user.id as i64),
    )
    .await?;
    Ok(Json((
        users.into_iter().map(ListSharedUser::from).collect(),
        groups.into_iter().map(ListSharedGroup::from).collect(),
    )))
}

// Sharing changes who can see the list — broadcast a list-update event so
// affected clients (the recipient and the owner) refetch their list set.
async fn broadcast_list_update(
    db: &UltrosDb,
    senders: &EventSenders,
    list_id: i32,
    user: i64,
) -> Result<(), ApiError> {
    let (list, _) = db.get_list(list_id, user).await?;
    send_list_event(
        senders,
        EventType::updated(ListEventData::List(List::try_from(list)?)),
    );
    Ok(())
}

pub(crate) async fn share_list_with_user(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
    Json(share): Json<ShareListUser>,
) -> Result<Json<()>, ApiError> {
    db.share_list_with_user(id, user.id as i64, share.user_id, share.permission)
        .await?;
    record_list_activity(
        &db,
        &senders,
        id,
        &user,
        ListActivityKind::SharedUser,
        None,
        None,
        serde_json::json!({
            "user_id": share.user_id,
            "permission": share.permission as i16,
        }),
        format!("{} shared this list with user {}", user.name, share.user_id),
    )
    .await?;
    broadcast_list_update(&db, &senders, id, user.id as i64).await?;
    Ok(Json(()))
}

pub(crate) async fn share_list_with_group(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
    Json(share): Json<ShareListGroup>,
) -> Result<Json<()>, ApiError> {
    db.share_list_with_group(id, user.id as i64, share.group_id, share.permission)
        .await?;
    record_list_activity(
        &db,
        &senders,
        id,
        &user,
        ListActivityKind::SharedGroup,
        None,
        None,
        serde_json::json!({
            "group_id": share.group_id,
            "permission": share.permission as i16,
        }),
        format!(
            "{} shared this list with group {}",
            user.name, share.group_id
        ),
    )
    .await?;
    broadcast_list_update(&db, &senders, id, user.id as i64).await?;
    Ok(Json(()))
}

pub(crate) async fn unshare_list_from_user(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Path((id, user_id)): Path<(i32, i64)>,
) -> Result<Json<()>, ApiError> {
    db.unshare_list_from_user(id, user.id as i64, user_id)
        .await?;
    let _ = record_list_activity(
        &db,
        &senders,
        id,
        &user,
        ListActivityKind::UnsharedUser,
        None,
        None,
        serde_json::json!({ "user_id": user_id }),
        format!("{} removed user {} from this list", user.name, user_id),
    )
    .await;
    // Best-effort: only broadcast if the caller still has read permission
    // (e.g. the owner unsharing someone else). If a member removed themselves
    // they can no longer fetch the list, so skip the broadcast in that case.
    let _ = broadcast_list_update(&db, &senders, id, user.id as i64).await;
    Ok(Json(()))
}

pub(crate) async fn unshare_list_from_group(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Path((id, group_id)): Path<(i32, i32)>,
) -> Result<Json<()>, ApiError> {
    db.unshare_list_from_group(id, user.id as i64, group_id)
        .await?;
    record_list_activity(
        &db,
        &senders,
        id,
        &user,
        ListActivityKind::UnsharedGroup,
        None,
        None,
        serde_json::json!({ "group_id": group_id }),
        format!("{} removed group {} from this list", user.name, group_id),
    )
    .await?;
    broadcast_list_update(&db, &senders, id, user.id as i64).await?;
    Ok(Json(()))
}

// --- Invites ---

pub(crate) async fn get_list_invites(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
) -> Result<Json<Vec<ListInvite>>, ApiError> {
    let invites = db.get_list_invites(id, user.id as i64).await?;
    Ok(Json(invites.into_iter().map(ListInvite::from).collect()))
}

pub(crate) async fn create_invite(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Path(id): Path<i32>,
    Json(invite): Json<CreateInvite>,
) -> Result<Json<ListInvite>, ApiError> {
    let invite = db
        .create_invite(id, user.id as i64, invite.permission, invite.max_uses)
        .await?;
    record_list_activity(
        &db,
        &senders,
        id,
        &user,
        ListActivityKind::InviteCreated,
        None,
        None,
        serde_json::json!({
            "invite_id": invite.id.clone(),
            "permission": invite.permission,
            "max_uses": invite.max_uses,
        }),
        format!("{} created an invite", user.name),
    )
    .await?;
    Ok(Json(ListInvite::from(invite)))
}

pub(crate) async fn use_invite(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    user: AuthDiscordUser,
    Path(id): Path<String>,
) -> Result<Json<i32>, ApiError> {
    let shared = db.use_invite(id, user.id as i64).await?;
    record_list_activity(
        &db,
        &senders,
        shared.list_id,
        &user,
        ListActivityKind::InviteUsed,
        None,
        None,
        serde_json::json!({
            "permission": shared.permission,
        }),
        format!("{} joined this list with an invite", user.name),
    )
    .await?;
    // The user just gained access — surface the list to their UI.
    broadcast_list_update(&db, &senders, shared.list_id, user.id as i64).await?;
    Ok(Json(shared.list_id))
}

pub(crate) async fn delete_invite(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(id): Path<String>,
) -> Result<Json<()>, ApiError> {
    db.delete_invite(id, user.id as i64).await?;
    Ok(Json(()))
}

async fn reorder_retainer(
    user: AuthDiscordUser,
    State(db): State<UltrosDb>,
    Json(data): Json<Vec<OwnedRetainer>>,
) -> Result<Json<()>, ApiError> {
    for retainer in data {
        db.update_owned_retainer(user.id as i64, retainer.id, |mut existing_retainer| {
            existing_retainer.weight = ActiveValue::Set(retainer.weight);
            existing_retainer
        })
        .await?;
    }
    Ok(Json(()))
}

async fn assign_retainer_character(
    user: AuthDiscordUser,
    State(db): State<UltrosDb>,
    Path(owned_retainer_id): Path<i32>,
    Json(data): Json<AssignRetainerCharacter>,
) -> Result<Json<()>, ApiError> {
    if let Some(character_id) = data.character_id {
        let owns_character = db.user_owns_character(user.id as i64, character_id).await?;
        if !owns_character {
            return Err(ApiError::Forbidden(
                "Cannot assign a character owned by another user",
            ));
        }
    }

    db.update_owned_retainer(user.id as i64, owned_retainer_id, |mut owned_retainer| {
        owned_retainer.character_id = ActiveValue::Set(data.character_id);
        owned_retainer
    })
    .await?;

    Ok(Json(()))
}

async fn delete_user(
    user: AuthDiscordUser,
    State(cache): State<AuthUserCache>,
    State(db): State<UltrosDb>,
    cookie_jar: CookieJar,
) -> Result<(CookieJar, Redirect), ApiError> {
    let id = user.id;
    db.delete_discord_user(id as i64).await?;
    let token = cookie_jar
        .get("discord_auth")
        .ok_or(anyhow::anyhow!("Failed to get icon"))?
        .value()
        .to_owned();
    cache.remove_token(&token).await;
    let cookie_jar = cookie_jar.remove(oauth::discord_auth_removal_cookie());
    // remove the token from the cache
    // remove the auth cookie from the cache
    Ok((cookie_jar, Redirect::to("/")))
}

async fn get_xiv_data_bytes(
    Path((_version, lang)): Path<(String, String)>,
) -> Result<&'static [u8], WebError> {
    let lang = match lang.strip_suffix(".rkyv").unwrap_or(&lang) {
        "en" => xiv_gen::Language::En,
        "ja" => xiv_gen::Language::Ja,
        "de" => xiv_gen::Language::De,
        "fr" => xiv_gen::Language::Fr,
        "cn" => xiv_gen::Language::Cn,
        "ko" => xiv_gen::Language::Ko,
        "tc" => xiv_gen::Language::Tc,
        _ => return Err(anyhow::anyhow!("Unsupported language").into()),
    };
    Ok(xiv_gen_db::embedded_bytes(lang))
}

/// Returns a region- attempts to guess it from the CF Region header
async fn detect_region(region: Option<Region>) -> impl IntoResponse {
    if region.is_none() {
        warn!("Unable to detect region");
    }
    let mut response = region.unwrap_or(Region::NorthAmerica).into_response();
    response.headers_mut().typed_insert(
        CacheControl::new()
            .with_private()
            .with_max_age(Duration::from_secs(604800)),
    );
    response
}

async fn listings_redirect(Path((world, id)): Path<(String, i32)>) -> Redirect {
    Redirect::permanent(&format!("/item/{world}/{id}"))
}

/// Returns the test-only auth routes when the `test-auth` feature is enabled;
/// an empty router otherwise. Compile-time gated so prod binaries are clean.
#[cfg(feature = "test-auth")]
fn test_auth_routes() -> Router<WebState> {
    Router::new().route("/test/login", get(self::oauth::test_auth::test_login))
}

#[cfg(not(feature = "test-auth"))]
fn test_auth_routes() -> Router<WebState> {
    Router::new()
}

pub(crate) async fn start_web(
    state: WebState,
    prometheus_handle: metrics_exporter_prometheus::PrometheusHandle,
) {
    // build our application with a route
    let worlds = state.world_helper.clone();
    let token = state.token.clone();
    let app = Router::new()
        .route("/alerts/websocket", get(connect_websocket))
        .route("/api/v1/search", get(search))
        .route("/api/v1/realtime/events", get(real_time_data))
        .route("/api/v1/cheapest/{world}", get(cheapest_per_world))
        .route("/api/v1/trends/{world}", get(get_trends))
        .route("/api/v1/best_deals/{world}", get(get_best_deals))
        .route("/api/v1/market_pulse/{world}", get(get_market_pulse))
        .route("/api/v1/item_stats/{world}/{itemid}", get(get_item_stats))
        .route("/api/v1/movers/{world}", get(get_movers))
        .route("/api/v1/sparklines/{world}", post(post_sparklines))
        .route("/api/v1/resale_quality/{world}", post(post_resale_quality))
        .route("/api/v1/market_heat/{world}", get(get_market_heat))
        .route("/api/v1/recentSales/{world}", get(recent_sales))
        .route("/api/v1/sale_stats/{world}", get(get_sale_stats))
        .route("/api/v1/alerts/events", get(list_alert_events))
        .route(
            "/api/v1/alerts/events/{id}/resend",
            post(resend_alert_event),
        )
        .route("/api/v1/alerts", get(list_alerts).post(create_alert))
        .route(
            "/api/v1/alerts/{id}",
            axum::routing::patch(update_alert).delete(delete_alert),
        )
        .route(
            "/api/v1/endpoints",
            get(list_endpoints).post(create_endpoint),
        )
        .route(
            "/api/v1/endpoints/discord-guilds",
            get(list_discord_writable_guilds),
        )
        .route(
            "/api/v1/endpoints/{id}",
            axum::routing::patch(update_endpoint).delete(delete_endpoint),
        )
        .route("/api/v1/endpoints/{id}/test", post(test_endpoint))
        .route(
            "/api/v1/push/vapid-public-key",
            get(crate::web::api::push::get_vapid_public_key),
        )
        .route(
            "/api/v1/push/subscribe",
            post(crate::web::api::push::create_push_subscription),
        )
        .route(
            "/api/v1/listings/{world}/{itemid}",
            get(world_item_listings),
        )
        .route(
            "/api/v1/extended_history/{world}/{itemid}",
            get(extended_sale_history),
        )
        .route("/api/v1/price_series/{world}/{itemid}", get(price_series))
        .route("/api/v1/price_density/{world}/{itemid}", get(price_density))
        .route("/api/v1/game-history", get(game_history))
        .route(
            "/api/v1/bulkListings/{world}/{itemids}",
            get(bulk_item_listings),
        )
        .route("/api/v1/list", get(get_lists))
        .route("/api/v1/list/create", post(create_list))
        .route("/api/v1/list/edit", post(edit_list))
        .route("/api/v1/list/item/edit", post(edit_list_item))
        .route("/api/v1/list/{id}", get(get_list))
        .route("/api/v1/list/{id}/activity", get(get_list_activity))
        .route("/api/v1/list/{id}/listings", get(get_list_with_listings))
        .route("/api/v1/list/{id}/add/item", post(post_item_to_list))
        .route("/api/v1/list/{id}/add/items", post(post_items_to_list))
        .route("/api/v1/list/{id}/delete", delete(delete_list))
        .route("/api/v1/list/item/{id}/delete", delete(delete_list_item))
        .route("/api/v1/list/item/delete", post(delete_multiple_list_items))
        .route("/api/v1/list/item/hq", post(bulk_edit_list_items_hq))
        .route("/api/v1/group", get(get_groups))
        .route("/api/v1/group/create", post(create_group))
        .route(
            "/api/v1/group/discord-guilds",
            get(get_group_discord_guilds),
        )
        .route(
            "/api/v1/group/create-from-guild",
            post(create_group_from_guild),
        )
        .route("/api/v1/group/{id}", delete(delete_group))
        .route("/api/v1/group/{id}/members", get(get_group_members))
        .route(
            "/api/v1/group/{group_id}/member/add/{member_id}",
            post(add_group_member),
        )
        .route(
            "/api/v1/group/{group_id}/member/remove/{member_id}",
            delete(remove_group_member),
        )
        .route("/api/v1/group/{id}/invites", get(get_group_invites))
        .route(
            "/api/v1/group/{id}/invite/create",
            post(create_group_invite),
        )
        // Kept off the `/api/v1/group/{id}/...` prefix: the invite id is a
        // string where that prefix takes an i32, and a sibling path can't hold
        // both without the router treating one as a malformed group id.
        .route("/api/v1/group-invite/{id}/use", post(use_group_invite))
        .route("/api/v1/group-invite/{id}", delete(delete_group_invite))
        .route("/api/v1/list/{id}/shares", get(get_list_shares))
        .route("/api/v1/list/{id}/share/user", post(share_list_with_user))
        .route("/api/v1/list/{id}/share/group", post(share_list_with_group))
        .route(
            "/api/v1/list/{id}/share/user/{user_id}",
            delete(unshare_list_from_user),
        )
        .route(
            "/api/v1/list/{id}/share/group/{group_id}",
            delete(unshare_list_from_group),
        )
        .route("/api/v1/list/{id}/invites", get(get_list_invites))
        .route("/api/v1/list/{id}/invite/create", post(create_invite))
        .route("/api/v1/invite/{id}/use", post(use_invite))
        .route("/api/v1/invite/{id}", delete(delete_invite))
        .route("/api/v1/world_data", get(world_data))
        .route("/api/v1/current_user", get(current_user))
        .route("/api/v1/user/retainer", get(user_retainers))
        .route("/api/v1/retainer/reorder", post(reorder_retainer))
        .route(
            "/api/v1/retainer/{id}/character",
            post(assign_retainer_character),
        )
        .route(
            "/api/v1/user/retainer/listings",
            get(user_retainer_listings),
        )
        .route("/api/v1/retainer/search/{query}", get(retainer_search))
        .route("/api/v1/retainer/claim/{id}", get(claim_retainer))
        .route("/api/v1/retainer/unclaim/{id}", get(unclaim_retainer))
        .route(
            "/item/refresh/{worldid}/{itemid}",
            get(refresh_world_item_listings),
        )
        .route("/api/v1/retainer/listings/{id}", get(retainer_listings))
        .route("/api/v1/characters/search/{name}", get(character_search))
        .route("/api/v1/characters/claim/{id}", get(claim_character))
        .route("/api/v1/characters/unclaim/{id}", get(unclaim_character))
        .route("/api/v1/characters", get(user_characters))
        .route("/api/v1/detectregion", get(detect_region))
        .route("/retainers/add/{id}", get(add_retainer))
        .route("/retainers/remove/{id}", get(remove_owned_retainer))
        .route("/static/{*path}", get(static_path))
        .route("/static/itemicon/fallback", get(fallback_item_icon))
        .route("/static/itemicon/{path}", get(get_item_icon))
        .route("/static/data/{version}/{lang}", get(get_xiv_data_bytes))
        .route("/redirect", get(self::oauth::redirect))
        .route("/login", get(begin_login))
        .route("/logout", get(logout))
        .route("/api/v1/current_user", delete(delete_user))
        .route("/invitebot", get(invite))
        .route("/favicon.ico", get(favicon))
        .route("/robots.txt", get(robots))
        .route("/service-worker.js", get(service_worker_js))
        .route("/itemcard/{world}/{id}", get(item_card))
        .route("/sitemap/items.xml", get(item_sitemap))
        .route("/sitemap.xml", get(sitemap_index))
        .route("/sitemap/pages.xml", get(generic_pages_sitemap))
        .route("/listings/{world}/{item}", get(listings_redirect))
        .merge(test_auth_routes())
        .merge(create_leptos_app(state.world_helper.clone()).await.unwrap())
        .fallback(leptos_axum::file_and_error_handler_with_context::<
            WebState,
            _,
        >(
            move || {
                provide_context(LocalWorldData(Ok(worlds.clone())));
            },
            // The file/404 fallback doesn't have per-request bootstrap data; an
            // empty script tag is harmless and the client falls back to HTTP.
            |options| shell(options, String::new()),
        ))
        .with_state(state)
        .route_layer(middleware::from_fn(track_metrics))
        .layer(middleware::from_fn(redirect_legacy_book_host))
        // tower-http's default `on_failure` logs every 5xx via `tracing::error!`,
        // which the `sentry_tracing` layer turns into a GlitchTip issue.
        // See `failure_report_level` for which failures still warrant one.
        .layer(TraceLayer::new_for_http().on_failure(
            |class: ServerErrorsFailureClass, latency: Duration, _: &Span| {
                match failure_report_level(&class) {
                    FailureReportLevel::Debug => tracing::debug!(
                        classification = %class,
                        ?latency,
                        "response failed (likely warm-up)",
                    ),
                    FailureReportLevel::Warn => tracing::warn!(
                        classification = %class,
                        ?latency,
                        "response failed",
                    ),
                    FailureReportLevel::Error => tracing::error!(
                        classification = %class,
                        ?latency,
                        "response failed",
                    ),
                }
            },
        ))
        // Sentry/Glitchtip: bind a fresh Hub per request and decorate captured
        // events with HTTP context (method, URL, status). NewSentryLayer must
        // come before SentryHttpLayer; ServiceBuilder applies in declared
        // order so this is correct.
        .layer(
            ServiceBuilder::new()
                .layer(sentry_tower::NewSentryLayer::new_from_top())
                .layer(sentry_tower::SentryHttpLayer::new().enable_transaction()),
        )
        .layer(
            CompressionLayer::new().compress_when(
                SizeAbove::new(256)
                    // don't compress images
                    .and(NotForContentType::IMAGES),
            ),
        )
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ))
        // `same-origin` would strip the Referer from every cross-origin
        // request, AdSense and Sentry included. `strict-origin-when-cross-origin`
        // is the modern browser default: full URL same-origin, bare origin
        // cross-origin, nothing on an HTTPS->HTTP downgrade. Stating it
        // explicitly pins the behaviour for older clients without changing
        // what third parties already receive.
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        // The standards-track spelling of the `X-Frame-Options: DENY` above,
        // not additional protection — every browser we serve honours one or
        // the other. It is here so scanners stop flagging its absence.
        //
        // CAREFUL: this claims the CSP header, and the layer is `overriding`.
        // Adding a directive here is not free — the app loads Google
        // Analytics, AdSense and Sentry from other origins, so a `script-src`
        // or `connect-src` added to this string silently kills all three.
        // Ship any new directive in report-only first.
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("frame-ancestors 'none'"),
        ));

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    let port = std::env::var("PORT")
        .map(|p| p.parse::<u16>().ok())
        .ok()
        .flatten()
        .unwrap_or(8080);
    let metrics_token = token.clone();
    let (_main_app, _metrics_app) = futures::future::join(
        async move {
            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            tracing::info!("listening on {}", addr);
            let listener = TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    token.cancelled().await;
                })
                .await
                .unwrap();
        },
        start_metrics_server(prometheus_handle, metrics_token),
    )
    .await;
}
