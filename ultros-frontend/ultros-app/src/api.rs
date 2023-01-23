use leptos::*;
use ultros_api_types::{
    cheapest_listings::CheapestListings,
    recent_sales::RecentSales,
    user::{UserData, UserRetainerListings, UserRetainers},
    world::WorldData,
    world_helper::AnyResult,
    CurrentlyShownItem, Retainer,
};

pub(crate) async fn get_listings(
    cx: Scope,
    item_id: i32,
    world: &str,
) -> Option<CurrentlyShownItem> {
    fetch_api(cx, &format!("/api/v1/listings/{world}/{item_id}")).await
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

/// Searches retainers based on their name
pub(crate) async fn search_retainers(cx: Scope, name: String) -> Option<Vec<Retainer>> {
    if name.is_empty() {
        return None;
    }
    fetch_api(cx, &format!("/api/v1/retainer/search/{name}")).await
}

/// Claims the given retainer based on their name
pub(crate) async fn claim_retainer(cx: Scope, retainer_id: i32) -> Option<()> {
    fetch_api(cx, &format!("/api/v1/retainer/claim/{retainer_id}")).await
}

pub(crate) async fn unclaim_retainer(cx: Scope, owned_retainer_id: i32) -> Option<()> {
    fetch_api(cx, &format!("/api/v1/retainer/unclaim/{owned_retainer_id}")).await
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
