use std::{error, fmt::Display, sync::Arc};

use serde::{Deserialize, Serialize, de::Visitor};
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
    /// The world list never loaded, so no world/datacenter/region name can be resolved.
    ///
    /// Produced when `LocalWorldData` is absent or holds an `Err` — on the client that is a
    /// failed `/api/v1/world_data` fetch (see `ultros-client`'s bootstrap, which stores the
    /// failure via `LocalWorldData::failed`).
    #[error("World data is unavailable")]
    WorldDataUnavailable,
    #[error("The internal API request timed out")]
    InternalApiTimeout,
}

impl AppError {
    /// Whether this is the API *answering* with an error, as opposed to the
    /// call to it failing.
    ///
    /// An [`AppError::ApiError`] is a response the server deliberately
    /// produced. By the time a caller sees one it has already been logged with
    /// the status and the path it came from — `get_api` warns on every
    /// non-success status, and the server reports its own 5xx directly
    /// (`ultros/src/web/error.rs`). Re-reporting it at error level from the
    /// component that awaited the resource adds no context and creates a
    /// *second* GlitchTip issue for one failure.
    ///
    /// That double report is GlitchTip issue 2210 ("Error getting value"). Its
    /// live traffic is a bad world name in the URL: a request for
    /// `/api/v1/listings/綛糸襲臂ゅ甥/42525` — mojibake where the world segment
    /// belongs — is a 404 the API is *right* to return, logged at warn by the
    /// fetch layer and then immediately re-logged at error by the caller.
    ///
    /// Everything else — a transport failure, a malformed body on a 2xx, a
    /// missing route parameter — is our own side breaking, is reported nowhere
    /// else, and stays at error level.
    pub(crate) fn is_api_response(&self) -> bool {
        matches!(self, AppError::ApiError(_))
    }

    /// A busy backend can still exceed the SSR deadline without a network hop.
    /// Keep these transient failures out of duplicate error-level reporting.
    pub(crate) fn is_transient_transport(&self) -> bool {
        matches!(self, Self::InternalApiTimeout)
    }
}

/// This error type implements From's for the non serializable error types and shoves them into a string
/// Upon being actually serialized
#[derive(Clone, Debug)]
pub enum SystemError {
    Message(String),
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

    /// Regression for GlitchTip issue 2210. A 404 for a bad world name in the
    /// URL is the API answering, not a failure of ours — the fetch layer has
    /// already logged it with its status and path, so the awaiting component
    /// must not report it a second time at error level.
    #[test]
    fn api_error_responses_are_the_api_answering() {
        use ultros_api_types::result::ApiError;

        for api in [
            // The shape the bad-world-name 404 actually arrives in: the server
            // flattens `WorldCacheError` into `Message` on its way out.
            ApiError::Message("Name lookup error 綛糸襲臂ゅ甥".to_string()),
            ApiError::NotFound,
            ApiError::NotAuthenticated,
            ApiError::Forbidden,
            ApiError::BadRequest("nope".to_string()),
        ] {
            let error = AppError::ApiError(api.clone());
            assert!(
                error.is_api_response(),
                "{api:?} is a response the API produced and already logged"
            );
        }
    }

    /// The complement: anything that is *our* side breaking is reported
    /// nowhere else, so it must keep error-level reporting.
    #[test]
    fn our_own_failures_are_not_api_responses() {
        for error in [
            // A 2xx whose body would not deserialize — a real bug.
            AppError::Json("expected value at line 1 column 1".to_string()),
            AppError::SystemError(SystemError::Message("connection reset".to_string())),
            AppError::ParamMissing,
            AppError::WorldDataUnavailable,
            AppError::NoItem,
        ] {
            assert!(
                !error.is_api_response(),
                "{error:?} is not something the API answered with"
            );
        }
    }
}

#[cfg(test)]
mod transient_transport_tests {
    use super::AppError;

    #[test]
    fn internal_timeout_is_transient_and_survives_hydration_serialization() {
        let error = AppError::InternalApiTimeout;
        let serialized = serde_json::to_string(&error).unwrap();
        let decoded: AppError = serde_json::from_str(&serialized).unwrap();
        assert!(decoded.is_transient_transport());
        assert!(!decoded.is_api_response());
    }

    #[test]
    fn a_deserialize_failure_is_not_transient() {
        assert!(!AppError::Json("invalid JSON".into()).is_transient_transport());
    }
}
