mod alerts_websocket;
pub(crate) mod api;
pub(crate) mod character_verifier_service;
pub(crate) mod country_code_decoder;
pub(crate) mod error;
pub(crate) mod item_card;
pub(crate) mod oauth;
pub(crate) mod sitemap;

use anyhow::Error;
use axum::body::Body;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::{HeaderValue, Response, StatusCode};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{delete, get, post};
use axum::{Json, Router, body, middleware};
use axum_extra::TypedHeader;
use axum_extra::extract::cookie::Key;
use axum_extra::headers::{CacheControl, ContentType, HeaderMapExt};
use futures::future::{try_join, try_join_all};
use hyper::header;
use itertools::Itertools;
use leptos::config::LeptosOptions;
use leptos::prelude::provide_context;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tower_http::compression::predicate::{NotForContentType, SizeAbove};
use tower_http::compression::{CompressionLayer, Predicate};
use tower_http::trace::TraceLayer;
use tracing::{debug, warn};
use ultros_api_types::icon_size::IconSize;
use ultros_api_types::websocket::ListingEventData;
use ultros_api_types::world::WorldData;
use ultros_api_types::world_helper::WorldHelper;
use ultros_api_types::{ActiveListing, CurrentlyShownItem, Retainer};
use ultros_app::{LocalWorldData, shell};
use ultros_db::world_cache::WorldCache;
use ultros_db::UltrosDb;
use ultros_xiv_icons::get_item_image;
use universalis::{ItemId, ListingView, UniversalisClient, WorldId};

use self::character_verifier_service::CharacterVerifierService;
use self::country_code_decoder::Region;
use self::error::{ApiError, WebError};
use self::oauth::{AuthUserCache, DiscordAuthConfig};
use crate::analyzer_service::AnalyzerService;
use crate::event::{EventReceivers, EventSenders, EventType};
use crate::leptos::create_leptos_app;
use crate::search_service::SearchService;
use crate::web::api::characters::{
    character_search, claim_character, pending_verifications, unclaim_character, user_characters,
    verify_character,
};
use crate::web::api::lists::{
    create_list, delete_list, delete_list_item, delete_multiple_list_items, edit_list,
    edit_list_item, get_list, get_list_with_listings, get_lists, post_item_to_list,
    post_items_to_list,
};
use crate::web::api::real_time_data::real_time_data;
use crate::web::api::retainers::{
    add_retainer, claim_retainer, remove_owned_retainer, reorder_retainer, retainer_listings,
    retainer_search, unclaim_retainer, user_retainer_listings, user_retainers,
};
use crate::web::api::user::{current_user, delete_user};
use crate::web::api::{cheapest_per_world, get_best_deals, get_trends, recent_sales};
use crate::web::sitemap::{generic_pages_sitemap, item_sitemap, sitemap_index, world_sitemap};
use crate::web::{
    alerts_websocket::connect_websocket,
    item_card::item_card,
    oauth::{begin_login, logout},
};
use crate::web_metrics::{start_metrics_server, track_metrics};

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

#[derive(Clone)]
pub(crate) struct WebState {
    pub(crate) db: UltrosDb,
    pub(crate) key: Key,
    pub(crate) oauth_config: DiscordAuthConfig,
    pub(crate) user_cache: AuthUserCache,
    pub(crate) event_receivers: EventReceivers,
    pub(crate) event_senders: EventSenders,
    pub(crate) world_cache: Arc<WorldCache>,
    /// Common variant of world_cache. Maybe get rid of world_cache?
    pub(crate) world_helper: Arc<WorldHelper>,
    pub(crate) analyzer_service: AnalyzerService,
    pub(crate) character_verification: CharacterVerifierService,
    pub(crate) leptos_options: LeptosOptions,
    pub(crate) search_service: SearchService,
    pub(crate) token: CancellationToken,
}

impl FromRef<WebState> for UltrosDb {
    fn from_ref(input: &WebState) -> Self {
        input.db.clone()
    }
}

impl FromRef<WebState> for Key {
    fn from_ref(input: &WebState) -> Self {
        input.key.clone()
    }
}

impl FromRef<WebState> for DiscordAuthConfig {
    fn from_ref(input: &WebState) -> Self {
        input.oauth_config.clone()
    }
}

impl FromRef<WebState> for AuthUserCache {
    fn from_ref(input: &WebState) -> Self {
        input.user_cache.clone()
    }
}

impl FromRef<WebState> for EventReceivers {
    fn from_ref(input: &WebState) -> Self {
        input.event_receivers.clone()
    }
}

impl FromRef<WebState> for Arc<WorldCache> {
    fn from_ref(input: &WebState) -> Self {
        input.world_cache.clone()
    }
}

impl FromRef<WebState> for Arc<WorldHelper> {
    fn from_ref(input: &WebState) -> Self {
        input.world_helper.clone()
    }
}

impl FromRef<WebState> for AnalyzerService {
    fn from_ref(input: &WebState) -> Self {
        input.analyzer_service.clone()
    }
}

impl FromRef<WebState> for EventSenders {
    fn from_ref(input: &WebState) -> Self {
        input.event_senders.clone()
    }
}

impl FromRef<WebState> for CharacterVerifierService {
    fn from_ref(input: &WebState) -> Self {
        input.character_verification.clone()
    }
}

impl FromRef<WebState> for LeptosOptions {
    fn from_ref(input: &WebState) -> Self {
        input.leptos_options.clone()
    }
}

impl FromRef<WebState> for SearchService {
    fn from_ref(input: &WebState) -> Self {
        input.search_service.clone()
    }
}

/// In release mode, return the files from a statically included dir
#[cfg(not(debug_assertions))]
fn get_static_file(path: &str) -> Option<&'static [u8]> {
    use include_dir::include_dir;
    static STATIC_DIR: include_dir::Dir = include_dir!("$CARGO_MANIFEST_DIR/static");
    let dir = &STATIC_DIR;
    let file = dir.get_file(path)?;
    Some(file.contents())
}

/// In debug mode, just load the files from disk
#[cfg(debug_assertions)]
fn get_static_file(path: &str) -> Option<Vec<u8>> {
    use std::{io::Read, path::PathBuf};

    let file = PathBuf::from("./ultros/static").join(path);
    let mut file = std::fs::File::open(file).ok()?;
    let mut vec = Vec::new();
    file.read_to_end(&mut vec).ok()?;
    Some(vec)
}

async fn get_file(path: &str) -> Result<impl IntoResponse + use<>, WebError> {
    let mime_type = mime_guess::from_path(path).first_or_text_plain();
    match get_static_file(path) {
        None => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::new(http_body_util::Empty::new()))?),
        Some(file) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_str(mime_type.as_ref()).unwrap(),
            )
            .header(
                header::CACHE_CONTROL,
                #[cfg(not(debug_assertions))]
                HeaderValue::from_str("public, max-age=86400").unwrap(),
                #[cfg(debug_assertions)]
                HeaderValue::from_str("none").unwrap(),
            )
            .body(Body::new(http_body_util::Full::from(file)))?),
    }
}

async fn favicon() -> impl IntoResponse {
    get_file("favicon.ico").await
}

async fn robots() -> impl IntoResponse {
    get_file("robots.txt").await
}

async fn static_path(Path(path): Path<String>) -> impl IntoResponse {
    let path = path.trim_start_matches('/');
    get_file(path).await
}

#[derive(Deserialize)]
struct IconQuery {
    size: IconSize,
}

async fn fallback_item_icon() -> impl IntoResponse {
    let fallback_image = include_bytes!("../static/fallback-image.png");
    (TypedHeader(ContentType::png()), fallback_image)
}

async fn get_item_icon(
    Path(item_id): Path<u32>,
    Query(query): Query<IconQuery>,
) -> Result<impl IntoResponse, WebError> {
    let bytes =
        get_item_image(item_id as i32, query.size).ok_or(anyhow::anyhow!("Failed to get icon"))?;
    let mime_type = mime_guess::from_path("icon.webp").first_or_text_plain();
    let age_header = HeaderValue::from_static("max-age=86400");
    Ok(Response::builder()
        .header(header::CACHE_CONTROL, age_header)
        .header(header::CONTENT_TYPE, mime_type.as_ref())
        .body(body::Body::new(http_body_util::Full::from(bytes)))?)
}

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
    let db = &db;
    // get item ids
    let item_ids: HashSet<i32> = item_ids.split(',').map(|id| id.parse()).try_collect()?;
    // now perform lookups for all the listings for each world/item pair
    let listings = try_join_all(item_ids.into_iter().map(|item| async move {
        db.get_all_listings_in_worlds_with_retainers(worlds, ItemId(item))
            .await
            // map the result to have the item id at the front.
            .map(|res| (item, res))
    }))
    .await?;
    // now convert the database models to API types.
    let listings = listings
        .into_iter()
        .map(|(id, l)| {
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

async fn get_bincode() -> &'static [u8] {
    xiv_gen_db::bincode()
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
        .route("/api/v1/recentSales/{world}", get(recent_sales))
        .route(
            "/api/v1/listings/{world}/{itemid}",
            get(world_item_listings),
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
        .route("/api/v1/list/{id}/listings", get(get_list_with_listings))
        .route("/api/v1/list/{id}/add/item", post(post_item_to_list))
        .route("/api/v1/list/{id}/add/items", post(post_items_to_list))
        .route("/api/v1/list/{id}/delete", delete(delete_list))
        .route("/api/v1/list/item/{id}/delete", delete(delete_list_item))
        .route("/api/v1/list/item/delete", post(delete_multiple_list_items))
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
        .route(
            &["/static/data/", xiv_gen::data_version(), ".bincode"].concat(),
            get(get_bincode),
        )
        .route("/redirect", get(self::oauth::redirect))
        .route("/login", get(begin_login))
        .route("/logout", get(logout))
        .route("/api/v1/current_user", delete(delete_user))
        .route("/invitebot", get(invite))
        .route("/favicon.ico", get(favicon))
        .route("/robots.txt", get(robots))
        .route("/itemcard/{world}/{id}", get(item_card))
        .route("/sitemap/world/{s}", get(world_sitemap))
        .route("/sitemap/items.xml", get(item_sitemap))
        .route("/sitemap.xml", get(sitemap_index))
        .route("/sitemap/pages.xml", get(generic_pages_sitemap))
        .route("/listings/{world}/{item}", get(listings_redirect))
        .merge(create_leptos_app(state.world_helper.clone()).await.unwrap())
        .fallback(leptos_axum::file_and_error_handler_with_context::<
            WebState,
            _,
        >(
            move || {
                provide_context(LocalWorldData(Ok(worlds.clone())));
            },
            shell,
        ))
        .with_state(state)
        .route_layer(middleware::from_fn(track_metrics))
        .layer(TraceLayer::new_for_http())
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
