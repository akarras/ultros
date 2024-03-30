use futures::future::join_all;
use itertools::Itertools;
use leptos::*;
use log::error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::instrument;
use ultros_api_types::{
    cheapest_listings::CheapestListings,
    list::{CreateList, List, ListItem},
    recent_sales::RecentSales,
    result::JsonError,
    retainer::{Retainer, RetainerListings},
    user::{OwnedRetainer, UserData, UserRetainerListings, UserRetainers},
    world::WorldData,
    ActiveListing, CurrentlyShownItem, FfxivCharacter, FfxivCharacterVerification,
};

use crate::error::{AppError, AppResult, SystemError};

pub(crate) async fn get_listings(item_id: i32, world: &str) -> AppResult<CurrentlyShownItem> {
    if item_id == 0 {
        return Err(AppError::NoItem);
    }
    fetch_api(&format!("/api/v1/listings/{world}/{item_id}")).await
}

pub(crate) async fn get_bulk_listings(
    world: &str,
    item_ids: impl Iterator<Item = i32>,
) -> AppResult<HashMap<i32, Vec<(ActiveListing, Option<Retainer>)>>> {
    if world.is_empty() {
        return Err(AppError::NoItem);
    }
    let ids = item_ids.format(",");
    fetch_api(&format!("/api/v1/bulkListings/{world}/{ids}")).await
}

#[instrument]
pub(crate) async fn get_worlds() -> AppResult<WorldData> {
    fetch_api("/api/v1/world_data").await
}

/// This is okay because the client will send our login cookie
pub(crate) async fn get_login() -> AppResult<UserData> {
    fetch_api("/api/v1/current_user").await
}

pub(crate) async fn delete_user() -> AppResult<()> {
    delete_api("/api/v1/current_user").await
}

/// Get analyzer data
pub(crate) async fn get_cheapest_listings(world_name: &str) -> AppResult<CheapestListings> {
    fetch_api(&format!("/api/v1/cheapest/{}", world_name)).await
}

/// Get most expensive
pub(crate) async fn get_recent_sales_for_world(region_name: &str) -> AppResult<RecentSales> {
    fetch_api(&format!("/api/v1/recentSales/{}", region_name)).await
}

/// Returns a list of the logged in user's retainers
pub(crate) async fn get_retainers() -> AppResult<UserRetainers> {
    fetch_api("/api/v1/user/retainer").await
}

pub(crate) async fn get_retainer_listings(retainer_id: i32) -> AppResult<RetainerListings> {
    fetch_api(&format!("/api/v1/retainer/listings/{retainer_id}")).await
}

pub(crate) async fn get_user_retainer_listings() -> AppResult<UserRetainerListings> {
    fetch_api("/api/v1/user/retainer/listings").await
}

#[derive(Deserialize, Serialize, Clone)]
pub(crate) struct UndercutData {
    pub(crate) current: ActiveListing,
    pub(crate) cheapest: i32,
}

pub type Undercuts = Vec<(Option<FfxivCharacter>, Vec<(Retainer, Vec<UndercutData>)>)>;

pub(crate) async fn get_retainer_undercuts() -> AppResult<Undercuts> {
    // get our retainer data
    let retainer_data = get_user_retainer_listings().await?;
    // build a unique list of worlds and item ids so we can fetch additional info about them
    // todo: couldn't I just use cheapest listings for each world & avoid looking up literally every retainer?
    let world_items: HashMap<i32, Vec<i32>> = retainer_data
        .retainers
        .iter()
        .flat_map(|(_, r)| {
            r.iter()
                .flat_map(|(_, l)| l.iter().map(|l| (l.world_id, l.item_id)))
        })
        .fold(HashMap::new(), |mut acc, (world, item_id)| {
            acc.entry(world).or_default().push(item_id);
            acc
        });
    // todo: once the api calls use a result type, swap this to a try_join all
    let listings = join_all(world_items.into_iter().map(|(world, items)| async move {
        get_bulk_listings(&world.to_string(), items.into_iter())
            .await
            // include the world id in the returned value
            .map(|listings| (world, listings))
    }))
    .await;
    // flatten the listings down so it's more usable
    let listings_map = listings.into_iter().flatten().fold(
        HashMap::new(),
        |mut world_map, (world_id, item_data)| {
            if world_map.insert(world_id, item_data).is_some() {
                unreachable!("Should only be one world id from the set above.");
            }
            world_map
        },
    );
    // Now remove every listing from the user retainer listings that is already the cheapest listing per world
    let retainer_data = retainer_data
        .retainers
        .into_iter()
        .map(|(c, retainers)| {
            (
                c,
                retainers
                    .into_iter()
                    .map(|(r, listings)| {
                        let new_listings = listings
                            .iter()
                            .filter_map(|listing| {
                                // use the world/item_id as keys to lookup the rest of the listings that match this retainer
                                listings_map
                                    .get(&listing.world_id)
                                    .and_then(|world_map| world_map.get(&listing.item_id))
                                    .and_then(|listings| {
                                        listings
                                            .iter()
                                            .filter(|(cheapest, _)| {
                                                if listing.hq {
                                                    listing.hq == cheapest.hq
                                                } else {
                                                    true
                                                }
                                            })
                                            .map(|(l, _)| l.price_per_unit)
                                            .min()
                                    })
                                    .and_then(|cheapest| {
                                        (listing.price_per_unit > cheapest).then(|| UndercutData {
                                            current: listing.clone(),
                                            cheapest,
                                        })
                                    })
                            })
                            .collect();
                        (r, new_listings)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();

    Ok(retainer_data)
}

/// Searches retainers based on their name
pub(crate) async fn search_retainers(name: String) -> AppResult<Vec<Retainer>> {
    if name.is_empty() {
        return Err(AppError::EmptyString);
    }
    fetch_api(&format!("/api/v1/retainer/search/{name}")).await
}

/// Claims the given retainer based on their id
pub(crate) async fn claim_retainer(retainer_id: i32) -> AppResult<()> {
    fetch_api(&format!("/api/v1/retainer/claim/{retainer_id}")).await
}

/// Unclaims the retainer based on the owned retainer id
pub(crate) async fn unclaim_retainer(owned_retainer_id: i32) -> AppResult<()> {
    fetch_api(&format!("/api/v1/retainer/unclaim/{owned_retainer_id}")).await
}

/// Gets the characters for this user
pub(crate) async fn get_characters() -> AppResult<Vec<FfxivCharacter>> {
    fetch_api(&format!("/api/v1/characters")).await
}

/// Gets pending character verifications for this user
pub(crate) async fn get_character_verifications() -> AppResult<Vec<FfxivCharacterVerification>> {
    fetch_api(&format!("/api/v1/characters/verifications")).await
}

pub(crate) async fn check_character_verification(character_id: i32) -> AppResult<bool> {
    fetch_api(&format!("/api/v1/characters/verify/{character_id}")).await
}

/// Starts to claim the given character
pub(crate) async fn claim_character(id: i32) -> AppResult<(i32, String)> {
    fetch_api(&format!("/api/v1/characters/claim/{id}")).await
}

pub(crate) async fn unclaim_character(id: i32) -> AppResult<(i32, String)> {
    fetch_api(&format!("/api/v1/characters/unclaim/{id}")).await
}

/// Searches for the given character with the given lodestone ID.
pub(crate) async fn search_characters(character: String) -> AppResult<Vec<FfxivCharacter>> {
    fetch_api(&format!("/api/v1/characters/search/{character}")).await
}

pub(crate) async fn get_lists() -> AppResult<Vec<List>> {
    fetch_api(&format!("/api/v1/list")).await
}

pub(crate) async fn get_list_items_with_listings(
    list_id: i32,
) -> AppResult<(List, Vec<(ListItem, Vec<ActiveListing>)>)> {
    if list_id == 0 {
        return Err(AppError::BadList);
    }
    fetch_api(&format!("/api/v1/list/{list_id}/listings")).await
}

pub(crate) async fn delete_list(list_id: i32) -> AppResult<()> {
    delete_api(&format!("/api/v1/list/{list_id}/delete")).await
}

pub(crate) async fn create_list(list: CreateList) -> AppResult<()> {
    post_api(&format!("/api/v1/list/create"), list).await
}

pub(crate) async fn edit_list(list: List) -> AppResult<()> {
    post_api(&format!("/api/v1/list/edit"), list).await
}

pub(crate) async fn bulk_add_item_to_list(
    list_id: i32,
    list_items: Vec<ListItem>,
) -> AppResult<()> {
    post_api(&format!("/api/v1/list/{list_id}/add/items"), list_items).await
}

pub(crate) async fn add_item_to_list(list_id: i32, list_item: ListItem) -> AppResult<()> {
    post_api(&format!("/api/v1/list/{list_id}/add/item"), list_item).await
}

pub(crate) async fn edit_list_item(list_item: ListItem) -> AppResult<()> {
    post_api(&format!("/api/v1/list/item/edit"), list_item).await
}

pub(crate) async fn delete_list_item(list_id: i32) -> AppResult<()> {
    delete_api(&format!("/api/v1/list/item/{list_id}/delete")).await
}

pub(crate) async fn update_retainer_order(retainers: Vec<OwnedRetainer>) -> AppResult<()> {
    post_api(&format!("/api/v1/retainer/reorder"), retainers).await
}

/// Return the T, or try and return an AppError
#[instrument]
fn deserialize<T>(json: &str) -> AppResult<T>
where
    T: Serializable,
{
    let data = T::de(json);
    match data {
        Ok(d) => return Ok(d),
        // try to deserialize as SystemError, if that fails then return this error
        Err(e) => {
            if let Ok(d) = JsonError::de(json) {
                match d {
                    JsonError::ApiError(api) => {
                        return Err(api.into());
                    }
                }
            } else if let Ok(d) = SystemError::de(json) {
                return Err(d.into());
            } else {
                return Err(e.into());
            }
        }
    }
}

#[cfg(not(feature = "ssr"))]
#[instrument(skip())]
pub(crate) async fn delete_api<T>(path: &str) -> AppResult<T>
where
    T: Serializable,
{
    let abort_controller = web_sys::AbortController::new().ok();
    let abort_signal = abort_controller.as_ref().map(|a| a.signal());
    let json: String = gloo_net::http::Request::delete(path)
        .abort_signal(abort_signal.as_ref())
        .send()
        .await
        .map_err(|e| {
            error!("{}", e);
            e
        })?
        .text()
        .await?;

    // abort in-flight requests if the Scope is disposed
    // i.e., if we've navigated away from this page
    on_cleanup(move || {
        if let Some(abort_controller) = abort_controller {
            abort_controller.abort()
        }
    });
    deserialize(&json)
}

#[cfg(feature = "ssr")]
#[instrument(skip())]
pub(crate) async fn delete_api<T>(path: &str) -> AppResult<T>
where
    T: Serializable,
{
    use axum::http::request::Parts;
    // use the original headers of the scope
    // add the hostname when using the ssr path.
    use tracing::Instrument;

    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::ClientBuilder::new()
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .build()
            .unwrap()
    });
    let req_parts = use_context::<Parts>().ok_or(AppError::ParamMissing)?;
    let headers = req_parts.headers;
    let hostname = "http://localhost:8080";
    let path = format!("{hostname}{path}");
    // headers.remove("Accept-Encoding");
    // this is only necessary because reqwest isn't updated to http 1.0- and I'm being lazy
    let mut new_map = reqwest::header::HeaderMap::new();
    for (name, value) in headers.into_iter().filter_map(|(name, value)| {
        Some((
            reqwest::header::HeaderName::from_lowercase(name?.as_str().as_bytes()).ok()?,
            reqwest::header::HeaderValue::from_bytes(value.as_bytes()).ok()?,
        ))
    }) {
        new_map.insert(name, value);
    }
    let request = client.delete(&path).headers(new_map).build()?;
    let json = client
        .execute(request)
        .await
        .instrument(tracing::trace_span!("HTTP FETCH"))
        .into_inner()
        .map_err(|e| {
            error!("Response {e}. {path}");
            e
        })?
        .text()
        .await?;
    deserialize(&json)
}

#[cfg(not(feature = "ssr"))]
#[instrument(skip())]
pub(crate) async fn fetch_api<T>(path: &str) -> AppResult<T>
where
    T: Serializable,
{
    let abort_controller = web_sys::AbortController::new().ok();
    let abort_signal = abort_controller.as_ref().map(|a| a.signal());
    let json: String = gloo_net::http::Request::get(path)
        .abort_signal(abort_signal.as_ref())
        .send()
        .await
        .map_err(|e| {
            error!("{}", e);
            e
        })?
        .text()
        .await?;

    // abort in-flight requests if the Scope is disposed
    // i.e., if we've navigated away from this page
    on_cleanup(move || {
        if let Some(abort_controller) = abort_controller {
            abort_controller.abort()
        }
    });
    deserialize(&json)
}

#[cfg(feature = "ssr")]
#[instrument(skip())]
pub(crate) async fn fetch_api<T>(path: &str) -> AppResult<T>
where
    T: Serializable,
{
    // use the original headers of the scope
    // add the hostname when using the ssr path.
    use axum::http::request::Parts;
    use tracing::Instrument;

    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::ClientBuilder::new()
            .tcp_keepalive(std::time::Duration::from_secs(60))
            .build()
            .unwrap()
    });
    let req_parts = use_context::<Parts>().ok_or(AppError::ParamMissing)?;
    let headers = req_parts.headers;
    let hostname = "http://localhost:8080";
    let path = format!("{hostname}{path}");
    // this is only necessary because reqwest isn't updated to http 1.0- and I'm being lazy
    let mut new_map = reqwest::header::HeaderMap::new();
    for (name, value) in headers.into_iter().filter_map(|(name, value)| {
        Some((
            reqwest::header::HeaderName::from_lowercase(name?.as_str().as_bytes()).ok()?,
            reqwest::header::HeaderValue::from_bytes(value.as_bytes()).ok()?,
        ))
    }) {
        new_map.insert(name, value);
    }
    let request = client.get(&path).headers(new_map).build()?;
    let json = client
        .execute(request)
        .await
        .instrument(tracing::trace_span!("HTTP FETCH"))
        .into_inner()
        .map_err(|e| {
            error!("Response {e}. {path}");
            e
        })?
        .text()
        .await?;
    deserialize(&json)
}

#[cfg(not(feature = "ssr"))]
#[instrument(skip(json))]
pub(crate) async fn post_api<Y, T>(path: &str, json: Y) -> AppResult<T>
where
    Y: serde::Serialize,
    T: Serializable,
{
    let abort_controller = web_sys::AbortController::new().ok();
    let abort_signal = abort_controller.as_ref().map(|a| a.signal());

    let json: String = gloo_net::http::Request::post(path)
        .abort_signal(abort_signal.as_ref())
        .json(&json)?
        .send()
        .await
        .map_err(|e| {
            error!("{e}");
            e
        })?
        .text()
        .await?;

    // abort in-flight requests if the Scope is disposed
    // i.e., if we've navigated away from this page
    on_cleanup(move || {
        if let Some(abort_controller) = abort_controller {
            abort_controller.abort()
        }
    });
    deserialize(&json)
}

#[cfg(feature = "ssr")]
#[instrument(skip(_json))]
pub(crate) async fn post_api<Y, T>(_path: &str, _json: Y) -> AppResult<T>
where
    Y: Serializable,
    T: Serializable,
{
    // This really only will be called by clients- I think.
    unreachable!("post_api should only be called on clients? I think...")
}
