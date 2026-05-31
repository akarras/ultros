mod alerts_websocket;
pub(crate) mod api;
pub(crate) mod character_verifier_service;
pub(crate) mod country_code_decoder;
pub(crate) mod error;
pub(crate) mod item_card;
pub(crate) mod list_permission;
pub(crate) mod oauth;
pub(crate) mod sitemap;
pub(crate) mod state;
pub(crate) mod static_files;

use anyhow::Error;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{delete, get, post};
use axum::{Json, Router, middleware};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use axum_extra::headers::{CacheControl, HeaderMapExt};
use futures::future::{try_join, try_join_all};
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
use tower_http::trace::TraceLayer;
use tracing::{Span, debug, warn};
use ultros_api_types::list::{
    CreateInvite, CreateList, List, ListActivity, ListActivityKind, ListInvite, ListItem,
    ListSharedGroup, ListSharedUser, ListWithPermission, ShareListGroup, ShareListUser,
};
use ultros_api_types::retainer::RetainerListings;
use ultros_api_types::user::group::{CreateGroup, UserGroup, UserGroupMember};
use ultros_api_types::user::{OwnedRetainer, UserData, UserRetainerListings, UserRetainers};
use ultros_api_types::websocket::{ListEventData, ListingEventData};
use ultros_api_types::world::WorldData;
use ultros_api_types::{
    ActiveListing, CompactSale, CurrentlyShownItem, ExtendedSaleHistory, FfxivCharacter,
    FfxivCharacterVerification, Retainer,
};
use ultros_app::{LocalWorldData, shell};
use ultros_db::ActiveValue;
use ultros_db::world_data::world_cache::AnySelector;
use ultros_db::{UltrosDb, world_data::world_cache::WorldCache};
use universalis::{ItemId, ListingView, UniversalisClient, WorldId};

use self::character_verifier_service::CharacterVerifierService;
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
    get_movers, get_trends, post_resale_quality, post_sparklines, recent_sales,
};
use crate::web::sitemap::{generic_pages_sitemap, item_sitemap, sitemap_index, world_sitemap};
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
    let world_iter = worlds.iter().copied();
    let (listings, sales) = try_join(
        db_clone.get_all_listings_in_worlds_with_retainers(&worlds, ItemId(item_id)),
        db.get_sale_history_from_multiple_worlds(world_iter, item_id, 200),
    )
    .await
    .inspect_err(|e| tracing::error!(error = ?e, "Error getting listings"))?;
    let currently_shown = CurrentlyShownItem {
        listings: listings
            .into_iter()
            .flat_map(|(l, r)| r.map(|r| (l.into(), r.into())))
            .collect(),
        sales: sales.into_iter().map(|s| s.into()).collect(),
    };
    Ok(axum::Json(currently_shown))
}

/// Compact extended sale history for charting. Returns up to `limit` rows (default
/// 1000, capped at 5000) of price/quantity/timestamp/world/hq — no buyer metadata.
/// Aimed at the "Load extended history" affordance on the price chart.
#[tracing::instrument(skip(db, world_cache))]
async fn extended_sale_history(
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    Path((world, item_id)): Path<(String, i32)>,
    axum::extract::Query(query): axum::extract::Query<ExtendedHistoryQuery>,
) -> Result<axum::Json<ExtendedSaleHistory>, WebError> {
    const DEFAULT_LIMIT: u64 = 1_000;
    const MAX_LIMIT: u64 = 5_000;
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

async fn refresh_world_item_listings(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    Path((world, item_id)): Path<(String, i32)>,
    State(world_cache): State<Arc<WorldCache>>,
) -> Result<Redirect, WebError> {
    let lookup = world_cache.lookup_value_by_name(&world)?;
    let all_worlds = world_cache
        .get_all_worlds_in(&lookup)
        .ok_or_else(|| anyhow::Error::msg("Unable to get worlds"))?;
    let world_clone = world.clone();
    let future = tokio::spawn(async move {
        let client = UniversalisClient::new("ultros");
        let current_data = client
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

pub(crate) async fn verify_character(
    State(character): State<CharacterVerifierService>,
    Path(verification_id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<bool>, ApiError> {
    character
        .check_verification(verification_id, user.id as i64)
        .await?;
    Ok(Json(true))
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
    let lists = db
        .get_lists_for_user(user.id as i64)
        .await?
        .into_iter()
        .map(|(list, permission)| {
            Ok::<_, ApiError>(ListWithPermission {
                list: List::try_from(list)?,
                permission,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(lists))
}

pub(crate) async fn get_list(
    State(db): State<UltrosDb>,
    perm: crate::web::list_permission::RequireListPermission<{ crate::web::list_permission::READ }>,
) -> Result<Json<(ListWithPermission, Vec<ListItem>)>, ApiError> {
    let (list, list_items) = futures::future::try_join(
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
    };
    Ok(Json((list, list_items)))
}

pub(crate) async fn get_list_with_listings(
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<(ListWithPermission, Vec<(ListItem, Vec<ActiveListing>)>)>, ApiError> {
    let (list, list_items) = futures::future::try_join(
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
    let list = db.get_list(perm.list_id, perm.user_id).await?;
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
    let list = db.get_list(perm.list_id, perm.user_id).await?;
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
    let list = db.get_list(id, user.id as i64).await?;

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

async fn pending_verifications(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<FfxivCharacterVerification>>, ApiError> {
    let verifications = db
        .get_all_pending_verification_challenges(user.id as i64)
        .await?;
    Ok(Json(
        verifications
            .into_iter()
            .flat_map(|(verification, character)| {
                character.map(|character| FfxivCharacterVerification {
                    id: verification.id,
                    character: character.into(),
                    verification_string: verification.challenge,
                })
            })
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
    let client = reqwest::Client::new();
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

async fn claim_character(
    user: AuthDiscordUser,
    Path(character_id): Path<u32>,
    State(verifier): State<CharacterVerifierService>,
) -> Result<Json<(i32, String)>, ApiError> {
    let result = verifier
        .start_verification(character_id, user.id as i64)
        .await?;
    // db.create_character_challenge(character_id, user.id as i64, challenge)
    Ok(Json(result))
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
    let list = db.get_list(list_id, user).await?;
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
    let cookie_jar = cookie_jar.remove(Cookie::from("discord_auth"));
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

pub(crate) async fn start_web(state: WebState) {
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
        .route("/api/v1/group", get(get_groups))
        .route("/api/v1/group/create", post(create_group))
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
        .route("/api/v1/characters/verify/{id}", get(verify_character))
        .route("/api/v1/characters", get(user_characters))
        .route(
            "/api/v1/characters/verifications",
            get(pending_verifications),
        )
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
        .route("/sitemap/world/{s}", get(world_sitemap))
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
        // which the `sentry_tracing` layer turns into a GlitchTip issue. The
        // analyzer service returns 503 during its warm-up window — those aren't
        // bugs, just a transient startup state (see WebError::as_status_code and
        // issues 5033/5034). Drop 503 to debug so it stays out of error logs.
        .layer(TraceLayer::new_for_http().on_failure(
            |class: ServerErrorsFailureClass, latency: Duration, _: &Span| match class {
                ServerErrorsFailureClass::StatusCode(status)
                    if status == hyper::StatusCode::SERVICE_UNAVAILABLE =>
                {
                    tracing::debug!(
                        %status,
                        ?latency,
                        "response failed (likely warm-up)",
                    );
                }
                _ => {
                    tracing::error!(
                        classification = %class,
                        ?latency,
                        "response failed",
                    );
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
        );

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    let port = std::env::var("PORT")
        .map(|p| p.parse::<u16>().ok())
        .ok()
        .flatten()
        .unwrap_or(8080);
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
        start_metrics_server(),
    )
    .await;
}
