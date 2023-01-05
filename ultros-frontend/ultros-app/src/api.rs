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
#[cfg(not(feature = "ssr"))]
pub(crate) async fn get_login(cx: Scope) -> Option<UserData> {
    leptos::log!("login get");
    fetch_api(cx, "/api/v1/current_user").await
}

/// On the server we need to use the cookie that the client already requested
#[cfg(feature = "ssr")]
pub(crate) async fn get_login(cx: Scope) -> Option<UserData> {
    use std::env;
    // get the cookie from axum for discord user logins
    use axum_extra::extract::{cookie::Key, PrivateCookieJar};
    use leptos::tracing::log::warn;
    use serenity::http::Http;
    use ultros_db::UltrosDb;
    let key = env::var("KEY").expect("environment variable KEY not found");
    let req_parts = use_context::<leptos_axum::RequestParts>(cx).unwrap();
    let cookie_jar = PrivateCookieJar::from_headers(&req_parts.headers, Key::from(key.as_bytes()));

    let discord_auth = cookie_jar.get("discord_auth")?;
    let db = use_context::<UltrosDb>(cx).expect("Must have db on server");
    let http = Http::new(&format!("Bearer {}", discord_auth.value()));
    let user = http
        .get_current_user()
        .await
        .map_err(|e| {
            error!("error accessing logged in user {e}");
        })
        .ok()?;
    let avatar_url = user
        .static_avatar_url()
        .unwrap_or_else(|| user.default_avatar_url());
    let user = UserData {
        id: user.id.0,
        username: user.name,
        avatar: avatar_url,
    };
    match db
        .get_or_create_discord_user(user.id, user.username.clone())
        .await
    {
        Ok(_) => {}
        Err(e) => {
            error!("{e:?}");
            return None;
        }
    }
    Some(user)
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
pub async fn fetch_api<T>(_cx: Scope, path: &str) -> Option<T>
where
    T: Serializable,
{
    use leptos::tracing::log;
    // add the hostname when using the ssr path.
    let hostname = "http://localhost:8080";
    let path = format!("{hostname}{path}");
    let json = reqwest::get(path)
        .await
        .map_err(|e| log::error!("{e}"))
        .ok()?
        .text()
        .await
        .ok()?;
    T::from_json(&json).map_err(|e| log::error!("{e}")).ok()
}
