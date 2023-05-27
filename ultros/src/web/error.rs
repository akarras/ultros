use std::{num::ParseIntError, sync::Arc};

use axum::{
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::extract::{
    cookie::{Cookie, Key},
    PrivateCookieJar,
};
use oauth2::{
    basic::BasicErrorResponseType, ConfigurationError, RequestTokenError,
    RevocationErrorResponseType, StandardErrorResponse,
};
use reqwest::StatusCode;
use sitemap_rs::{sitemap_index_error::SitemapIndexError, url_set_error::UrlSetError};
use thiserror::Error;
use tokio::{sync::broadcast::error::SendError, time::error::Elapsed};
use tracing::log::error;
use ultros_api_types::result::JsonError;
use ultros_db::{
    common_type_conversions::ApiConversionError, world_cache::WorldCacheError, SeaDbErr,
};

use crate::{analyzer_service::AnalyzerError, event};

use super::character_verifier_service::VerifierError;

#[derive(Debug, Error)]
pub enum ApiError {
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
    #[error("API conversions error {0}")]
    ApiConversionError(#[from] ApiConversionError),
    #[error("Not authenticated")]
    NotAuthenticated,
    #[error("Discord token was invalid")]
    DiscordTokenInvalid(PrivateCookieJar<Key>),
}

impl ApiError {
    fn as_status_code(&self) -> StatusCode {
        match self {
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        error!("error {}", self);
        if let ApiError::DiscordTokenInvalid(mut cookies) = self {
            // remove the discord user cookie
            cookies = cookies.remove(Cookie::named("discord_auth"));
            return (
                cookies,
                Json(JsonError::ApiError(
                    ultros_api_types::result::ApiError::NotAuthenticated,
                )),
            )
                .into_response();
        }
        if let ApiError::NotAuthenticated = self {
            return (
                self.as_status_code(),
                Json(JsonError::ApiError(
                    ultros_api_types::result::ApiError::NotAuthenticated,
                )),
            )
                .into_response();
        }

        (self.as_status_code(), Json(JsonError::from(self))).into_response()
    }
}

#[derive(Debug, Error)]
pub enum WebError {
    #[error("Not authorized to view this page")]
    NotAuthenticated,
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
}

impl WebError {
    fn as_status_code(&self) -> StatusCode {
        match self {
            WebError::NotAuthenticated => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        error!("Error returned {self:?}");

        (self.as_status_code(), format!("{self}")).into_response()
    }
}
