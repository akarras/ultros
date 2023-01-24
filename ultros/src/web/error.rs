use std::{num::ParseIntError, sync::Arc};

use axum::{
    response::{IntoResponse, Redirect, Response},
    Json,
};
use image::ImageError;
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
use ultros_db::{world_cache::WorldCacheError, SeaDbErr};

use crate::{analyzer_service::AnalyzerError, event};

use super::character_verifier_service::VerifierError;

#[derive(Debug, Error)]
pub enum ApiError {
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
    #[error("Home world has not been set")]
    HomeWorldNotSet,
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
    #[error("Image error {0}")]
    Image(#[from] ImageError),
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
    #[error("Invalid item")]
    InvalidItem(i32),
    #[error("Token error {0}")]
    TokenError(
        #[from]
        RequestTokenError<
            oauth2::reqwest::Error<reqwest::Error>,
            StandardErrorResponse<BasicErrorResponseType>,
        >,
    ),
}

impl ApiError {
    fn as_status_code(&self) -> StatusCode {
        match self {
            ApiError::NotAuthenticated => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        error!("error {}", self);
        let e = format!("{self}");

        (self.as_status_code(), Json(JsonError { error_message: e })).into_response()
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
    #[error("Home world has not been set")]
    HomeWorldNotSet,
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
    #[error("Image error {0}")]
    Image(#[from] ImageError),
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
    #[error("Invalid item")]
    InvalidItem(i32),
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
        if let WebError::HomeWorldNotSet = self {
            return Redirect::to("/profile").into_response();
        }
        (self.as_status_code(), format!("{self}")).into_response()
    }
}
