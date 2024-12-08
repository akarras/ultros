use std::error::Error;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Deserialize, Serialize, Error, Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum ApiError {
    #[error("Generic error: {0}")]
    Message(String),
    #[error("Unauthenticated. Please login to use this feature")]
    NotAuthenticated,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum JsonErrorWrapper {
    ApiError(ApiError),
}

impl<E> From<E> for JsonErrorWrapper
where
    E: Error,
{
    fn from(value: E) -> Self {
        let str_value = value.to_string();
        Self::ApiError(ApiError::Message(str_value))
    }
}
