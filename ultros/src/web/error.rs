use std::{num::ParseIntError, sync::Arc};

use aide::OperationOutput;
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
    SeaDbErr, common_type_conversions::ApiConversionError, world_cache::WorldCacheError,
};

use crate::{analyzer_service::AnalyzerError, event};

use super::character_verifier_service::VerifierError;

#[derive(Debug, Error, schemars::JsonSchema)]
pub enum ApiError {
    #[error("OAuth configuration error {0}")]
    #[schemars(skip)]
    ConfigurationError(#[from] ConfigurationError),
    #[error("Error creating oauth token {0}")]
    #[schemars(skip)]
    RequestErrorToken(
        #[from]
        RequestTokenError<
            oauth2::reqwest::Error<reqwest::Error>,
            StandardErrorResponse<RevocationErrorResponseType>,
        >,
    ),
    #[error("Generic error {0}")]
    #[schemars(skip)]
    AnyhowError(#[from] anyhow::Error),
    #[error("Parse int failed {0}")]
    #[schemars(skip)]
    ParseIntError(#[from] ParseIntError),
    #[error("{0}")]
    #[schemars(skip)]
    WorldSelectError(#[from] WorldCacheError),
    #[error("Db Error {0}")]
    #[schemars(skip)]
    DbError(#[from] SeaDbErr),
    #[error("Error communicaing with universalis {0}")]
    #[schemars(skip)]
    UniversalisError(#[from] universalis::Error),
    #[error("Error sending listing update {0}")]
    #[schemars(skip)]
    ListingSendError(
        #[from] SendError<event::EventType<Arc<Vec<ultros_db::entity::active_listing::Model>>>>,
    ),
    #[error("Error making an internal HTTP request {0}")]
    #[schemars(skip)]
    ReqwestError(#[from] reqwest::Error),
    #[error("Internal HTTP Error {0}")]
    #[schemars(skip)]
    AxumError(#[from] axum::http::Error),
    #[error("IO Error {0}")]
    #[schemars(skip)]
    StdError(#[from] std::io::Error),
    #[error("Error reading lodestone server name {0}")]
    #[schemars(skip)]
    LodestoneServerParse(#[from] lodestone::model::server::ServerParseError),
    #[error("Lodestone error {0}")]
    #[schemars(skip)]
    LodestoneError(#[from] lodestone::LodestoneError),
    // this is kind of bad if I ever use the elapsed error for something else but I'll pretend
    #[error("Universalis is being slow. {0}. Will continue waiting")]
    #[schemars(skip)]
    TimeoutElapsed(#[from] Elapsed),
    #[error("Analyzer Error: {0}")]
    #[schemars(skip)]
    AnalyzerError(#[from] AnalyzerError),
    #[error("Verifier error {0}")]
    #[schemars(skip)]
    VerificationError(#[from] VerifierError),
    #[error("Error generating sitemap {0}")]
    #[schemars(skip)]
    SiteMapError(#[from] SitemapIndexError),
    #[error("Error generating url set {0}")]
    #[schemars(skip)]
    UrlSetError(#[from] UrlSetError),
    #[error("Token error {0}")]
    #[schemars(skip)]
    TokenError(
        #[from]
        RequestTokenError<
            oauth2::reqwest::Error<reqwest::Error>,
            StandardErrorResponse<BasicErrorResponseType>,
        >,
    ),
    #[error("API conversions error {0}")]
    #[schemars(skip)]
    ApiConversionError(#[from] ApiConversionError),
    #[error("No Auth Cookie")]
    NoAuthCookie,
    #[error("Discord token was invalid")]
    #[schemars(skip)]
    DiscordTokenInvalid(PrivateCookieJar<Key>),
}

impl OperationOutput for ApiError {
    type Inner = Json<JsonErrorWrapper>;

    fn operation_response(
        _ctx: &mut aide::gen::GenContext,
        _operation: &mut aide::openapi::Operation,
    ) -> Option<aide::openapi::Response> {
        // You can use the `ctx` to get access to the type stores, etc.
        // The `operation` can be used to modify the operation.
        //
        // But here, we just return a fixed response.
        Some(aide::openapi::Response {
            description: "An API error".into(),
            ..aide::openapi::Response::default()
        })
    }

    fn inferred_responses(
        _ctx: &mut aide::gen::GenContext,
        _operation: &mut aide::openapi::Operation,
    ) -> Vec<(Option<u16>, aide::openapi::Response)> {
        // You can also use this to let aide infer the responses.
        // We are going to provide a fixed response, so we don't need this.
        vec![(
            Some(200),
            aide::openapi::Response {
                description: "An API error".into(),
                ..aide::openapi::Response::default()
            },
        )]
    }
}

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
    #[error("Item id {0} is not valid")]
    InvalidItemId(i32),
    #[error("World not found {0}")]
    WorldNotFound(String),
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
        tracing::error!(error = %self, "Returning web error");

        (self.as_status_code(), format!("{self}")).into_response()
    }
}
