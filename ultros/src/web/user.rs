use axum::Json;
use axum::extract::State;
use axum::response::Redirect;
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use ultros_api_types::user::UserData;
use ultros_db::UltrosDb;

use crate::web::error::ApiError;
use crate::web::oauth::{AuthDiscordUser, AuthUserCache};

pub(crate) async fn current_user(user: AuthDiscordUser) -> Json<UserData> {
    Json(UserData {
        id: user.id,
        username: user.name,
        avatar: user.avatar_url,
    })
}

pub(crate) async fn delete_user(
    user: AuthDiscordUser,
    State(cache): State<AuthUserCache>,
    State(db): State<UltrosDb>,
    cookie_jar: CookieJar,
) -> Result<(CookieJar, Redirect), ApiError> {
    let id = user.id;
    db.delete_discord_user(id as i64).await?;
    let token = cookie_jar
        .get("discord_auth")
        .ok_or(anyhow::anyhow!("Failed to get icon"))?
        .value()
        .to_owned();
    cache.remove_token(&token).await;
    let cookie_jar = cookie_jar.remove(Cookie::from("discord_auth"));
    // remove the token from the cache
    // remove the auth cookie from the cache
    Ok((cookie_jar, Redirect::to("/")))
}
