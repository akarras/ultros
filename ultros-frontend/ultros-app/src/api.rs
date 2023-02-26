use std::collections::HashMap;

use futures::future::join_all;
use itertools::Itertools;
use leptos::Serializable;
use leptos::*;
use ultros_api_types::{
    cheapest_listings::CheapestListings,
    list::{CreateList, List, ListItem},
    recent_sales::RecentSales,
    user::{OwnedRetainer, UserData, UserRetainerListings, UserRetainers},
    world::WorldData,
    ActiveListing, CurrentlyShownItem, FfxivCharacter, FfxivCharacterVerification, Retainer,
};

use crate::error::{AppError, AppResult, SystemError};

pub(crate) async fn get_listings(
    cx: Scope,
    item_id: i32,
    world: &str,
) -> AppResult<CurrentlyShownItem> {
    if item_id == 0 {
        return Err(AppError::NoItem);
    }
    fetch_api(cx, &format!("/api/v1/listings/{world}/{item_id}")).await
}

pub(crate) async fn get_bulk_listings(
    cx: Scope,
    world: &str,
    item_ids: impl Iterator<Item = i32>,
) -> AppResult<HashMap<i32, Vec<(ActiveListing, Option<Retainer>)>>> {
    if world.is_empty() {
        return Err(AppError::NoItem);
    }
    let ids = item_ids.format(",");
    fetch_api(cx, &format!("/api/v1/bulkListings/{world}/{ids}")).await
}

pub(crate) async fn get_worlds(cx: Scope) -> AppResult<WorldData> {
    fetch_api(cx, "/api/v1/world_data").await
}

/// This is okay because the client will send our login cookie
pub(crate) async fn get_login(cx: Scope) -> AppResult<UserData> {
    fetch_api(cx, "/api/v1/current_user").await
}

/// Get analyzer data
pub(crate) async fn get_cheapest_listings(
    cx: Scope,
    world_name: &str,
) -> AppResult<CheapestListings> {
    fetch_api(cx, &format!("/api/v1/cheapest/{}", world_name)).await
}

/// Get most expensive
pub(crate) async fn get_recent_sales_for_world(
    cx: Scope,
    region_name: &str,
) -> AppResult<RecentSales> {
    fetch_api(cx, &format!("/api/v1/recentSales/{}", region_name)).await
}

/// Returns a list of the logged in user's retainers
pub(crate) async fn get_retainers(cx: Scope) -> AppResult<UserRetainers> {
    fetch_api(cx, "/api/v1/user/retainer").await
}

pub(crate) async fn get_retainer_listings(cx: Scope) -> AppResult<UserRetainerListings> {
    fetch_api(cx, "/api/v1/user/retainer/listings").await
}

pub(crate) async fn get_retainer_undercuts(cx: Scope) -> AppResult<UserRetainerListings> {
    // get our retainer data
    let mut retainer_data = get_retainer_listings(cx).await?;
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
        get_bulk_listings(cx, &world.to_string(), items.into_iter())
            .await
            // include the world id in the returned value
            .map(|listings| (world, listings))
    }))
    .await;
    // flatten the listings down so it's more usable
    let listings_map = listings.into_iter().flatten().fold(
        HashMap::new(),
        |mut world_map, (world_id, item_data)| {
            if let Some(_) = world_map.insert(world_id, item_data) {
                unreachable!("Should only be one world id from the set above.");
            }
            world_map
        },
    );
    // Now remove every listing from the user retainer listings that is already the cheapest listing per world
    for (_, retainers) in &mut retainer_data.retainers {
        for (_retainer, listings) in retainers {
            let mut new_listings = vec![];
            for listing in listings.iter() {
                // use the world/item_id as keys to lookup the rest of the listings that match this retainer
                if let Some(cheapest) = listings_map
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
                {
                    if listing.price_per_unit > cheapest {
                        new_listings.push(listing.clone());
                    }
                } else {
                    return Err(AppError::NoRetainerItems); // in theory this shouldn't happen, but mark as false to leave it in the set?
                };
            }
            *listings = new_listings;
        }
    }

    Ok(retainer_data)
}

/// Searches retainers based on their name
pub(crate) async fn search_retainers(cx: Scope, name: String) -> AppResult<Vec<Retainer>> {
    if name.is_empty() {
        return Err(AppError::EmptyString);
    }
    fetch_api(cx, &format!("/api/v1/retainer/search/{name}")).await
}

/// Claims the given retainer based on their id
pub(crate) async fn claim_retainer(cx: Scope, retainer_id: i32) -> AppResult<()> {
    fetch_api(cx, &format!("/api/v1/retainer/claim/{retainer_id}")).await
}

/// Unclaims the retainer based on the owned retainer id
pub(crate) async fn unclaim_retainer(cx: Scope, owned_retainer_id: i32) -> AppResult<()> {
    fetch_api(cx, &format!("/api/v1/retainer/unclaim/{owned_retainer_id}")).await
}

/// Gets the characters for this user
pub(crate) async fn get_characters(cx: Scope) -> AppResult<Vec<FfxivCharacter>> {
    fetch_api(cx, &format!("/api/v1/characters")).await
}

/// Gets pending character verifications for this user
pub(crate) async fn get_character_verifications(
    cx: Scope,
) -> AppResult<Vec<FfxivCharacterVerification>> {
    fetch_api(cx, &format!("/api/v1/characters/verifications")).await
}

pub(crate) async fn check_character_verification(cx: Scope, character_id: i32) -> AppResult<bool> {
    fetch_api(cx, &format!("/api/v1/characters/verify/{character_id}")).await
}

/// Starts to claim the given character
pub(crate) async fn claim_character(cx: Scope, id: i32) -> AppResult<(i32, String)> {
    fetch_api(cx, &format!("/api/v1/characters/claim/{id}")).await
}

pub(crate) async fn unclaim_character(cx: Scope, id: i32) -> AppResult<(i32, String)> {
    fetch_api(cx, &format!("/api/v1/characters/unclaim/{id}")).await
}

/// Searches for the given character with the given lodestone ID.
pub(crate) async fn search_characters(
    cx: Scope,
    character: String,
) -> AppResult<Vec<FfxivCharacter>> {
    fetch_api(cx, &format!("/api/v1/characters/search/{character}")).await
}

pub(crate) async fn get_lists(cx: Scope) -> AppResult<Vec<List>> {
    fetch_api(cx, &format!("/api/v1/list")).await
}

pub(crate) async fn get_list_items_with_listings(
    cx: Scope,
    list_id: i32,
) -> AppResult<(List, Vec<(ListItem, Vec<ActiveListing>)>)> {
    if list_id == 0 {
        return Err(AppError::BadList);
    }
    fetch_api(cx, &format!("/api/v1/list/{list_id}/listings")).await
}

pub(crate) async fn delete_list(cx: Scope, list_id: i32) -> AppResult<()> {
    fetch_api(cx, &format!("/api/v1/list/{list_id}/delete")).await
}

pub(crate) async fn create_list(cx: Scope, list: CreateList) -> AppResult<()> {
    post_api(cx, &format!("/api/v1/list/create"), list).await
}

pub(crate) async fn edit_list(cx: Scope, list: List) -> AppResult<()> {
    post_api(cx, &format!("/api/v1/list/edit"), list).await
}

pub(crate) async fn add_item_to_list(
    cx: Scope,
    list_id: i32,
    list_item: ListItem,
) -> AppResult<()> {
    post_api(cx, &format!("/api/v1/list/{list_id}/add/item"), list_item).await
}

pub(crate) async fn delete_list_item(cx: Scope, list_id: i32) -> AppResult<()> {
    fetch_api(cx, &format!("/api/v1/list/item/{list_id}/delete")).await
}

pub(crate) async fn update_retainer_order(
    cx: Scope,
    retainers: Vec<(OwnedRetainer, Retainer)>,
) -> AppResult<()> {
    post_api(cx, &format!("/api/v1/retainer/reorder"), retainers).await
}

/// Return the T, or try and return an AppError
fn deserialize<T>(json: &str) -> AppResult<T>
where
    T: Serializable,
{
    let data = T::from_json(json);
    match data {
        Ok(d) => return Ok(d),
        // try to deserialize as SystemError, if that fails then return this error
        Err(e) => {
            if let Ok(d) = SystemError::from_json(json) {
                return Err(d.into());
            } else {
                return Err(e.into());
            }
        }
    }
}

#[cfg(not(feature = "ssr"))]
pub(crate) async fn fetch_api<T>(cx: Scope, path: &str) -> AppResult<T>
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
            log!("{e}");
            e
        })?
        .text()
        .await?;

    // abort in-flight requests if the Scope is disposed
    // i.e., if we've navigated away from this page
    on_cleanup(cx, move || {
        if let Some(abort_controller) = abort_controller {
            abort_controller.abort()
        }
    });
    deserialize(&json)
}

#[cfg(feature = "ssr")]
pub(crate) async fn fetch_api<T>(cx: Scope, path: &str) -> AppResult<T>
where
    T: Serializable,
{
    // use the original headers of the scope
    // add the hostname when using the ssr path.
    let req_parts = use_context::<leptos_axum::RequestParts>(cx).ok_or(AppError::ParamMissing)?;
    let mut headers = req_parts.headers;
    let hostname = "http://localhost:8080";
    let path = format!("{hostname}{path}");
    headers.remove("Accept-Encoding");
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()?;
    let request = client.get(&path).build()?;
    let json = client
        .execute(request)
        .await
        .map_err(|e| {
            log::error!("Response {e}. {path}");
            e
        })?
        .text()
        .await?;
    deserialize(&json)
}

#[cfg(not(feature = "ssr"))]
pub(crate) async fn post_api<Y, T>(cx: Scope, path: &str, json: Y) -> AppResult<T>
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
            log!("{e}");
            e
        })?
        .text()
        .await?;

    // abort in-flight requests if the Scope is disposed
    // i.e., if we've navigated away from this page
    on_cleanup(cx, move || {
        if let Some(abort_controller) = abort_controller {
            abort_controller.abort()
        }
    });
    deserialize(&json)
}

#[cfg(feature = "ssr")]
pub(crate) async fn post_api<Y, T>(_cx: Scope, _path: &str, _json: Y) -> AppResult<T>
where
    Y: Serializable,
    T: Serializable,
{
    // This really only will be called by clients- I think.
    unreachable!("post_api should only be called on clients? I think...")
}
