use std::{error, fmt::Display, rc::Rc};

use leptos::SerializationError;
use serde::{de::Visitor, Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, Deserialize, Serialize)]
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
}

/// This error type implements From's for the non serializable error types and shoves them into a string
/// Upon being actually serialized
#[derive(Clone, Debug)]
pub enum SystemError {
    Message(String),
    #[cfg(feature = "ssr")]
    ReqwestError(Rc<reqwest::Error>),
    #[cfg(not(feature = "ssr"))]
    GlooError(Rc<gloo_net::Error>),
    SerializationError(SerializationError),
    Anyhow(Rc<anyhow::Error>),
}

impl From<anyhow::Error> for SystemError {
    fn from(value: anyhow::Error) -> Self {
        Self::Anyhow(Rc::new(value))
    }
}

impl From<anyhow::Error> for AppError {
    fn from(value: anyhow::Error) -> Self {
        Self::SystemError(value.into())
    }
}

#[cfg(feature = "ssr")]
impl From<reqwest::Error> for SystemError {
    fn from(value: reqwest::Error) -> Self {
        Self::ReqwestError(Rc::new(value))
    }
}

#[cfg(feature = "ssr")]
impl From<reqwest::Error> for AppError {
    fn from(value: reqwest::Error) -> Self {
        Self::SystemError(value.into())
    }
}

#[cfg(not(feature = "ssr"))]
impl From<gloo_net::Error> for SystemError {
    fn from(value: gloo_net::Error) -> Self {
        Self::GlooError(Rc::new(value))
    }
}

#[cfg(not(feature = "ssr"))]
impl From<gloo_net::Error> for AppError {
    fn from(value: gloo_net::Error) -> Self {
        Self::SystemError(value.into())
    }
}

impl From<SerializationError> for SystemError {
    fn from(value: SerializationError) -> Self {
        Self::SerializationError(value)
    }
}

impl From<SerializationError> for AppError {
    fn from(value: SerializationError) -> Self {
        Self::SystemError(value.into())
    }
}

impl Display for SystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemError::Message(message) => write!(f, "{}", message),
            #[cfg(feature = "ssr")]
            SystemError::ReqwestError(reqwest) => write!(f, "{}", reqwest),
            #[cfg(not(feature = "ssr"))]
            SystemError::GlooError(g) => write!(f, "{}", g),
            SystemError::SerializationError(serialization) => write!(f, "{}", serialization),
            SystemError::Anyhow(anyhow) => write!(f, "{}", anyhow),
        }
    }
}

impl error::Error for SystemError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            SystemError::Message(_) => None,
            #[cfg(feature = "ssr")]
            SystemError::ReqwestError(reqwest) => Some(reqwest.as_ref()),
            #[cfg(not(feature = "ssr"))]
            SystemError::GlooError(gloo) => Some(gloo.as_ref()),
            SystemError::SerializationError(serialize) => Some(serialize),
            SystemError::Anyhow(anyhow) => Some(anyhow.root_cause()),
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
    #[test]
    fn error_types() {}
}
