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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_display_messages() {
        let e = ApiError::Message("boom".into());
        assert_eq!(e.to_string(), "Generic error: boom");
        let e = ApiError::NotAuthenticated;
        assert_eq!(
            e.to_string(),
            "Unauthenticated. Please login to use this feature"
        );
    }

    #[test]
    fn api_error_serde_roundtrip() {
        for e in [
            ApiError::Message("oops".into()),
            ApiError::NotAuthenticated,
        ] {
            let s = serde_json::to_string(&e).unwrap();
            let back: ApiError = serde_json::from_str(&s).unwrap();
            assert_eq!(e, back);
        }
    }

    #[test]
    fn json_error_wrapper_from_std_error_captures_message() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let wrapper: JsonErrorWrapper = io.into();
        match wrapper {
            JsonErrorWrapper::ApiError(ApiError::Message(s)) => assert_eq!(s, "missing"),
            _ => panic!("expected ApiError::Message"),
        }
    }
}
