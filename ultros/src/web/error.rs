use std::{num::ParseIntError, sync::Arc};

use axum::{
    Json,
    response::{IntoResponse, Response},
};
use axum_extra::extract::{PrivateCookieJar, cookie::Key};
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
    SeaDbErr, common_type_conversions::ApiConversionError, lists::ListError,
    retainers::RetainerError, world_data::world_cache::WorldCacheError,
};

use crate::{analyzer_service::AnalyzerError, event};

use crate::character_claim::ClaimError;
use crate::lodestone_profile::ProfileError;

/// A ClickHouse call that failed, tagged with which query it was and why it
/// failed.
///
/// Exists so ClickHouse failures stop falling into [`AnyhowError`]'s
/// `"Generic error {0}"` catch-all. Two things were wrong with going through
/// `anyhow`: the typed error was flattened to a string at the call site, and the
/// string it was flattened into carried ClickHouse's live memory figures, which
/// differ on every occurrence.
///
/// `query` is a `&'static str` and `kind` is a small enum precisely so
/// [`Display`](std::fmt::Display) stays low-cardinality: `query × kind` is a
/// handful of possible messages, each one alertable. Per-occurrence detail lives
/// on `source`, which callers log as a structured field.
///
/// Note the `Display` here *does* include `source`, volatile figures and all —
/// deliberately. It renders into the `error` **field**, which is not part of the
/// grouping key, so an operator still sees "would use 5.44 GiB, maximum: 5.40
/// GiB" on the issue. Only [`report_title`], which builds the grouping key,
/// leaves it out.
///
/// [`AnyhowError`]: WebError::AnyhowError
#[derive(Debug, Error)]
#[error("ClickHouse {query} query failed ({kind}): {source}")]
pub struct ClickHouseQueryError {
    /// Which query failed — the function name in `ultros_clickhouse::queries`.
    pub query: &'static str,
    pub kind: ultros_clickhouse::ClickHouseErrorKind,
    #[source]
    pub source: ultros_clickhouse::ClickHouseError,
}

impl ClickHouseQueryError {
    /// Classify `source` and tag it with the query that produced it.
    pub fn new(query: &'static str, source: ultros_clickhouse::ClickHouseError) -> Self {
        let kind = source.kind();
        Self {
            query,
            kind,
            source,
        }
    }
}

/// Generates an `Error`-deriving enum with the variants shared between `ApiError` and `WebError`.
/// The shared variants and their `#[from]` / `#[error]` attributes are kept in one place so the
/// two enums can't drift. Caller passes in any enum-specific variants between braces.
macro_rules! define_error_enum {
    ($name:ident { $($extra:tt)* }) => {
        #[derive(Debug, Error)]
        pub enum $name {
            #[error("OAuth configuration error {0}")]
            ConfigurationError(#[from] ConfigurationError),
            #[error("Error creating oauth token {0}")]
            RequestErrorToken(
                #[from]
                RequestTokenError<
                    oauth2::HttpClientError<oauth2::reqwest::Error>,
                    StandardErrorResponse<RevocationErrorResponseType>,
                >,
            ),
            #[error("Generic error {0}")]
            AnyhowError(#[from] anyhow::Error),
            // Kept ahead of the `anyhow` catch-all on purpose: a ClickHouse
            // failure that reaches `AnyhowError` loses its type and, with it,
            // any hope of being alerted on specifically.
            #[error(transparent)]
            ClickHouse(#[from] ClickHouseQueryError),
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
            #[error("Character claim error {0}")]
            CharacterClaimError(#[from] ClaimError),
            #[error("Error generating sitemap {0}")]
            SiteMapError(#[from] SitemapIndexError),
            #[error("Error generating url set {0}")]
            UrlSetError(#[from] UrlSetError),
            #[error("Token error {0}")]
            TokenError(
                #[from]
                RequestTokenError<
                    oauth2::HttpClientError<oauth2::reqwest::Error>,
                    StandardErrorResponse<BasicErrorResponseType>,
                >,
            ),
            $($extra)*
        }
    };
}

define_error_enum!(ApiError {
    #[error("API conversions error {0}")]
    ApiConversionError(#[from] ApiConversionError),
    #[error("No Auth Cookie")]
    NoAuthCookie,
    #[error("Discord token was invalid")]
    DiscordTokenInvalid(PrivateCookieJar<Key>),
    #[error("{0}")]
    Forbidden(&'static str),
});

impl ApiError {
    fn as_status_code(&self) -> StatusCode {
        match self {
            // Auth failures are 401 — the same status `WebError::NotAuthenticated`
            // already uses for page routes. This used to answer `200` to avoid
            // "a real error", but a 401 achieves that intent without lying about
            // the status: it's a *client* error, so it never trips the
            // `is_server_error()` branches below that log at error level.
            //
            // Answering 200 made an auth failure indistinguishable from success
            // at the HTTP layer, so the SSR fetch helper took its
            // `status.is_success()` branch and reported the structured
            // `{"ApiError":"NotAuthenticated"}` body as a *deserialization*
            // failure at error level (the GlitchTip 2218/2210 lineage).
            ApiError::NoAuthCookie | ApiError::DiscordTokenInvalid(_) => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            // A character id that the Lodestone doesn't know is a bad request
            // parameter, not a server fault - answering 500 both lied to the
            // caller and reported the typo to GlitchTip.
            ApiError::CharacterClaimError(ClaimError::Lodestone(
                ProfileError::CharacterNotFound(_),
            )) => StatusCode::NOT_FOUND,
            ApiError::AnyhowError(e) => match e.downcast_ref::<ListError>() {
                Some(ListError::Forbidden(_)) => StatusCode::FORBIDDEN,
                Some(ListError::NotFound | ListError::InviteNotFound) => StatusCode::NOT_FOUND,
                Some(ListError::BadRequest(_) | ListError::InviteExhausted) => {
                    StatusCode::BAD_REQUEST
                }
                None => StatusCode::INTERNAL_SERVER_ERROR,
            }
            .or_else_status(e.downcast_ref::<RetainerError>()),
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn as_api_error(&self) -> ultros_api_types::result::ApiError {
        match self {
            ApiError::NoAuthCookie => ultros_api_types::result::ApiError::NotAuthenticated,
            ApiError::Forbidden(_) => ultros_api_types::result::ApiError::Forbidden,
            ApiError::CharacterClaimError(ClaimError::Lodestone(
                ProfileError::CharacterNotFound(_),
            )) => ultros_api_types::result::ApiError::NotFound,
            ApiError::AnyhowError(e) => match e.downcast_ref::<ListError>() {
                Some(ListError::Forbidden(_)) => ultros_api_types::result::ApiError::Forbidden,
                Some(ListError::NotFound | ListError::InviteNotFound) => {
                    ultros_api_types::result::ApiError::NotFound
                }
                Some(ListError::BadRequest(msg)) => {
                    ultros_api_types::result::ApiError::BadRequest((*msg).into())
                }
                Some(ListError::InviteExhausted) => ultros_api_types::result::ApiError::BadRequest(
                    "Invite has reached max uses".into(),
                ),
                None => match e.downcast_ref::<RetainerError>() {
                    Some(RetainerError::Forbidden(_)) => {
                        ultros_api_types::result::ApiError::Forbidden
                    }
                    Some(RetainerError::NotFound) => ultros_api_types::result::ApiError::NotFound,
                    None => ultros_api_types::result::ApiError::Message(
                        "Internal server error".to_string(),
                    ),
                },
            },
            _ => {
                if self.as_status_code().is_server_error() {
                    ultros_api_types::result::ApiError::Message("Internal server error".to_string())
                } else {
                    ultros_api_types::result::ApiError::Message(self.to_string())
                }
            }
        }
    }
}

trait RetainerStatus {
    fn or_else_status(self, retainer_error: Option<&RetainerError>) -> StatusCode;
}

impl RetainerStatus for StatusCode {
    fn or_else_status(self, retainer_error: Option<&RetainerError>) -> StatusCode {
        if self != StatusCode::INTERNAL_SERVER_ERROR {
            return self;
        }
        match retainer_error {
            Some(RetainerError::Forbidden(_)) => StatusCode::FORBIDDEN,
            Some(RetainerError::NotFound) => StatusCode::NOT_FOUND,
            None => self,
        }
    }
}

/// [`report_title`]'s counterpart for [`ApiError`]. Kept as two small functions
/// rather than a trait: the enums are macro-generated and only share variants,
/// not a common type, and two three-line matches read better than the generic
/// machinery needed to unify them.
fn api_report_title(error: &ApiError) -> std::borrow::Cow<'static, str> {
    match error {
        ApiError::ClickHouse(e) => {
            std::borrow::Cow::Owned(format!("ClickHouse {} query failed ({})", e.query, e.kind))
        }
        _ => std::borrow::Cow::Borrowed("Generic API error"),
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let ApiError::DiscordTokenInvalid(mut cookies) = self {
            // remove the discord user cookie
            info!("Removed invalid Discord token");
            cookies = cookies.remove(super::oauth::discord_auth_removal_cookie());
            // An expired/revoked token is an auth failure like any other, so it
            // gets the same 401. Without an explicit status this tuple response
            // defaulted to `200`.
            return (
                StatusCode::UNAUTHORIZED,
                cookies,
                Json(JsonErrorWrapper::ApiError(
                    ultros_api_types::result::ApiError::NotAuthenticated,
                )),
            )
                .into_response();
        }
        let status = self.as_status_code();
        if status.is_server_error() {
            // Same grouping rule as `WebError` — see `report_title`. The API
            // routes are where the ClickHouse-backed endpoints live
            // (item_stats, movers, resale_quality, market_heat), so collapsing
            // them all under "Generic API error" is what made a ClickHouse
            // outage indistinguishable from any other 500.
            let title = api_report_title(&self);
            error!(error = ?self, "{title}");
        }
        (
            status,
            Json(JsonErrorWrapper::ApiError(self.as_api_error())),
        )
            .into_response()
    }
}

define_error_enum!(WebError {
    #[error("Not authorized to view this page")]
    NotAuthenticated,
    #[error("Not found")]
    NotFound,
    #[error("Bad request")]
    BadRequest,
    #[error("Service temporarily unavailable")]
    TemporarilyUnavailable,
});

/// The title error reporting groups this error under.
///
/// `tracing`'s *message* is the grouping key — structured fields are not — so a
/// constant message collapses every 5xx into a single undifferentiated issue.
/// That is what `"Returning web error"` did: a ClickHouse outage and an OAuth
/// failure landed in the same bucket, so neither could be alerted on. Naming the
/// failure class here splits them, while `query × kind` keeps the number of
/// distinct titles small enough that each accumulates a count instead of
/// splintering.
///
/// Everything else keeps the original title so existing issues stay continuous.
fn report_title(error: &WebError) -> std::borrow::Cow<'static, str> {
    match error {
        WebError::ClickHouse(e) => {
            std::borrow::Cow::Owned(format!("ClickHouse {} query failed ({})", e.query, e.kind))
        }
        _ => std::borrow::Cow::Borrowed("Returning web error"),
    }
}

impl WebError {
    fn as_status_code(&self) -> StatusCode {
        match self {
            WebError::NotAuthenticated => StatusCode::UNAUTHORIZED,
            WebError::NotFound => StatusCode::NOT_FOUND,
            WebError::BadRequest => StatusCode::BAD_REQUEST,
            WebError::TemporarilyUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            // Analyzer warm-up isn't a server bug — it's a transient state at
            // startup. 503 lets clients retry instead of treating it as fatal.
            WebError::AnalyzerError(AnalyzerError::Uninitialized) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            WebError::AnalyzerError(AnalyzerError::NotFound) | WebError::WorldSelectError(_) => {
                StatusCode::NOT_FOUND
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let status = self.as_status_code();
        // Expected 503s are transient states, not server bugs. Keep them out
        // of `tracing::error!` so the `sentry_tracing` layer doesn't capture
        // them as GlitchTip issues (see issues 5033/5034 for the analyzer
        // warm-up case).
        let is_expected_transient = matches!(
            self,
            WebError::AnalyzerError(AnalyzerError::Uninitialized)
                | WebError::TemporarilyUnavailable
        );

        let message = if status.is_server_error() && !is_expected_transient {
            "Internal server error".to_string()
        } else {
            format!("{self}")
        };

        // `error = %self` is a *field*, not the message, so it never affects
        // grouping — which is why the per-occurrence detail (ClickHouse's live
        // memory figures, the failing item id) can safely ride along here while
        // the title stays stable.
        let title = report_title(&self);
        if status.is_server_error() && !is_expected_transient {
            tracing::error!(error = %self, %status, "{title}");
        } else {
            tracing::debug!(error = %self, %status, "{title}");
        }
        (status, message).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unauthenticated request must answer `401`, not `200`.
    ///
    /// `AuthDiscordUser`'s extractor rejection is `ApiError::NoAuthCookie`, so
    /// this is the status every logged-out request to every authenticated API
    /// route gets. Answering `200` with an `{"ApiError":"NotAuthenticated"}`
    /// body makes an auth failure indistinguishable from success at the HTTP
    /// layer: the SSR fetch helper takes its `status.is_success()` branch, the
    /// structured error then looks like a *deserialization* failure, and it
    /// gets reported at error level (the GlitchTip 2218 / 2210 lineage that
    /// `ultros-app/src/api.rs` carries two separate workaround comments for).
    #[test]
    fn no_auth_cookie_is_unauthorized_not_ok() {
        let err = ApiError::NoAuthCookie;
        assert_eq!(err.as_status_code(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            err.into_response().status(),
            StatusCode::UNAUTHORIZED,
            "the response status must match as_status_code()"
        );
    }

    /// An expired/revoked Discord token is also an auth failure, so it gets the
    /// same `401`. This arm returns early in `into_response` to attach the
    /// cookie removal, and previously returned no status at all — which axum
    /// defaults to `200`.
    ///
    /// Only the status is asserted: the cookie-clearing behaviour is untouched
    /// by this change, and an empty test jar can't reproduce it anyway —
    /// `CookieJar::remove` only emits a removal `Set-Cookie` when the name is
    /// already in `original_cookies`, which in production it is (we only reach
    /// this variant when a `discord_auth` cookie was present but Discord
    /// rejected the token).
    #[test]
    fn discord_token_invalid_is_unauthorized() {
        let jar = PrivateCookieJar::new(Key::generate());
        let response = ApiError::DiscordTokenInvalid(jar).into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// The wire body is unchanged — clients match on `NotAuthenticated` (e.g.
    /// the list-invite login redirect in `ultros-app/src/routes/lists.rs`), so
    /// only the status moves.
    #[test]
    fn auth_failures_keep_their_structured_body() {
        assert_eq!(
            ApiError::NoAuthCookie.as_api_error(),
            ultros_api_types::result::ApiError::NotAuthenticated
        );
    }

    /// The typed ClickHouse title only survives if the error stays a
    /// [`WebError::ClickHouse`] all the way to `into_response`.
    ///
    /// Regression test for the 2026-08-23 outage: the item-card chart generator
    /// returned `anyhow::Result`, so `build_price_series`'s typed error was
    /// flattened into `AnyhowError` at the first `?`. Every item-card request
    /// during the outage reported as the generic "Returning web error" — the
    /// exact failure mode the `ClickHouse` variant was added to prevent.
    ///
    /// The second half of this test is what the *old* code did, kept so the
    /// hazard stays visible: any call site that routes a `WebError` through
    /// `anyhow` silently loses its grouping.
    #[test]
    fn clickhouse_errors_keep_their_title_unless_laundered_through_anyhow() {
        let typed: WebError = ClickHouseQueryError::new(
            "price_series",
            ultros_clickhouse::ClickHouseError::Client(clickhouse::error::Error::TimedOut),
        )
        .into();
        assert_eq!(
            report_title(&typed),
            "ClickHouse price_series query failed (timeout)"
        );

        let laundered: WebError = anyhow::Error::from(ClickHouseQueryError::new(
            "price_series",
            ultros_clickhouse::ClickHouseError::Client(clickhouse::error::Error::TimedOut),
        ))
        .into();
        assert_eq!(
            report_title(&laundered),
            "Returning web error",
            "an `anyhow` hop erases the grouping — call sites must return WebError"
        );
    }

    /// A ClickHouse failure is still a 500: only the reporting title changes,
    /// not what the client sees.
    #[test]
    fn clickhouse_failure_is_a_server_error() {
        let err: WebError = ClickHouseQueryError::new(
            "price_series",
            ultros_clickhouse::ClickHouseError::Client(clickhouse::error::Error::TimedOut),
        )
        .into();
        assert_eq!(err.as_status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// 401 is a *client* error, so it must not trip the `is_server_error()`
    /// paths that log at error level and replace the message with a generic
    /// "Internal server error". This is what preserves the original intent of
    /// the `NoAuthCookie => OK` mapping ("I don't want a real error") without
    /// lying about the status.
    #[test]
    fn auth_failure_is_not_reported_as_a_server_error() {
        assert!(!ApiError::NoAuthCookie.as_status_code().is_server_error());
    }
}
