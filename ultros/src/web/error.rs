use axum::response::{IntoResponse, Response};
use oauth2::{
    ConfigurationError, RequestTokenError, RevocationErrorResponseType, StandardErrorResponse,
};
use reqwest::StatusCode;
use thiserror::Error;

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
    #[error("Prometheus error {0}")]
    AnalyticsError(#[from] prometheus::Error),
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
        (self.as_status_code(), format!("{self}")).into_response()
    }
}
