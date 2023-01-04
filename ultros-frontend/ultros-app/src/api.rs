pub async fn get_listings(cx: Scope, item_id: i32, world: &str) -> Option<CurrentlyShownItem> {
    fetch_api(cx, &format!("/api/v1/listings/{world}/{item_id}")).await
}
use leptos::{Scope, Serializable};
use ultros_api_types::CurrentlyShownItem;

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
    use std::path::PathBuf;

    use leptos::tracing::log;
    // add the hostname when using the ssr path.
    let hostname = "http://localhost:8080";
    let path = format!("{hostname}{path}");
    log::warn!("fetching {path:?}");
    let json = reqwest::get(path)
        .await
        .map_err(|e| log::error!("{e}"))
        .ok()?
        .text()
        .await
        .ok()?;
    T::from_json(&json).map_err(|e| log::error!("{e}")).ok()
}
