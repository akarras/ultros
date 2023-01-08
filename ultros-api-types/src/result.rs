use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub enum ApiError {
    Generic,
}

#[derive(Deserialize, Serialize)]
pub enum ApiResult<T> {
    Ok(T),
    Error(ApiError),
}
