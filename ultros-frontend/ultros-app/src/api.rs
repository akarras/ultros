use leptos::*;
use ultros_api_types::user_data::UserData;
use ultros_api_types::{world::WorldData, CurrentlyShownItem};

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
    leptos::log!("login get");
    fetch_api(cx, "/api/v1/current_user").await
}

#[cfg(not(feature = "ssr"))]
pub async fn fetch_api<T>(cx: Scope, path: &str) -> Option<T>
where
    T: Serializable,
{
    use leptos::{log, on_cleanup};

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
    T::from_json(&json).ok()
}

#[cfg(feature = "ssr")]
pub async fn fetch_api<T>(cx: Scope, path: &str) -> Option<T>
where
    T: Serializable,
{
    use leptos::tracing::log;
    use reqwest::header::HeaderMap;
    // use the original headers of the scope 
    // add the hostname when using the ssr path.
    let req_parts = use_context::<leptos_axum::RequestParts>(cx).unwrap();
    let mut headers = req_parts.headers;
    let hostname = "http://localhost:8080";
    let path = format!("{hostname}{path}");
    headers.remove("Accept-Encoding");
    let client = reqwest::Client::builder().default_headers(headers).build().ok()?;
    let request = client.get(&path).build().ok()?;
    let json = client.execute(request)
        .await
        .map_err(|e| log::error!("Response {e}. {path}"))
        .ok()?
        .text()
        .await
        .ok()?;
    T::from_json(&json).map_err(|e| log::error!("{e} {path} returned: json text {json}")).ok()
}
