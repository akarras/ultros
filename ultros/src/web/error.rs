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
        (self.as_status_code(), format!("{self}")).into_response()
    }
}
