use std::{num::ParseIntError, sync::Arc};

use axum::response::{IntoResponse, Redirect, Response};
use image::ImageError;
use oauth2::{
    ConfigurationError, RequestTokenError, RevocationErrorResponseType, StandardErrorResponse,
};
use reqwest::StatusCode;
use thiserror::Error;
use tokio::{sync::broadcast::error::SendError, time::error::Elapsed};
use tracing::log::error;
use ultros_db::SeaDbErr;

use crate::{analyzer_service::AnalyzerError, event, world_cache::WorldCacheError};

use super::character_verifier_service::VerifierError;

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
    #[error("Could not find an item with the ID of {0}")]
    InvalidItem(i32),
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
}

impl WebError {
    fn as_status_code(&self) -> StatusCode {
        match self {
            WebError::NotAuthenticated => StatusCode::UNAUTHORIZED,
            WebError::InvalidItem(_) => StatusCode::BAD_REQUEST,
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
