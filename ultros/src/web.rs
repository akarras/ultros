mod alerts_websocket;
pub(crate) mod character_verifier_service;
pub mod error;
mod fuzzy_item_search;
mod home_world_cookie;
pub mod item_search_index;
pub mod oauth;
mod templates;

use anyhow::Error;
use axum::body::{Empty, Full};
use axum::extract::{FromRef, Path, Query, State};
use axum::http::{HeaderValue, Response, StatusCode};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use axum::{body, Router};
use axum_extra::extract::cookie::{Cookie, Key, SameSite};
use axum_extra::extract::CookieJar;
use futures::future::join;
use image::imageops::FilterType;
use image::ImageOutputFormat;
use maud::Render;
use opentelemetry_prometheus::PrometheusExporter;
use reqwest::header;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use tokio::time::timeout;
use tracing::debug;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use ultros_db::UltrosDb;
use universalis::{ItemId, ListingView, UniversalisClient, WorldId};

use self::character_verifier_service::CharacterVerifierService;
use self::error::WebError;
use self::home_world_cookie::HomeWorld;
use self::oauth::{AuthDiscordUser, AuthUserCache, DiscordAuthConfig};
use self::templates::pages::retainer::generic_retainer_page::GenericRetainerPage;
use self::templates::pages::{
    alerts::AlertsPage, analyzer_page::AnalyzerPage, retainer::add_retainer::AddRetainer,
};
use self::templates::{
    page::RenderPage,
    pages::{
        home_page::HomePage,
        listings_view::ListingsPage,
        retainer::user_retainers_page::{RetainerViewType, UserRetainersPage},
    },
};
use crate::analyzer_service::{AnalyzerService, ResaleOptions};
use crate::event::{EventReceivers, EventSenders, EventType};
use crate::metrics::metrics;
use crate::web::templates::pages::character::add_character::add_character;
use crate::{
    web::{
        alerts_websocket::connect_websocket,
        oauth::{begin_login, logout},
        templates::pages::{profile::profile, retainer::edit_retainer::edit_retainer},
    },
    world_cache::{AnySelector, WorldCache},
};
use image::io::Reader as ImageReader;
use std::io::Cursor;

// basic handler that responds with a static string
async fn root(user: Option<AuthDiscordUser>) -> RenderPage<HomePage> {
    RenderPage(HomePage { user })
}

async fn get_retainer_listings(
    State(db): State<ultros_db::UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(retainer_id): Path<i32>,
    user: Option<AuthDiscordUser>,
) -> Result<RenderPage<GenericRetainerPage>, WebError> {
    let data = db
        .get_retainer_listings(retainer_id)
        .await?
        .ok_or(WebError::InvalidItem(retainer_id))?;
    let (retainer, listings) = data;
    // get all listings from the retainer and calculate heuristics
    //let multiple_listings = db
    //    .get_multiple_listings_for_worlds(
    //        [WorldId(retainer.world_id)].into_iter(),
    //        listings.iter().map(|i| ItemId(i.item_id)),
    //    )
    //    .await?;

    Ok(RenderPage(GenericRetainerPage {
        retainer_name: retainer.name,
        retainer_id: retainer.id,
        world_name: world_cache
            .lookup_selector(&AnySelector::World(retainer.world_id))
            .map(|w| w.get_name().to_string())
            .unwrap_or_default(),
        listings,
        user,
    }))
}

async fn user_retainers_listings(
    State(db): State<UltrosDb>,
    current_user: AuthDiscordUser,
) -> Result<RenderPage<UserRetainersPage>, WebError> {
    let (owned_retainers, mut retainer_listings) = db
        .get_retainer_listings_for_discord_user(current_user.id)
        .await?;
    let items = &xiv_gen_db::decompress_data().items;
    // sort the undercut retainers by item sort ui category to match in game
    for (_, listings) in &mut retainer_listings {
        listings.sort_by(|a, b| {
            let item_a = items
                .get(&xiv_gen::ItemId(a.item_id))
                .expect("Unknown item");
            let item_b = items
                .get(&xiv_gen::ItemId(b.item_id))
                .expect("Unknown item");
            item_a
                .item_ui_category
                .0
                .cmp(&item_b.item_ui_category.0)
                .then_with(|| item_a.name.cmp(&item_b.name))
        });
    }
    Ok(RenderPage(UserRetainersPage {
        character_names: Vec::new(),
        view_type: RetainerViewType::Listings(retainer_listings),
        current_user,
        owned_retainers,
    }))
}

async fn user_retainers_undercuts(
    State(db): State<UltrosDb>,
    current_user: AuthDiscordUser,
) -> Result<RenderPage<UserRetainersPage>, WebError> {
    let (owned_retainers, mut undercut_retainers) =
        db.get_retainer_undercut_items(current_user.id).await?;
    let items = &xiv_gen_db::decompress_data().items;
    // sort the undercut retainers by item sort ui category to match in game
    for (_, listings) in &mut undercut_retainers {
        listings.sort_by(|(a, _), (b, _)| {
            let item_a = items
                .get(&xiv_gen::ItemId(a.item_id))
                .expect("Unknown item");
            let item_b = items
                .get(&xiv_gen::ItemId(b.item_id))
                .expect("Unknown item");
            item_a
                .item_ui_category
                .0
                .cmp(&item_b.item_ui_category.0)
                .then_with(|| item_a.name.cmp(&item_b.name))
        });
    }
    Ok(RenderPage(UserRetainersPage {
        character_names: Vec::new(),
        view_type: RetainerViewType::Undercuts(undercut_retainers),
        current_user,
        owned_retainers,
    }))
}

#[derive(Deserialize)]
struct RetainerAddQueryParams {
    search: Option<String>,
}

async fn add_retainer_page(
    State(db): State<UltrosDb>,
    current_user: AuthDiscordUser,
    Query(query_parameter): Query<RetainerAddQueryParams>,
) -> Result<RenderPage<AddRetainer>, WebError> {
    let mut results = None;
    if let Some(search_str) = &query_parameter.search {
        results = Some(db.search_retainers(search_str).await?);
    }

    Ok(RenderPage(AddRetainer {
        user: Some(current_user),
        search_results: results.unwrap_or_default(),
        search_text: query_parameter.search.unwrap_or_default(),
    }))
}

async fn add_retainer(
    State(db): State<UltrosDb>,
    current_user: AuthDiscordUser,
    Path(retainer_id): Path<i32>,
) -> Result<Redirect, WebError> {
    let _register_retainer = db
        .register_retainer(retainer_id, current_user.id, current_user.name)
        .await?;
    Ok(Redirect::to("/retainers"))
}

async fn remove_owned_retainer(
    State(db): State<UltrosDb>,
    current_user: AuthDiscordUser,
    Path(retainer_id): Path<i32>,
) -> Result<Redirect, WebError> {
    db.remove_owned_retainer(current_user.id, retainer_id)
        .await?;
    Ok(Redirect::to("/retainers"))
}

async fn world_item_listings(
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    Path((world, item_id)): Path<(String, i32)>,
    user: Option<AuthDiscordUser>,
    home_world: Option<HomeWorld>,
    cookie_jar: CookieJar,
) -> Result<(CookieJar, RenderPage<ListingsPage>), WebError> {
    let selected_value = world_cache.lookup_value_by_name(&world)?;
    let worlds = world_cache
        .get_all_worlds_in(&selected_value)
        .ok_or_else(|| Error::msg("Unable to get worlds"))?;
    let db_clone = db.clone();
    let world_iter = worlds.iter().copied();
    let (listings, sale_history) = join(
        db_clone.get_all_listings_in_worlds_with_retainers(&worlds, ItemId(item_id)),
        db.get_sale_history_with_characters_from_multiple_worlds_single(world_iter, item_id, 25),
    )
    .await;
    let listings = listings?;
    let sale_history = sale_history?;
    let item = xiv_gen_db::decompress_data()
        .items
        .get(&xiv_gen::ItemId(item_id))
        .ok_or(WebError::InvalidItem(item_id))?;
    let page = ListingsPage {
        listings,
        selected_world: selected_value.get_name().to_string(),
        item_id,
        item,
        user,
        world_cache,
        sale_history,
        home_world,
    };
    let cookie = Cookie::build("last_listing_view", world)
        .permanent()
        .path("/")
        .same_site(SameSite::Lax)
        .finish();
    let cookie_jar = cookie_jar.add(cookie);
    Ok((cookie_jar, RenderPage(page)))
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
        let client = UniversalisClient::new();
        let current_data = client
            .marketboard_current_data(&world_clone, &[item_id])
            .await?;
        // we can potentially get listings from multiple worlds from this call so we should group listings by world
        let listings = match current_data {
            universalis::MarketView::SingleView(v) => v.listings,
            universalis::MarketView::MultiView(_) => {
                return Result::<_, anyhow::Error>::Err(
                    anyhow::Error::msg("multiple listings returned?").into(),
                )
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
            senders.listings.send(EventType::Add(Arc::new(added)))?;
            senders
                .listings
                .send(EventType::Remove(Arc::new(removed)))?;
        }
        Ok(())
    });
    let _ = timeout(Duration::from_secs(1), future).await?;
    Ok(Redirect::to(&format!("/listings/{world}/{item_id}")))
}

async fn alerts(discord_user: AuthDiscordUser) -> Result<RenderPage<AlertsPage>, WebError> {
    Ok(RenderPage(AlertsPage { discord_user }))
}

#[derive(Deserialize, Serialize, Copy, Clone, PartialEq, PartialOrd, Eq, Ord)]
#[serde(rename_all = "camelCase")]
pub enum AnalyzerSort {
    Profit,
    Margin,
}

impl Render for AnalyzerSort {
    fn render(&self) -> maud::Markup {
        maud::PreEscaped(
            match self {
                AnalyzerSort::Profit => "profit",
                AnalyzerSort::Margin => "margin",
            }
            .to_string(),
        )
    }
}

#[serde_as]
#[derive(Deserialize, Serialize, Clone)]
pub struct AnalyzerOptions {
    sort: Option<AnalyzerSort>,
    page: Option<usize>,
    days: Option<i32>,
    minimum_profit: Option<i32>,
    world: Option<i32>,
    filter_world: Option<i32>,
    filter_datacenter: Option<i32>,
}

async fn analyze_profits(
    State(analyzer): State<AnalyzerService>,
    State(world_cache): State<Arc<WorldCache>>,
    home_world: Option<HomeWorld>,
    user: Option<AuthDiscordUser>,
    Query(options): Query<AnalyzerOptions>,
) -> Result<RenderPage<AnalyzerPage>, WebError> {
    // this doesn't change often, could easily cache.
    let world = if let Some(world) = options.world {
        world
    } else if let Some(home_world) = &home_world {
        home_world.home_world
    } else {
        return Ok(RenderPage(AnalyzerPage {
            user,
            analyzer_results: vec![],
            world: None,
            region: None,
            options,
            world_cache,
        }));
    };
    let world = world_cache.lookup_selector(&AnySelector::World(world))?;
    let region = world_cache
        .get_region(&world)
        .ok_or_else(|| anyhow::Error::msg("Unable to get region"))?;
    let world = match world {
        crate::world_cache::AnyResult::World(w) => w,
        crate::world_cache::AnyResult::Datacenter(_) => {
            return Err(Error::msg("Datacenter found?").into())
        }
        crate::world_cache::AnyResult::Region(_) => {
            return Err(Error::msg("Region not found").into())
        }
    };
    let mut analyzer_results = analyzer
        .get_best_resale(
            world.id,
            region.id,
            ResaleOptions {
                days: options.days.unwrap_or(10),
                minimum_profit: options.minimum_profit,
                filter_world: options.filter_world,
                filter_datacenter: options.filter_datacenter,
            },
            &world_cache,
        )
        .await
        .ok_or_else(|| anyhow::Error::msg("Couldn't find items. Might need more warm up time"))?;
    match options.sort.as_ref().unwrap_or(&AnalyzerSort::Profit) {
        AnalyzerSort::Profit => {
            analyzer_results.sort_by(|a, b| {
                b.profit
                    .cmp(&a.profit)
                    .then_with(|| a.cheapest.cmp(&b.cheapest))
            });
        }
        AnalyzerSort::Margin => {
            analyzer_results.sort_by(|a, b| {
                b.return_on_investment
                    .partial_cmp(&a.return_on_investment)
                    .unwrap_or_else(|| a.cheapest.cmp(&b.cheapest))
            });
        }
    }

    Ok(RenderPage(AnalyzerPage {
        user,
        analyzer_results,
        region: Some(region.clone()),
        world: Some(world.clone()),
        options,
        world_cache,
    }))
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
    pub(crate) analyzer_service: AnalyzerService,
    pub(crate) analytics_exporter: Arc<PrometheusExporter>,
    pub(crate) character_verification: CharacterVerifierService,
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

impl FromRef<WebState> for AnalyzerService {
    fn from_ref(input: &WebState) -> Self {
        input.analyzer_service.clone()
    }
}

impl FromRef<WebState> for Arc<PrometheusExporter> {
    fn from_ref(input: &WebState) -> Self {
        input.analytics_exporter.clone()
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

async fn get_file(path: &str) -> Result<impl IntoResponse, WebError> {
    let mime_type = mime_guess::from_path(path).first_or_text_plain();
    match get_static_file(path) {
        None => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(body::boxed(Empty::new()))?),
        Some(file) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_str(mime_type.as_ref()).unwrap(),
            )
            .header(
                header::CACHE_CONTROL,
                #[cfg(not(debug_assertions))]
                HeaderValue::from_str("max-age=86400").unwrap(),
                #[cfg(debug_assertions)]
                HeaderValue::from_str("none").unwrap(),
            )
            .body(body::boxed(Full::from(file)))?),
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
    size: u32,
}

async fn get_item_icon(
    Path(item_id): Path<u32>,
    Query(query): Query<IconQuery>,
) -> Result<impl IntoResponse, WebError> {
    let url =
        format!("https://universalis-ffxiv.github.io/universalis-assets/icon2x/{item_id}.png");
    let image = reqwest::get(url).await?;
    let bytes = image.bytes().await?;
    let mime_type = mime_guess::from_path("icon.webp").first_or_text_plain();
    let age_header = HeaderValue::from_str("max-age=86400").unwrap();
    let img = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()?
        .decode()?;
    let smaller_image = img.resize(query.size, query.size, FilterType::Lanczos3);
    let file = vec![];
    let mut cursor = Cursor::new(file);
    smaller_image.write_to(&mut cursor, ImageOutputFormat::WebP)?;
    let bytes = cursor.into_inner();
    Ok(Response::builder()
        .header(header::CACHE_CONTROL, age_header)
        .header(header::CONTENT_TYPE, mime_type.as_ref())
        .body(body::boxed(Full::from(bytes)))?)
}

pub(crate) async fn invite() -> Redirect {
    let client_id = std::env::var("DISCORD_CLIENT_ID").expect("Unable to get DISCORD_CLIENT_ID");
    Redirect::to(&format!("https://discord.com/oauth2/authorize?client_id={client_id}&scope=bot&permissions=2147483648"))
}

pub(crate) async fn start_web(state: WebState) {
    // build our application with a route
    let app = Router::with_state(state)
        .route("/", get(root))
        .route("/alerts", get(alerts))
        .route("/alerts/websocket", get(connect_websocket))
        .route("/listings/:world/:itemid", get(world_item_listings))
        .route(
            "/listings/refresh/:worldid/:itemid",
            get(refresh_world_item_listings),
        )
        .route("/characters/add", get(add_character))
        .route("/retainers/listings/:id", get(get_retainer_listings))
        .route("/retainers/undercuts", get(user_retainers_undercuts))
        .route("/retainers/listings", get(user_retainers_listings))
        .route("/retainers/add", get(add_retainer_page))
        .route("/retainers/add/:id", get(add_retainer))
        .route("/retainers/remove/:id", get(remove_owned_retainer))
        .route("/retainers/edit", get(edit_retainer))
        .route("/retainers", get(user_retainers_listings))
        .route("/analyzer", get(analyze_profits))
        .route("/items/:search", get(fuzzy_item_search::search_items))
        .route("/static/*path", get(static_path))
        .route("/static/itemicon/:path", get(get_item_icon))
        .route("/redirect", get(self::oauth::redirect))
        .route("/profile", get(profile))
        .route("/login", get(begin_login))
        .route("/logout", get(logout))
        .route("/invitebot", get(invite))
        .route("/metrics", get(metrics))
        .route("/favicon.ico", get(favicon))
        .route("/robots.txt", get(robots))
        .fallback(fallback);

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    let port = std::env::var("PORT")
        .map(|p| p.parse::<u16>().ok())
        .ok()
        .flatten()
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn fallback() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "Not found")
}
