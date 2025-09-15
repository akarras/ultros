use std::{error, fmt::Display, sync::Arc};

use serde::{de::Visitor, Deserialize, Serialize};
use thiserror::Error;
use ultros_api_types::result::ApiError;

#[derive(Debug, Error, Clone, Deserialize, Serialize, PartialEq)]
pub enum AppError {
    #[error("JSON {0}")]
    Json(String),
    #[error("System error {0}")]
    SystemError(#[from] SystemError),
    #[error("No valid item ID was provided to the request")]
    NoItem,
    #[error("Can't search an empty string")]
    EmptyString,
    #[error("Retainer didn't have any items")]
    NoRetainerItems,
    #[error("List does not exist")]
    BadList,
    #[error("Url missing dynamic parameter")]
    ParamMissing,
    #[error("{0}")]
    ApiError(#[from] ApiError),
    #[error("Homeworld not set")]
    NoHomeWorld,
}

/// This error type implements From's for the non serializable error types and shoves them into a string
/// Upon being actually serialized
#[derive(Clone, Debug)]
pub enum SystemError {
    Message(String),
    ReqwestError(Arc<reqwest::Error>),
    Anyhow(Arc<anyhow::Error>),
    #[cfg(feature = "hydrate")]
    GlooNet(Arc<gloo_net::Error>),
}

impl PartialEq for SystemError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Message(l0), Self::Message(r0)) => l0 == r0,
            _ => false,
        }
    }
}

impl From<anyhow::Error> for SystemError {
    fn from(value: anyhow::Error) -> Self {
        Self::Anyhow(Arc::new(value))
    }
}

impl From<anyhow::Error> for AppError {
    fn from(value: anyhow::Error) -> Self {
        Self::SystemError(value.into())
    }
}

impl From<reqwest::Error> for SystemError {
    fn from(value: reqwest::Error) -> Self {
        Self::ReqwestError(Arc::new(value))
    }
}

impl From<reqwest::Error> for AppError {
    fn from(value: reqwest::Error) -> Self {
        Self::SystemError(value.into())
    }
}

#[cfg(feature = "hydrate")]
impl From<gloo_net::Error> for AppError {
    fn from(value: gloo_net::Error) -> Self {
        Self::SystemError(SystemError::GlooNet(Arc::new(value)))
    }
}

impl Display for SystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemError::Message(message) => write!(f, "{}", message),
            SystemError::ReqwestError(reqwest) => write!(f, "{}", reqwest),
            SystemError::Anyhow(anyhow) => write!(f, "{}", anyhow),
            #[cfg(feature = "hydrate")]
            SystemError::GlooNet(error) => write!(f, "{}", error),
        }
    }
}

impl error::Error for SystemError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            SystemError::Message(_) => None,
            SystemError::ReqwestError(reqwest) => Some(reqwest.as_ref()),
            SystemError::Anyhow(anyhow) => Some(anyhow.root_cause()),
            #[cfg(feature = "hydrate")]
            SystemError::GlooNet(error) => Some(error),
        }
    }
}

impl Serialize for SystemError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

struct StringVisitor;

impl<'de> Visitor<'de> for StringVisitor {
    type Value = String;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "Expecting a string type")
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(v)
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(v.to_string())
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(v.to_string())
    }
}

impl<'de> Deserialize<'de> for SystemError {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string = deserializer.deserialize_string(StringVisitor)?;
        Ok(Self::Message(string))
    }
}

pub(crate) type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod test {
    use crate::error::SystemError;

    use super::AppError;

    #[test]
    fn error_types() {
        let sample_error = "{\"Err\":{\"SystemError\":\"error deserializing Resource: expected value at line 1 column 1\"}}";
        let app_error = serde_json::from_str::<Result<(), AppError>>(sample_error).unwrap();
        assert_eq!(
            app_error,
            Err(AppError::SystemError(SystemError::Message(
                "error deserializing Resource: expected value at line 1 column 1".to_string()
            )))
        );
    }
}

