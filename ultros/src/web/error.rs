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
    SeaDbErr, common_type_conversions::ApiConversionError, world_data::world_cache::WorldCacheError,
};

use crate::{analyzer_service::AnalyzerError, event};

use super::character_verifier_service::VerifierError;

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
                    oauth2::reqwest::Error<reqwest::Error>,
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
            #[error("Verifier error {0}")]
            VerificationError(#[from] VerifierError),
            #[error("Error generating sitemap {0}")]
            SiteMapError(#[from] SitemapIndexError),
            #[error("Error generating url set {0}")]
            UrlSetError(#[from] UrlSetError),
            #[error("Token error {0}")]
            TokenError(
                #[from]
                RequestTokenError<
                    oauth2::reqwest::Error<reqwest::Error>,
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
});

impl ApiError {
    fn as_status_code(&self) -> StatusCode {
        match self {
            ApiError::NoAuthCookie => StatusCode::OK, // In this case I don't want a real error.
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let ApiError::DiscordTokenInvalid(mut cookies) = self {
            // remove the discord user cookie
            info!("Removed invalid Discord token");
            cookies = cookies.remove(Cookie::from("discord_auth"));
            return (
                cookies,
                Json(JsonErrorWrapper::ApiError(
                    ultros_api_types::result::ApiError::NotAuthenticated,
                )),
            )
                .into_response();
        }
        if let ApiError::NoAuthCookie = self {
            return (
                self.as_status_code(),
                Json(JsonErrorWrapper::ApiError(
                    ultros_api_types::result::ApiError::NotAuthenticated,
                )),
            )
                .into_response();
        }
        error!(error = ?self, "Generic API error");
        (self.as_status_code(), Json(JsonErrorWrapper::from(self))).into_response()
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
        if status.is_server_error() {
            tracing::error!(error = %self, %status, "Returning web error");
        } else {
            tracing::debug!(error = %self, %status, "Returning web error");
        }
        (status, format!("{self}")).into_response()
    }
}
