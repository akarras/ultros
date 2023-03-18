mod alerts_websocket;
pub mod api;
pub(crate) mod character_verifier_service;
pub mod error;
pub mod oauth;
pub mod sitemap;

use anyhow::Error;
use axum::body::{Empty, Full};
use axum::extract::{FromRef, Path, Query, State};
use axum::http::{HeaderValue, Response, StatusCode};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{body, middleware, Json, Router};
use axum_extra::extract::cookie::Key;
use futures::future::{try_join, try_join_all};
use futures::stream::TryStreamExt;
use futures::{stream, StreamExt};
use itertools::Itertools;
use maud::Render;
use reqwest::header;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tower_http::compression::CompressionLayer;
use tracing::debug;
use ultros_api_types::icon_size::IconSize;
use ultros_api_types::list::{CreateList, List, ListItem};
use ultros_api_types::user::{OwnedRetainer, UserData, UserRetainerListings, UserRetainers};
use ultros_api_types::world::WorldData;
use ultros_api_types::{
    ActiveListing, CurrentlyShownItem, FfxivCharacter, FfxivCharacterVerification, Retainer,
};
use ultros_db::common_type_conversions::ApiConversionError;
use ultros_db::world_cache::AnySelector;
use ultros_db::ActiveValue;
use ultros_db::{world_cache::WorldCache, UltrosDb};
use ultros_xiv_icons::get_item_image;
use universalis::{ItemId, ListingView, UniversalisClient, WorldId};

use self::character_verifier_service::CharacterVerifierService;
use self::error::{ApiError, WebError};
use self::oauth::{AuthDiscordUser, AuthUserCache, DiscordAuthConfig};
use crate::analyzer_service::AnalyzerService;
use crate::event::{EventReceivers, EventSenders, EventType, ListingData};
use crate::leptos::create_leptos_app;
use crate::web::api::{cheapest_per_world, recent_sales};
use crate::web::sitemap::{sitemap_index, world_sitemap};
use crate::web::{
    alerts_websocket::connect_websocket,
    oauth::{begin_login, logout},
};
use crate::web_metrics::{start_metrics_server, track_metrics};

async fn add_retainer(
    State(db): State<UltrosDb>,
    current_user: AuthDiscordUser,
    Path(retainer_id): Path<i32>,
) -> Result<Redirect, WebError> {
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
        db.get_sale_history_from_multiple_worlds(world_iter, item_id, 1000),
    )
    .await?;
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
            senders.listings.send(EventType::Add(Arc::new(ListingData {
                item_id: ItemId(item_id),
                world_id: WorldId(world_id.into()),
                listings: added,
            })))?;
            senders
                .listings
                .send(EventType::Remove(Arc::new(ListingData {
                    item_id: ItemId(item_id),
                    world_id: WorldId(world_id.into()),
                    listings: removed,
                })))?;
        }
        Ok(())
    });
    let _ = timeout(Duration::from_secs(1), future).await?;
    Ok(Redirect::to(&format!("/item/{world}/{item_id}")))
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

#[derive(Deserialize, Serialize, Clone)]
pub enum SaleTimeLabel {
    NoFilter,
    Today,
    Week,
    Month,
    Year,
}

impl Render for SaleTimeLabel {
    fn render(&self) -> maud::Markup {
        maud::PreEscaped(match self {
            SaleTimeLabel::NoFilter => "No Filter".to_string(),
            SaleTimeLabel::Today => "Today".to_string(),
            SaleTimeLabel::Week => "Week".to_string(),
            SaleTimeLabel::Month => "Month".to_string(),
            SaleTimeLabel::Year => "Year".to_string(),
        })
    }
}

#[serde_as]
#[derive(Deserialize, Serialize, Clone)]
pub struct AnalyzerOptions {
    sort: Option<AnalyzerSort>,
    page: Option<usize>,
    minimum_profit: Option<i32>,
    world: Option<i32>,
    filter_world: Option<i32>,
    filter_datacenter: Option<i32>,
    sale_label: Option<SaleTimeLabel>,
    sale_value: Option<u8>,
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
    size: IconSize,
}

async fn get_item_icon(
    Path(item_id): Path<u32>,
    Query(query): Query<IconQuery>,
) -> Result<impl IntoResponse, WebError> {
    let bytes =
        get_item_image(item_id as i32, query.size).ok_or(anyhow::anyhow!("Failed to get icon"))?;
    let mime_type = mime_guess::from_path("icon.webp").first_or_text_plain();
    let age_header = HeaderValue::from_str("max-age=86400").unwrap();
    Ok(Response::builder()
        .header(header::CACHE_CONTROL, age_header)
        .header(header::CONTENT_TYPE, mime_type.as_ref())
        .body(body::boxed(Full::from(bytes)))?)
}

pub(crate) async fn invite() -> Redirect {
    let client_id = std::env::var("DISCORD_CLIENT_ID").expect("Unable to get DISCORD_CLIENT_ID");
    Redirect::to(&format!("https://discord.com/oauth2/authorize?client_id={client_id}&scope=bot&permissions=2147483648"))
}

pub(crate) async fn world_data(State(world_cache): State<Arc<WorldCache>>) -> Json<WorldData> {
    Json(WorldData::from(world_cache.as_ref()))
}

pub(crate) async fn current_user(user: AuthDiscordUser) -> Json<UserData> {
    Json(UserData {
        id: user.id,
        username: user.name,
        avatar: user.avatar_url,
    })
}

pub(crate) async fn user_retainers(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Json<Option<UserRetainers>> {
    // load the retainer/character details from the database and then extract it into the shared API types.
    let retainers = db
        .get_all_owned_retainers_and_character(user.id)
        .await
        .ok()
        .map(|c| UserRetainers {
            retainers: c
                .into_iter()
                .map(|(character, retainers)| {
                    (
                        character.map(|character| FfxivCharacter::from(character)),
                        retainers
                            .into_iter()
                            .map(|(owned, retainer)| {
                                (OwnedRetainer::from(owned), Retainer::from(retainer))
                            })
                            .collect(),
                    )
                })
                .collect(),
        });
    Json(retainers)
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
                    listings.map(|listings| {
                        (
                            Retainer::from(retainer),
                            listings
                                .map(|(_, listings)| {
                                    listings
                                        .into_iter()
                                        .map(|l| ActiveListing::from(l))
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default(),
                        )
                    })
                }))
                .await;
            retainers_with_listings.map(|r| (character.map(|c| FfxivCharacter::from(c)), r))
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
    let retainers = retainers
        .into_iter()
        .map(|retainers| retainers.into())
        .collect();
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
) -> Result<Json<Vec<List>>, ApiError> {
    let lists = db
        .get_lists_for_user(user.id as i64)
        .await?
        .into_iter()
        .map(|list| List::try_from(list))
        .collect::<Result<Vec<_>, ApiConversionError>>()?;
    Ok(Json(lists))
}

pub(crate) async fn get_list(
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<(List, Vec<ListItem>)>, ApiError> {
    let (list, list_items) = futures::future::try_join(
        db.get_list(id, user.id as i64),
        db.get_list_items(id, user.id as i64),
    )
    .await?;
    let list_items = list_items
        .into_iter()
        .map(|item| ListItem::from(item))
        .collect::<Vec<_>>();
    let list = List::try_from(list)?;
    Ok(Json((list, list_items)))
}

pub(crate) async fn get_list_with_listings(
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<(List, Vec<(ListItem, Vec<ActiveListing>)>)>, ApiError> {
    let (list, list_items) = futures::future::try_join(
        db.get_list(id, user.id as i64),
        db.get_list_items(id, user.id as i64),
    )
    .await?;
    // tbd: probably don't need to send clients all listings, but for now keep it this way.
    let selector = AnySelector::try_from(&list)?;
    let world = world_cache.lookup_selector(&selector)?;
    let world_ids = world_cache
        .get_all_worlds_in(&world)
        .ok_or(anyhow::anyhow!("Bad world id"))?;
    // borrow these for use inside the closure
    let world_ids = &world_ids;
    let db = &db;
    let list_items = stream::iter(list_items.into_iter().map(|list| async move {
        // get alll the listings that match our item list
        let listings = db
            .get_all_listings_in_worlds(&world_ids, ItemId(list.item_id))
            .await;
        listings.map(|listings| {
            // return this as a tuple and bring the list that we moved vec
            (
                ListItem::from(list),
                // convert our new active listing to the API types
                listings
                    .into_iter()
                    .map(|listing| ActiveListing::from(listing))
                    .collect(),
            )
        })
    }))
    .buffered(2)
    .try_collect()
    .await?;

    Ok(Json((List::try_from(list)?, list_items)))
}

pub(crate) async fn delete_list(
    State(db): State<UltrosDb>,
    Path(list_id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<()>, ApiError> {
    db.delete_list(list_id, user.id as i64).await?;
    Ok(Json(()))
}

pub(crate) async fn create_list(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(list): Json<CreateList>,
) -> Result<Json<()>, ApiError> {
    let discord_user = db.get_or_create_discord_user(user.id, user.name).await?;
    db.create_list(discord_user, list.name, Some(list.wdr_filter.into()))
        .await?;
    Ok(Json(()))
}

pub(crate) async fn edit_list(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(list): Json<List>,
) -> Result<Json<()>, ApiError> {
    db.update_list(list.id, user.id as i64, |ulist| {
        ulist.datacenter_id = ActiveValue::Set(match list.wdr_filter {
            ultros_api_types::world_helper::AnySelector::Datacenter(dc) => Some(dc),
            _ => None,
        });
        ulist.region_id = ActiveValue::Set(match list.wdr_filter {
            ultros_api_types::world_helper::AnySelector::Region(region) => Some(region),
            _ => None,
        });
        ulist.world_id = ActiveValue::Set(match list.wdr_filter {
            ultros_api_types::world_helper::AnySelector::World(world) => Some(world),
            _ => None,
        });
        ulist.name = ActiveValue::Set(list.name);
    })
    .await?;
    Ok(Json(()))
}

pub(crate) async fn post_item_to_list(
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
    Json(item): Json<ListItem>,
) -> Result<Json<()>, ApiError> {
    let list = db.get_list(id, user.id as i64).await?;
    let ListItem {
        item_id,
        hq,
        quantity,
        ..
    } = item;
    db.add_item_to_list(&list, user.id as i64, item_id, hq, quantity)
        .await?;
    Ok(Json(()))
}

pub(crate) async fn delete_list_item(
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<()>, ApiError> {
    db.remove_item_from_list(user.id as i64, id).await?;
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
    let db = &db;
    // get item ids
    let item_ids: HashSet<i32> = item_ids.split(",").map(|id| id.parse()).try_collect()?;
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
                        (
                            ActiveListing::from(listing),
                            retainer.map(|retainer| Retainer::from(retainer)),
                        )
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
            let world = cache
                .lookup_value_by_name(world)
                .ok()
                .unwrap_or_else(|| panic!("World {} not found", world));
            let (first_name, last_name) = r
                .name
                .split_once(' ')
                .expect("Should always have first last name");
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

pub(crate) async fn start_web(state: WebState) {
    // build our application with a route
    let app = Router::new()
        .route("/alerts/websocket", get(connect_websocket))
        .route("/api/v1/cheapest/:world", get(cheapest_per_world))
        .route("/api/v1/recentSales/:world", get(recent_sales))
        .route("/api/v1/listings/:world/:itemid", get(world_item_listings))
        .route(
            "/api/v1/bulkListings/:world/:itemids",
            get(bulk_item_listings),
        )
        .route("/api/v1/list", get(get_lists))
        .route("/api/v1/list/create", post(create_list))
        .route("/api/v1/list/edit", post(edit_list))
        .route("/api/v1/list/:id", get(get_list))
        .route("/api/v1/list/:id/listings", get(get_list_with_listings))
        .route("/api/v1/list/:id/add/item", post(post_item_to_list))
        .route("/api/v1/list/:id/delete", get(delete_list))
        .route("/api/v1/list/item/:id/delete", get(delete_list_item))
        .route("/api/v1/world_data", get(world_data))
        .route("/api/v1/current_user", get(current_user))
        .route("/api/v1/user/retainer", get(user_retainers))
        .route("/api/v1/retainer/reorder", post(reorder_retainer))
        .route(
            "/api/v1/user/retainer/listings",
            get(user_retainer_listings),
        )
        .route("/api/v1/retainer/search/:query", get(retainer_search))
        .route("/api/v1/retainer/claim/:id", get(claim_retainer))
        .route("/api/v1/retainer/unclaim/:id", get(unclaim_retainer))
        .route(
            "/item/refresh/:worldid/:itemid",
            get(refresh_world_item_listings),
        )
        .route("/api/v1/characters/search/:name", get(character_search))
        .route("/api/v1/characters/claim/:id", get(claim_character))
        .route("/api/v1/characters/unclaim/:id", get(unclaim_character))
        .route("/api/v1/characters/verify/:id", get(verify_character))
        .route("/api/v1/characters", get(user_characters))
        .route(
            "/api/v1/characters/verifications",
            get(pending_verifications),
        )
        .route("/retainers/add/:id", get(add_retainer))
        .route("/retainers/remove/:id", get(remove_owned_retainer))
        .route("/static/*path", get(static_path))
        .route("/static/itemicon/:path", get(get_item_icon))
        .route("/redirect", get(self::oauth::redirect))
        .route("/login", get(begin_login))
        .route("/logout", get(logout))
        .route("/invitebot", get(invite))
        .route("/favicon.ico", get(favicon))
        .route("/robots.txt", get(robots))
        .route("/sitemap/world/:s.xml", get(world_sitemap))
        .route("/sitemap.xml", get(sitemap_index))
        .nest("/", create_leptos_app().await.unwrap())
        .with_state(state)
        .route_layer(middleware::from_fn(track_metrics))
        .layer(CompressionLayer::new());

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    let port = std::env::var("PORT")
        .map(|p| p.parse::<u16>().ok())
        .ok()
        .flatten()
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("listening on {}", addr);
    let (_main_app, _metrics_app) = futures::future::join(
        axum::Server::bind(&addr).serve(app.into_make_service()),
        start_metrics_server(),
    )
    .await;
}
