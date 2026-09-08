use std::{
    num::ParseIntError,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

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
use tracing::{error, info, warn};
use ultros_api_types::result::JsonErrorWrapper;
use ultros_db::{
    SeaDbErr, common_type_conversions::ApiConversionError, group_roles::GroupError,
    lists::ListError, retainers::RetainerError, world_data::world_cache::WorldCacheError,
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

/// Why a call to Discord failed, as far as the response boundary needs to care.
///
/// Only two things depend on this: the status the caller gets, and whether an
/// operator is told. Both hinge on "will this clear on its own?", so that is
/// the whole distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscordFailure {
    /// Discord gave a definitive "no" that only a deploy change fixes — the
    /// Server Members intent left off, the bot removed from the guild. The
    /// caller did nothing wrong, so this is `502`, not `4xx`, and it is worth
    /// waking an operator (once — see [`report_discord_misconfiguration`]).
    Misconfigured,
    /// A rate limit, a Discord 5xx, or no answer at all. Retrying works, so
    /// it is a `503` and it stays out of the error tracker: member search runs
    /// on a per-keystroke debounce, and one Discord blip would otherwise file
    /// an issue per character typed.
    Transient,
}

/// The one-shot latch behind the `Misconfigured` arm of
/// [`reports_to_tracker`].
static DISCORD_MISCONFIGURATION_REPORTED: AtomicBool = AtomicBool::new(false);

/// Whether this error should be filed as an issue in the error tracker, as
/// opposed to merely logged.
///
/// `error!` is what the `sentry_tracing` layer captures, so this is the whole
/// decision about who gets woken up. Not every 5xx belongs there: a dependency
/// that is down or rate-limiting us is weather, and
/// `GET /group/{id}/member-search` runs on a 300ms per-keystroke debounce, so
/// one Discord 429 would otherwise file an issue per character typed.
///
/// `misconfiguration_latch` is a parameter rather than a direct read of the
/// static so this is testable: a process-wide `AtomicBool` cannot be reset
/// between tests that share a process.
fn reports_to_tracker(
    error: &ApiError,
    status: StatusCode,
    misconfiguration_latch: &AtomicBool,
) -> bool {
    if !status.is_server_error() {
        return false;
    }
    match error {
        // A disconnected Discord bot is a transient state, not a bug in this
        // process — the same carve-out `WebError` makes for analyzer warm-up.
        ApiError::ServiceUnavailable(_) => false,
        ApiError::Discord {
            kind: DiscordFailure::Transient,
            ..
        } => false,
        // A misconfiguration is real and an operator has to see it, but it is
        // identical on every request until someone changes the deploy. The
        // spec asks for one loud line per process; `group_sync::reconcile`
        // keeps its own latch for the same reason on the background path, and
        // the two are deliberately separate so silencing one does not silence
        // the other.
        ApiError::Discord {
            kind: DiscordFailure::Misconfigured,
            ..
        } => !misconfiguration_latch.swap(true, Ordering::Relaxed),
        _ => true,
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
    #[error("{0}")]
    BadRequest(&'static str),
    /// A dependency we don't control is down — in practice the Discord
    /// gateway. Distinct from the `anyhow` catch-all because that one is
    /// reported as a 500 *and* has its message replaced by "Internal server
    /// error", which is exactly what makes an offline bot undiagnosable from
    /// the UI.
    #[error("{0}")]
    ServiceUnavailable(&'static str),
    /// A call to Discord failed and the reason has to survive to the client.
    ///
    /// [`ServiceUnavailable`](ApiError::ServiceUnavailable) already does this
    /// for the bot being offline, but it carries a `&'static str` and these
    /// messages are built from Discord's own response — a status, a guild
    /// name, Discord's own words. Routing them through `anyhow` instead is
    /// what silently discarded them: `AnyhowError` has no arm in
    /// `as_api_error`, so the 403 that names the Server Members intent was
    /// constructed, formatted, and then replaced with "Internal server error".
    #[error("{message}")]
    Discord {
        message: String,
        kind: DiscordFailure,
    },
});

impl From<ultros_db::list_doc::ListDocError> for ApiError {
    fn from(error: ultros_db::list_doc::ListDocError) -> Self {
        use ultros_db::list_doc::ListDocError;
        use ultros_list_doc::DocError;
        match error {
            ListDocError::List(inner) => ApiError::from(anyhow::Error::from(inner)),
            ListDocError::MetaForbidden => ApiError::from(anyhow::Error::from(
                ListError::Forbidden("only the list owner can change its name or scope"),
            )),
            ListDocError::InvalidUpdate => ApiError::from(anyhow::Error::from(
                ListError::BadRequest("invalid document update"),
            )),
            ListDocError::MissingHistory => {
                ApiError::from(anyhow::Error::from(ListError::BadRequest(
                    "update depends on history this server does not have; resync from a snapshot",
                )))
            }
            // A row that isn't in the document (already removed, or a stale
            // client-side id) is a 404, not the 500 the `other` catch-all
            // below would otherwise give it.
            ListDocError::Doc(DocError::MissingRow(_)) => {
                ApiError::from(anyhow::Error::from(ListError::NotFound))
            }
            // Keep the typed `DbErr` on the `DbError` variant instead of
            // stringifying it through `anyhow`, so the `#[source]` chain and
            // GlitchTip grouping survive the same way every other DB failure
            // in this file does.
            ListDocError::Db(e) => ApiError::DbError(e),
            other => ApiError::from(anyhow::anyhow!("{other}")),
        }
    }
}

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
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            // A refusal from Discord is an upstream answering "no" to *us*:
            // 502 says the gateway between the caller and Discord is at
            // fault, which is exactly right for an intent nobody enabled.
            // A blip is a 503, which also says "retry".
            ApiError::Discord {
                kind: DiscordFailure::Misconfigured,
                ..
            } => StatusCode::BAD_GATEWAY,
            ApiError::Discord {
                kind: DiscordFailure::Transient,
                ..
            } => StatusCode::SERVICE_UNAVAILABLE,
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
            .or_else_status(e.downcast_ref::<RetainerError>())
            .or_else_group_status(e.downcast_ref::<GroupError>()),
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn as_api_error(&self) -> ultros_api_types::result::ApiError {
        match self {
            ApiError::NoAuthCookie => ultros_api_types::result::ApiError::NotAuthenticated,
            ApiError::Forbidden(_) => ultros_api_types::result::ApiError::Forbidden,
            ApiError::BadRequest(message) => {
                ultros_api_types::result::ApiError::BadRequest((*message).into())
            }
            // Explicit arm: 503 is a server error, so the fallback below would
            // swap this message for "Internal server error" and defeat the
            // point of the variant.
            ApiError::ServiceUnavailable(message) => {
                ultros_api_types::result::ApiError::Message((*message).to_string())
            }
            // Same reasoning, same trap: both Discord statuses are 5xx, so
            // without an explicit arm the fallback below would swap the
            // message for "Internal server error" and the endpoint would fail
            // exactly as opaquely as before this variant existed.
            ApiError::Discord { message, .. } => {
                ultros_api_types::result::ApiError::Message(message.clone())
            }
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
                    None => match e.downcast_ref::<GroupError>() {
                        Some(GroupError::Forbidden(_)) => {
                            ultros_api_types::result::ApiError::Forbidden
                        }
                        Some(GroupError::NotFound | GroupError::RoleNotFound) => {
                            ultros_api_types::result::ApiError::NotFound
                        }
                        Some(GroupError::BadRequest(msg)) => {
                            ultros_api_types::result::ApiError::BadRequest((*msg).into())
                        }
                        Some(GroupError::ManagedByDiscord) => {
                            ultros_api_types::result::ApiError::BadRequest(
                                "That member is managed by Discord; change their Discord role instead"
                                    .into(),
                            )
                        }
                        Some(GroupError::RoleManagedByDiscord) => {
                            ultros_api_types::result::ApiError::BadRequest(
                                "That role is managed by Discord; its members come from the Discord role"
                                    .into(),
                            )
                        }
                        None => ultros_api_types::result::ApiError::Message(
                            "Internal server error".to_string(),
                        ),
                    },
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

trait GroupStatus {
    fn or_else_group_status(self, group_error: Option<&GroupError>) -> StatusCode;
}

impl GroupStatus for StatusCode {
    fn or_else_group_status(self, group_error: Option<&GroupError>) -> StatusCode {
        if self != StatusCode::INTERNAL_SERVER_ERROR {
            return self;
        }
        match group_error {
            Some(GroupError::NotFound) | Some(GroupError::RoleNotFound) => StatusCode::NOT_FOUND,
            Some(GroupError::Forbidden(_)) => StatusCode::FORBIDDEN,
            Some(GroupError::BadRequest(_))
            | Some(GroupError::ManagedByDiscord)
            | Some(GroupError::RoleManagedByDiscord) => StatusCode::BAD_REQUEST,
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
        // Its own bucket for the same reason ClickHouse has one: a Discord
        // misconfiguration wants an alert of its own, not a share of the
        // "Generic API error" pile.
        ApiError::Discord { .. } => std::borrow::Cow::Borrowed("Discord API call failed"),
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
        let report_to_tracker =
            reports_to_tracker(&self, status, &DISCORD_MISCONFIGURATION_REPORTED);
        // Same grouping rule as `WebError` — see `report_title`. The API
        // routes are where the ClickHouse-backed endpoints live (item_stats,
        // movers, resale_quality, market_heat), so collapsing them all under
        // "Generic API error" is what made a ClickHouse outage
        // indistinguishable from any other 500.
        let title = api_report_title(&self);
        if report_to_tracker {
            error!(error = ?self, "{title}");
        } else if status.is_server_error() {
            warn!(error = ?self, %status, "{title}");
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

    #[test]
    fn bad_request_preserves_client_error_message() {
        let error = ApiError::BadRequest("unsupported push provider");
        assert_eq!(
            error.as_api_error(),
            ultros_api_types::result::ApiError::BadRequest("unsupported push provider".into())
        );
        assert_eq!(error.into_response().status(), StatusCode::BAD_REQUEST);
    }

    /// The group endpoints do their authorization in `ultros-db` and let the
    /// typed error travel up through `anyhow`. That only produces a usable API
    /// response if the downcast here still finds it, so this pins the statuses
    /// the role endpoints depend on: a non-owner gets 403, a missing role 404,
    /// and a change to a Discord-managed role or member gets a 400 that says
    /// why rather than a bare "bad request".
    #[test]
    fn group_errors_keep_their_status_and_message_through_anyhow() {
        let cases = [
            (
                GroupError::Forbidden("Only the group owner can manage roles"),
                StatusCode::FORBIDDEN,
            ),
            (GroupError::NotFound, StatusCode::NOT_FOUND),
            (GroupError::RoleNotFound, StatusCode::NOT_FOUND),
            (
                GroupError::BadRequest("That Discord role is already imported"),
                StatusCode::BAD_REQUEST,
            ),
            (GroupError::ManagedByDiscord, StatusCode::BAD_REQUEST),
            (GroupError::RoleManagedByDiscord, StatusCode::BAD_REQUEST),
        ];
        for (error, expected) in cases {
            let described = error.to_string();
            let api = ApiError::from(anyhow::Error::from(error));
            assert_eq!(api.as_status_code(), expected, "{described}");
        }
    }

    /// Both "managed by Discord" refusals have to reach the client as text a
    /// person can act on — the whole reason the endpoints refuse instead of
    /// making a change reconciliation would undo.
    #[test]
    fn discord_managed_refusals_explain_themselves() {
        for error in [
            GroupError::ManagedByDiscord,
            GroupError::RoleManagedByDiscord,
        ] {
            let api = ApiError::from(anyhow::Error::from(error));
            let ultros_api_types::result::ApiError::BadRequest(message) = api.as_api_error() else {
                panic!("expected a BadRequest body");
            };
            assert!(
                message.contains("managed by Discord"),
                "unexpected message: {message}"
            );
        }
    }

    /// An offline Discord bot is not a bug in this process. It answers 503 and
    /// keeps its message: the `anyhow` catch-all would have made it a 500 with
    /// the text replaced by "Internal server error", which is exactly what
    /// makes a disconnected bot undiagnosable from the UI.
    #[test]
    fn service_unavailable_keeps_its_message() {
        let error = ApiError::ServiceUnavailable("The Ultros Discord bot is not connected");
        assert_eq!(error.as_status_code(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            error.as_api_error(),
            ultros_api_types::result::ApiError::Message(
                "The Ultros Discord bot is not connected".to_string()
            )
        );
    }

    /// The Discord messages the spec spells out have to survive all the way to
    /// the wire, which is the half `member_search_message`'s own unit test
    /// could not see: the message was built correctly and then thrown away by
    /// the `anyhow` catch-all below.
    #[tokio::test]
    async fn discord_failures_keep_their_message_and_a_sensible_status() {
        let cases = [
            (
                DiscordFailure::Misconfigured,
                StatusCode::BAD_GATEWAY,
                "Discord refused the member search (403). The Ultros bot needs the \
                 Server Members intent enabled in the Discord developer portal.",
            ),
            (
                DiscordFailure::Transient,
                StatusCode::SERVICE_UNAVAILABLE,
                "Discord member search failed: connection reset",
            ),
        ];
        for (kind, expected_status, message) in cases {
            let error = ApiError::Discord {
                message: message.to_string(),
                kind,
            };
            assert_eq!(error.as_status_code(), expected_status, "{kind:?}");
            assert_eq!(
                error.as_api_error(),
                ultros_api_types::result::ApiError::Message(message.to_string()),
                "{kind:?}"
            );

            let response = error.into_response();
            assert_eq!(response.status(), expected_status, "{kind:?}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body = String::from_utf8(body.to_vec()).unwrap();
            assert!(body.contains(message), "{kind:?}: {body}");
            assert!(!body.contains("Internal server error"), "{kind:?}: {body}");
        }
    }

    /// A transient Discord failure never reaches the error tracker.
    ///
    /// `member-search` fires on a 300ms per-keystroke debounce, so one Discord
    /// 429 or blip would otherwise file an issue per character typed — a pile
    /// of reports nobody can act on, burying the ones somebody can.
    #[test]
    fn a_transient_discord_failure_never_reaches_the_error_tracker() {
        let error = ApiError::Discord {
            message: "Discord member search failed: 429 Too Many Requests".to_string(),
            kind: DiscordFailure::Transient,
        };
        let status = error.as_status_code();
        assert!(
            status.is_server_error(),
            "still a 5xx — it just isn't this process's fault"
        );
        for attempt in 0..5 {
            assert!(
                !reports_to_tracker(&error, status, &AtomicBool::new(false)),
                "attempt {attempt} filed an issue for a retryable Discord failure"
            );
        }
    }

    /// A misconfigured deploy is a real problem, so it is reported — but it is
    /// identical on every request until somebody fixes it, so it is reported
    /// exactly once per process, matching what `group_sync::reconcile` already
    /// does on the background path.
    #[test]
    fn a_misconfiguration_is_reported_once_and_only_once() {
        let error = ApiError::Discord {
            message: "Discord refused the member search (403). The Ultros bot needs the \
                      Server Members intent enabled in the Discord developer portal."
                .to_string(),
            kind: DiscordFailure::Misconfigured,
        };
        let status = error.as_status_code();
        let latch = AtomicBool::new(false);
        assert!(
            reports_to_tracker(&error, status, &latch),
            "an operator has to hear about a misconfigured intent"
        );
        for attempt in 1..5 {
            assert!(
                !reports_to_tracker(&error, status, &latch),
                "attempt {attempt} repeated a report that says nothing new"
            );
        }
    }

    /// The carve-outs are narrow: an ordinary 500 still gets reported, or this
    /// change would have quietly blinded the tracker.
    #[test]
    fn an_ordinary_server_error_is_still_reported() {
        let error = ApiError::from(anyhow::anyhow!("something actually broke"));
        assert!(reports_to_tracker(
            &error,
            error.as_status_code(),
            &AtomicBool::new(false)
        ));
    }

    /// Discord failures get their own reporting bucket rather than sharing the
    /// "Generic API error" pile, so an unenabled intent can be alerted on.
    #[test]
    fn discord_failures_group_under_their_own_title() {
        assert_eq!(
            api_report_title(&ApiError::Discord {
                message: "whatever".to_string(),
                kind: DiscordFailure::Misconfigured,
            }),
            "Discord API call failed"
        );
    }

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
