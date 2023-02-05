use std::collections::HashMap;

use futures::future::join_all;
use itertools::Itertools;
use leptos::*;
use ultros_api_types::{
    cheapest_listings::CheapestListings,
    list::{CreateList, List, ListItem},
    recent_sales::RecentSales,
    user::{UserData, UserRetainerListings, UserRetainers},
    world::WorldData,
    ActiveListing, CurrentlyShownItem, Retainer,
};

#[cfg(not(feature = "ssr"))]
use serde::Serialize;

pub(crate) async fn get_listings(
    cx: Scope,
    item_id: i32,
    world: &str,
) -> Option<CurrentlyShownItem> {
    fetch_api(cx, &format!("/api/v1/listings/{world}/{item_id}")).await
}

pub(crate) async fn get_bulk_listings(
    cx: Scope,
    world: &str,
    item_ids: impl Iterator<Item = i32>,
) -> Option<HashMap<i32, Vec<(ActiveListing, Option<Retainer>)>>> {
    let ids = item_ids.format(",");
    fetch_api(cx, &format!("/api/v1/bulkListings/{world}/{ids}")).await
}

pub(crate) async fn get_worlds(cx: Scope) -> Option<WorldData> {
    fetch_api(cx, "/api/v1/world_data").await
}

/// This is okay because the client will send our login cookie
pub(crate) async fn get_login(cx: Scope) -> Option<UserData> {
    fetch_api(cx, "/api/v1/current_user").await
}

/// Get analyzer data
pub(crate) async fn get_cheapest_listings(cx: Scope, world_name: &str) -> Option<CheapestListings> {
    fetch_api(cx, &format!("/api/v1/cheapest/{}", world_name)).await
}

/// Get most expensive
pub(crate) async fn get_recent_sales_for_world(
    cx: Scope,
    region_name: &str,
) -> Option<RecentSales> {
    fetch_api(cx, &format!("/api/v1/recentSales/{}", region_name)).await
}

/// Returns a list of the logged in user's retainers
pub(crate) async fn get_retainers(cx: Scope) -> Option<UserRetainers> {
    fetch_api(cx, "/api/v1/user/retainer").await
}

pub(crate) async fn get_retainer_listings(cx: Scope) -> Option<UserRetainerListings> {
    fetch_api(cx, "/api/v1/user/retainer/listings").await
}

pub(crate) async fn get_retainer_undercuts(cx: Scope) -> Option<UserRetainerListings> {
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
                if let Some((cheapest, _)) = listings_map
                    .get(&listing.world_id)
                    .and_then(|world_map| world_map.get(&listing.item_id))
                    .and_then(|listings| {
                        listings.iter().find(|(cheapest, _)| {
                            if listing.hq {
                                listing.hq == cheapest.hq
                            } else {
                                true
                            }
                        })
                    })
                {
                    if listing.price_per_unit > cheapest.price_per_unit {
                        new_listings.push(listing.clone());
                    }
                } else {
                    return None; // in theory this shouldn't happen, but mark as false to leave it in the set?
                };
            }
            *listings = new_listings;
        }
    }

    Some(retainer_data)
}

/// Searches retainers based on their name
pub(crate) async fn search_retainers(cx: Scope, name: String) -> Option<Vec<Retainer>> {
    if name.is_empty() {
        return None;
    }
    fetch_api(cx, &format!("/api/v1/retainer/search/{name}")).await
}

/// Claims the given retainer based on their id
pub(crate) async fn claim_retainer(cx: Scope, retainer_id: i32) -> Option<()> {
    fetch_api(cx, &format!("/api/v1/retainer/claim/{retainer_id}")).await
}

/// Unclaims the retainer based on the owned retainer id
pub(crate) async fn unclaim_retainer(cx: Scope, owned_retainer_id: i32) -> Option<()> {
    fetch_api(cx, &format!("/api/v1/retainer/unclaim/{owned_retainer_id}")).await
}

pub(crate) async fn get_lists(cx: Scope) -> Option<Vec<List>> {
    fetch_api(cx, &format!("/api/v1/list")).await
}

pub(crate) async fn get_list_items_with_listings(
    cx: Scope,
    list_id: i32,
) -> Option<(List, Vec<(ListItem, Vec<ActiveListing>)>)> {
    fetch_api(cx, &format!("/api/v1/list/{list_id}/listings")).await
}

pub(crate) async fn delete_list(cx: Scope, list_id: i32) -> Option<()> {
    fetch_api(cx, &format!("/api/v1/list/{list_id}/delete")).await
}

pub(crate) async fn create_list(cx: Scope, list: CreateList) -> Option<()> {
    post_api(cx, &format!("/api/v1/list/create"), list).await
}

pub(crate) async fn edit_list(cx: Scope, list: List) -> Option<()> {
    post_api(cx, &format!("/api/v1/list/edit"), list).await
}

pub(crate) async fn add_item_to_list(cx: Scope, list_id: i32, list_item: ListItem) -> Option<()> {
    post_api(cx, &format!("/api/v1/list/{list_id}/add/item"), list_item).await
}

pub(crate) async fn delete_list_item(cx: Scope, list_id: i32) -> Option<()> {
    fetch_api(cx, &format!("/api/v1/list/item/{list_id}/delete")).await
}

#[cfg(not(feature = "ssr"))]
pub async fn fetch_api<T>(cx: Scope, path: &str) -> Option<T>
where
    T: Serializable,
{
    let abort_controller = web_sys::AbortController::new().ok();
    let abort_signal = abort_controller.as_ref().map(|a| a.signal());

    let json: String = gloo_net::http::Request::get(path)
        .abort_signal(abort_signal.as_ref())
        .send()
        .await
        .map_err(|e| log!("{e}"))
        .ok()?
        .text()
        .await
        .ok()?;

    // abort in-flight requests if the Scope is disposed
    // i.e., if we've navigated away from this page
    on_cleanup(cx, move || {
        if let Some(abort_controller) = abort_controller {
            abort_controller.abort()
        }
    });
    T::from_json(&json)
        .map_err(|e| {
            gloo::console::error!(format!("Error receiving json error: {e:?}"));
            e
        })
        .ok()
}

#[cfg(feature = "ssr")]
pub async fn fetch_api<T>(cx: Scope, path: &str) -> Option<T>
where
    T: Serializable,
{
    use leptos::tracing::log;
    // use the original headers of the scope
    // add the hostname when using the ssr path.
    let req_parts = use_context::<leptos_axum::RequestParts>(cx)?;
    let mut headers = req_parts.headers;
    let hostname = "http://localhost:8080";
    let path = format!("{hostname}{path}");
    headers.remove("Accept-Encoding");
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .ok()?;
    let request = client.get(&path).build().ok()?;
    let json = client
        .execute(request)
        .await
        .map_err(|e| log::error!("Response {e}. {path}"))
        .ok()?
        .text()
        .await
        .ok()?;
    T::from_json(&json)
        .map_err(|e| log::error!("{e} {path} returned: json text {json}"))
        .ok()
}

#[cfg(not(feature = "ssr"))]
pub async fn post_api<Y, T>(cx: Scope, path: &str, json: Y) -> Option<T>
where
    Y: Serialize,
    T: Serializable,
{
    let abort_controller = web_sys::AbortController::new().ok();
    let abort_signal = abort_controller.as_ref().map(|a| a.signal());

    let json: String = gloo_net::http::Request::post(path)
        .abort_signal(abort_signal.as_ref())
        .json(&json)
        .ok()?
        .send()
        .await
        .map_err(|e| log!("{e}"))
        .ok()?
        .text()
        .await
        .ok()?;

    // abort in-flight requests if the Scope is disposed
    // i.e., if we've navigated away from this page
    on_cleanup(cx, move || {
        if let Some(abort_controller) = abort_controller {
            abort_controller.abort()
        }
    });
    T::from_json(&json)
        .map_err(|e| {
            gloo::console::error!(format!("Error receiving json error: {e:?}"));
            e
        })
        .ok()
}

#[cfg(feature = "ssr")]
pub async fn post_api<Y, T>(_cx: Scope, _path: &str, _json: Y) -> Option<T>
where
    Y: Serializable,
    T: Serializable,
{
    // This really only will be called by clients- I think.
    unreachable!("post_api should only be called on clients? I think...")
}
