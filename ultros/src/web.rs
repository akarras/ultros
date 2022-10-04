mod alerts_websocket;
pub mod error;
mod fuzzy_item_search;
pub mod item_search_index;
pub mod oauth;
mod templates;

use anyhow::Error;
use axum::body::{Empty, Full};
use axum::extract::{FromRef, Path, Query, State};
use axum::http::{HeaderValue, Response, StatusCode};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::get;
use axum::{body, Router};
use axum_extra::extract::cookie::Key;
use reqwest::header;
use serde::Deserialize;
use std::fmt::Write;
use std::io::Read;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use ultros_db::UltrosDb;
use universalis::{ItemId, WorldId};
use xiv_gen::ItemId as XivDBItemId;

use self::error::WebError;
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
use crate::event::EventReceivers;
use crate::web::alerts_websocket::connect_websocket;
use crate::web::oauth::{begin_login, logout};
use crate::world_cache::{AnySelector, WorldCache};

// basic handler that responds with a static string
async fn root(user: Option<AuthDiscordUser>) -> RenderPage<HomePage> {
    RenderPage(HomePage { user })
}

async fn search_retainers(
    State(db): State<ultros_db::UltrosDb>,
    Path(search): Path<String>,
) -> Result<Html<String>, StatusCode> {
    let retainers = db
        .search_retainers(&search)
        .await
        .map_err(|e| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut string = String::new();
    write!(
        string,
        "<table><tr><th>retainer name</th><th>retainer id</th><th>world id</th><th>world name</th></tr>"
    ).unwrap();
    for (retainer, world) in retainers {
        write!(
            &mut string,
            "<tr><td><a href=\"/listings/retainer/{}\">{}</a></td><td>{}<td><td>{}</td></tr>",
            retainer.id,
            retainer.name,
            retainer.world_id,
            world
                .map(|w| w.name)
                .unwrap_or(retainer.world_id.to_string())
        )
        .unwrap();
    }
    write!(string, "</table>").unwrap();
    Ok(Html(string))
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

    let game_data = xiv_gen_db::decompress_data();
    let items = &game_data.items;
    let (retainer, listings) = data;
    let data = format!("<h1>{}</h1>", retainer.name);
    // get all listings from the retainer and calculate heuristics
    let multiple_listings = db
        .get_multiple_listings_for_worlds(
            [WorldId(retainer.world_id)].into_iter(),
            listings.iter().map(|i| ItemId(i.item_id)),
        )
        .await?;

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
    let (owned_retainers, retainer_listings) = db
        .get_retainer_listings_for_discord_user(current_user.id)
        .await?;
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
    let (owned_retainers, undercut_retainers) =
        db.get_retainer_undercut_items(current_user.id).await?;
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
) -> Result<RenderPage<ListingsPage>, WebError> {
    let selected_value = world_cache
        .lookup_value_by_name(&world)
        .ok_or(Error::msg("Unable to find world/datacenter"))?;
    let worlds = world_cache
        .get_all_worlds_in(&selected_value)
        .ok_or(Error::msg("Unable to get worlds"))?;
    let listings = db
        .get_all_listings_in_worlds_with_retainers(&worlds, ItemId(item_id))
        .await?;
    let region = world_cache
        .get_region(&selected_value)
        .ok_or(Error::msg("No region found?"))?;
    let datacenter = world_cache
        .get_datacenters(&selected_value)
        .ok_or(Error::msg("No datacenter found"))?;
    let item = xiv_gen_db::decompress_data()
        .items
        .get(&xiv_gen::ItemId(item_id))
        .ok_or(WebError::InvalidItem(item_id))?;
    let mut world_names: Vec<_> = worlds
        .iter()
        .flat_map(|i| {
            let world = AnySelector::World(*i);
            world_cache.lookup_selector(&world)
        })
        .map(|selector| selector.get_name().to_string())
        .collect();
    world_names.push(region.name.clone());
    for dc in datacenter {
        world_names.push(dc.name.clone());
    }
    let page = ListingsPage {
        listings,
        selected_world: selected_value.get_name().to_string(),
        worlds: world_names,
        item_id,
        item,
        user,
        world_cache,
    };
    Ok(RenderPage(page))
}

async fn alerts(discord_user: AuthDiscordUser) -> Result<RenderPage<AlertsPage>, WebError> {
    Ok(RenderPage(AlertsPage { discord_user }))
}

#[derive(Deserialize)]
struct ProfitParameters {
    sale_amount_threshold: Option<i32>,
    sale_window_days: Option<i64>,
    world: Option<String>,
}

async fn analyze_profits(
    State(db): State<UltrosDb>,
    Query(parameters): Query<ProfitParameters>,
    user: Option<AuthDiscordUser>,
) -> Result<RenderPage<AnalyzerPage>, WebError> {
    let ProfitParameters {
        sale_amount_threshold,
        sale_window_days,
        world,
    } = &parameters;
    // this doesn't change often, could easily cache.
    let world = db
        .get_world(&world.as_ref().map(|w| w.as_str()).unwrap_or("Sargatanas"))
        .await?;
    let datacenter = db.get_datacenter_from_world(&world).await?;
    let region = db.get_region_from_datacenter(&datacenter).await?;
    let analyzer_results = db
        .get_best_item_to_resell_on_world(
            world.id,
            sale_amount_threshold.unwrap_or(10),
            chrono::Duration::days(sale_window_days.unwrap_or(2)),
        )
        .await?;

    Ok(RenderPage(AnalyzerPage {
        user,
        analyzer_results,
        region: region,
        world,
    }))
}

#[derive(Clone)]
pub(crate) struct WebState {
    pub(crate) db: UltrosDb,
    pub(crate) key: Key,
    pub(crate) oauth_config: DiscordAuthConfig,
    pub(crate) user_cache: AuthUserCache,
    pub(crate) event_receivers: EventReceivers,
    pub(crate) world_cache: Arc<WorldCache>,
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
    let file = PathBuf::from("./ultros/static").join(path);
    let mut file = std::fs::File::open(file).ok()?;
    let mut vec = Vec::new();
    file.read_to_end(&mut vec).ok()?;
    Some(vec)
}

async fn static_path(Path(path): Path<String>) -> impl IntoResponse {
    let path = path.trim_start_matches('/');
    let mime_type = mime_guess::from_path(path).first_or_text_plain();
    match get_static_file(&path) {
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(body::boxed(Empty::new()))
            .unwrap(),
        Some(file) => Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_str(mime_type.as_ref()).unwrap(),
            )
            .header(
                header::CACHE_CONTROL,
                #[cfg(not(debug_assertions))]
                HeaderValue::from_str("max-age=3600").unwrap(),
                #[cfg(debug_assertions)]
                HeaderValue::from_str("none").unwrap(),
            )
            .body(body::boxed(Full::from(file)))
            .unwrap(),
    }
}

pub(crate) async fn start_web(state: WebState) {
    // build our application with a route
    let app = Router::with_state(state)
        .route("/", get(root))
        .route("/alerts", get(alerts))
        .route("/alerts/websocket", get(connect_websocket))
        .route("/retainer/search/:search", get(search_retainers))
        .route("/listings/:world/:itemid", get(world_item_listings))
        .route("/retainers/listings/:id", get(get_retainer_listings))
        .route("/retainers/undercuts", get(user_retainers_undercuts))
        .route("/retainers/listings", get(user_retainers_listings))
        .route("/retainers/add", get(add_retainer_page))
        .route("/retainers/add/:id", get(add_retainer))
        .route("/retainers/remove/:id", get(remove_owned_retainer))
        .route("/retainers", get(user_retainers_listings))
        .route("/analyzer", get(analyze_profits))
        .route("/items/:search", get(fuzzy_item_search::search_items))
        .route("/static/*path", get(static_path))
        .route("/redirect", get(self::oauth::redirect))
        .route("/login", get(begin_login))
        .route("/logout", get(logout))
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
