use std::{num::ParseIntError, sync::Arc};

use axum::{
    Json,
    response::{IntoResponse, Response},
};
use axum_extra::extract::{
    PrivateCookieJar,
    cookie::{Cookie, Key},
};
use hyper::StatusCode;
use oauth2::{
    ConfigurationError, RequestTokenError, RevocationErrorResponseType, StandardErrorResponse,
    basic::BasicErrorResponseType,
};
use sitemap_rs::{sitemap_index_error::SitemapIndexError, url_set_error::UrlSetError};
use thiserror::Error;
use tokio::{sync::broadcast::error::SendError, time::error::Elapsed};
use tracing::{error, info};
use ultros_api_types::result::JsonErrorWrapper;
use ultros_db::{
    SeaDbErr, common_type_conversions::ApiConversionError, lists::ListError,
    retainers::RetainerError, world_data::world_cache::WorldCacheError,
};

use crate::{analyzer_service::AnalyzerError, event};

use crate::character_claim::ClaimError;

/// Generates an `Error`-deriving enum with the variants shared between `ApiError` and `WebError`.
/// The shared variants and their `#[from]` / `#[error]` attributes are kept in one place so the
/// two enums can't drift. Caller passes in any enum-specific variants between braces.
macro_rules! define_error_enum {
    ($name:ident { $($extra:tt)* }) => {
        #[derive(Debug, Error)]
        pub enum $name {
            #[error("OAuth configuration error {0}")]
            ConfigurationError(#[from] ConfigurationError),
            #[error("Error creating oauth token {0}")]
            RequestErrorToken(
                #[from]
                RequestTokenError<
                    oauth2::HttpClientError<oauth2::reqwest::Error>,
                    StandardErrorResponse<RevocationErrorResponseType>,
                >,
            ),
            #[error("Generic error {0}")]
            AnyhowError(#[from] anyhow::Error),
            #[error("Parse int failed {0}")]
            ParseIntError(#[from] ParseIntError),
            #[error("{0}")]
            WorldSelectError(#[from] WorldCacheError),
            #[error("Db Error {0}")]
            DbError(#[from] SeaDbErr),
            #[error("Error communicaing with universalis {0}")]
            UniversalisError(#[from] universalis::Error),
            #[error("Error sending listing update {0}")]
            ListingSendError(
                #[from] SendError<event::EventType<Arc<Vec<ultros_db::entity::active_listing::Model>>>>,
            ),
            #[error("Error making an internal HTTP request {0}")]
            ReqwestError(#[from] reqwest::Error),
            #[error("Internal HTTP Error {0}")]
            AxumError(#[from] axum::http::Error),
            #[error("IO Error {0}")]
            StdError(#[from] std::io::Error),
            #[error("Error reading lodestone server name {0}")]
            LodestoneServerParse(#[from] lodestone::model::server::ServerParseError),
            #[error("Lodestone error {0}")]
            LodestoneError(#[from] lodestone::LodestoneError),
            // this is kind of bad if I ever use the elapsed error for something else but I'll pretend
            #[error("Universalis is being slow. {0}. Will continue waiting")]
            TimeoutElapsed(#[from] Elapsed),
            #[error("Analyzer Error: {0}")]
            AnalyzerError(#[from] AnalyzerError),
            #[error("Character claim error {0}")]
            CharacterClaimError(#[from] ClaimError),
            #[error("Error generating sitemap {0}")]
            SiteMapError(#[from] SitemapIndexError),
            #[error("Error generating url set {0}")]
            UrlSetError(#[from] UrlSetError),
            #[error("Token error {0}")]
            TokenError(
                #[from]
                RequestTokenError<
                    oauth2::HttpClientError<oauth2::reqwest::Error>,
                    StandardErrorResponse<BasicErrorResponseType>,
                >,
            ),
            $($extra)*
        }
    };
}

define_error_enum!(ApiError {
    #[error("API conversions error {0}")]
    ApiConversionError(#[from] ApiConversionError),
    #[error("No Auth Cookie")]
    NoAuthCookie,
    #[error("Discord token was invalid")]
    DiscordTokenInvalid(PrivateCookieJar<Key>),
    #[error("{0}")]
    Forbidden(&'static str),
});

impl ApiError {
    fn as_status_code(&self) -> StatusCode {
        match self {
            // Auth failures are 401 — the same status `WebError::NotAuthenticated`
            // already uses for page routes. This used to answer `200` to avoid
            // "a real error", but a 401 achieves that intent without lying about
            // the status: it's a *client* error, so it never trips the
            // `is_server_error()` branches below that log at error level.
            //
            // Answering 200 made an auth failure indistinguishable from success
            // at the HTTP layer, so the SSR fetch helper took its
            // `status.is_success()` branch and reported the structured
            // `{"ApiError":"NotAuthenticated"}` body as a *deserialization*
            // failure at error level (the GlitchTip 2218/2210 lineage).
            ApiError::NoAuthCookie | ApiError::DiscordTokenInvalid(_) => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::AnyhowError(e) => match e.downcast_ref::<ListError>() {
                Some(ListError::Forbidden(_)) => StatusCode::FORBIDDEN,
                Some(ListError::NotFound | ListError::InviteNotFound) => StatusCode::NOT_FOUND,
                Some(ListError::BadRequest(_) | ListError::InviteExhausted) => {
                    StatusCode::BAD_REQUEST
                }
                None => StatusCode::INTERNAL_SERVER_ERROR,
            }
            .or_else_status(e.downcast_ref::<RetainerError>()),
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn as_api_error(&self) -> ultros_api_types::result::ApiError {
        match self {
            ApiError::NoAuthCookie => ultros_api_types::result::ApiError::NotAuthenticated,
            ApiError::Forbidden(_) => ultros_api_types::result::ApiError::Forbidden,
            ApiError::AnyhowError(e) => match e.downcast_ref::<ListError>() {
                Some(ListError::Forbidden(_)) => ultros_api_types::result::ApiError::Forbidden,
                Some(ListError::NotFound | ListError::InviteNotFound) => {
                    ultros_api_types::result::ApiError::NotFound
                }
                Some(ListError::BadRequest(msg)) => {
                    ultros_api_types::result::ApiError::BadRequest((*msg).into())
                }
                Some(ListError::InviteExhausted) => ultros_api_types::result::ApiError::BadRequest(
                    "Invite has reached max uses".into(),
                ),
                None => match e.downcast_ref::<RetainerError>() {
                    Some(RetainerError::Forbidden(_)) => {
                        ultros_api_types::result::ApiError::Forbidden
                    }
                    Some(RetainerError::NotFound) => ultros_api_types::result::ApiError::NotFound,
                    None => ultros_api_types::result::ApiError::Message(
                        "Internal server error".to_string(),
                    ),
                },
            },
            _ => {
                if self.as_status_code().is_server_error() {
                    ultros_api_types::result::ApiError::Message("Internal server error".to_string())
                } else {
                    ultros_api_types::result::ApiError::Message(self.to_string())
                }
            }
        }
    }
}

trait RetainerStatus {
    fn or_else_status(self, retainer_error: Option<&RetainerError>) -> StatusCode;
}

impl RetainerStatus for StatusCode {
    fn or_else_status(self, retainer_error: Option<&RetainerError>) -> StatusCode {
        if self != StatusCode::INTERNAL_SERVER_ERROR {
            return self;
        }
        match retainer_error {
            Some(RetainerError::Forbidden(_)) => StatusCode::FORBIDDEN,
            Some(RetainerError::NotFound) => StatusCode::NOT_FOUND,
            None => self,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let ApiError::DiscordTokenInvalid(mut cookies) = self {
            // remove the discord user cookie
            info!("Removed invalid Discord token");
            cookies = cookies.remove(Cookie::from("discord_auth"));
            // An expired/revoked token is an auth failure like any other, so it
            // gets the same 401. Without an explicit status this tuple response
            // defaulted to `200`.
            return (
                StatusCode::UNAUTHORIZED,
                cookies,
                Json(JsonErrorWrapper::ApiError(
                    ultros_api_types::result::ApiError::NotAuthenticated,
                )),
            )
                .into_response();
        }
        let status = self.as_status_code();
        if status.is_server_error() {
            error!(error = ?self, "Generic API error");
        }
        (
            status,
            Json(JsonErrorWrapper::ApiError(self.as_api_error())),
        )
            .into_response()
    }
}

define_error_enum!(WebError {
    #[error("Not authorized to view this page")]
    NotAuthenticated,
    #[error("Item id {0} is not valid")]
    InvalidItemId(i32),
    #[error("World not found {0}")]
    WorldNotFound(String),
    #[error("Not found")]
    NotFound,
    #[error("Bad request")]
    BadRequest,
});

impl WebError {
    fn as_status_code(&self) -> StatusCode {
        match self {
            WebError::NotAuthenticated => StatusCode::UNAUTHORIZED,
            WebError::NotFound => StatusCode::NOT_FOUND,
            WebError::BadRequest => StatusCode::BAD_REQUEST,
            WebError::InvalidItemId(_) | WebError::WorldNotFound(_) => StatusCode::BAD_REQUEST,
            // Analyzer warm-up isn't a server bug — it's a transient state at
            // startup. 503 lets clients retry instead of treating it as fatal.
            WebError::AnalyzerError(AnalyzerError::Uninitialized) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            WebError::AnalyzerError(AnalyzerError::NotFound) | WebError::WorldSelectError(_) => {
                StatusCode::NOT_FOUND
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let status = self.as_status_code();
        // Analyzer warm-up (503) is an expected transient state at startup, not
        // a real server bug. Keep it out of `tracing::error!` so the
        // `sentry_tracing` layer doesn't capture it as a GlitchTip issue
        // (see issues 5033/5034 — e2e harness racing the warm-up window).
        let is_transient_warmup =
            matches!(self, WebError::AnalyzerError(AnalyzerError::Uninitialized));

        let message = if status.is_server_error() && !is_transient_warmup {
            "Internal server error".to_string()
        } else {
            format!("{self}")
        };

        if status.is_server_error() && !is_transient_warmup {
            tracing::error!(error = %self, %status, "Returning web error");
        } else {
            tracing::debug!(error = %self, %status, "Returning web error");
        }
        (status, message).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unauthenticated request must answer `401`, not `200`.
    ///
    /// `AuthDiscordUser`'s extractor rejection is `ApiError::NoAuthCookie`, so
    /// this is the status every logged-out request to every authenticated API
    /// route gets. Answering `200` with an `{"ApiError":"NotAuthenticated"}`
    /// body makes an auth failure indistinguishable from success at the HTTP
    /// layer: the SSR fetch helper takes its `status.is_success()` branch, the
    /// structured error then looks like a *deserialization* failure, and it
    /// gets reported at error level (the GlitchTip 2218 / 2210 lineage that
    /// `ultros-app/src/api.rs` carries two separate workaround comments for).
    #[test]
    fn no_auth_cookie_is_unauthorized_not_ok() {
        let err = ApiError::NoAuthCookie;
        assert_eq!(err.as_status_code(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            err.into_response().status(),
            StatusCode::UNAUTHORIZED,
            "the response status must match as_status_code()"
        );
    }

    /// An expired/revoked Discord token is also an auth failure, so it gets the
    /// same `401`. This arm returns early in `into_response` to attach the
    /// cookie removal, and previously returned no status at all — which axum
    /// defaults to `200`.
    ///
    /// Only the status is asserted: the cookie-clearing behaviour is untouched
    /// by this change, and an empty test jar can't reproduce it anyway —
    /// `CookieJar::remove` only emits a removal `Set-Cookie` when the name is
    /// already in `original_cookies`, which in production it is (we only reach
    /// this variant when a `discord_auth` cookie was present but Discord
    /// rejected the token).
    #[test]
    fn discord_token_invalid_is_unauthorized() {
        let jar = PrivateCookieJar::new(Key::generate());
        let response = ApiError::DiscordTokenInvalid(jar).into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// The wire body is unchanged — clients match on `NotAuthenticated` (e.g.
    /// the list-invite login redirect in `ultros-app/src/routes/lists.rs`), so
    /// only the status moves.
    #[test]
    fn auth_failures_keep_their_structured_body() {
        assert_eq!(
            ApiError::NoAuthCookie.as_api_error(),
            ultros_api_types::result::ApiError::NotAuthenticated
        );
    }

    /// 401 is a *client* error, so it must not trip the `is_server_error()`
    /// paths that log at error level and replace the message with a generic
    /// "Internal server error". This is what preserves the original intent of
    /// the `NoAuthCookie => OK` mapping ("I don't want a real error") without
    /// lying about the status.
    #[test]
    fn auth_failure_is_not_reported_as_a_server_error() {
        assert!(!ApiError::NoAuthCookie.as_status_code().is_server_error());
    }
}
