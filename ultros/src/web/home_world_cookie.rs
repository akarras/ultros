use super::error::WebError;
use async_trait::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum_extra::extract::CookieJar;

#[derive(Clone, Copy)]
pub(crate) struct HomeWorld {
    pub(crate) home_world: i32,
}

pub(crate) const HOME_WORLD_COOKIE: &str = "HOME_WORLD";

#[async_trait]
impl<S> FromRequestParts<S> for HomeWorld
where
    S: Send + Sync,
    axum_extra::extract::cookie::Key: FromRef<S>,
{
    type Rejection = WebError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let cookie_jar: CookieJar = CookieJar::from_request_parts(parts, state).await.unwrap();
        let cookie = cookie_jar
            .get(HOME_WORLD_COOKIE)
            .ok_or(WebError::HomeWorldNotSet)?;
        let home_world = cookie.value().parse::<i32>()?;
        Ok(Self { home_world })
    }
}
