//! Typed `BotAdminClient` boundary between `ultros-web` and `ultros-market`
//! for Discord-bot-backed web admin operations.
//!
//! Two implementations share one trait:
//! - [`InProcessBotAdminClient`]: direct in-process calls when `ULTROS_ROLES`
//!   includes `bot` (including the default `ULTROS_ROLES=all`), with zero
//!   extra network hop or serialization overhead.
//! - [`HttpBotAdminClient`]: multiplexed HTTP/2 client targeting
//!   `ULTROS_MARKET_URL` (`/internal/bot/*`, authenticated with
//!   `ULTROS_MARKET_TOKEN`) when running stateless `ULTROS_ROLES=web` replicas.
//!
//! Security boundary:
//! - User Discord OAuth tokens (`discord_auth` cookie) NEVER cross this
//!   boundary; `ultros-web` queries Discord `GET /users/@me/guilds` locally and
//!   only passes intersected candidate guild IDs to `ultros-market`.
//! - Discord notification test/resend calls pass `(endpoint_id, owner_id)` and
//!   never arbitrary channel or user IDs; `ultros-market` independently loads
//!   and verifies endpoint ownership and Discord delivery method in Postgres.

use std::time::Duration;

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use tracing::{error, warn};
use ultros_api_types::{alert::DiscordWritableGuild, user::GroupSyncStatus};
use ultros_db::UltrosDb;
use url::Url;

use crate::web::{
    api::discord_lookup,
    error::{ApiError, DiscordFailure},
};

/// Shared user-facing message when the Discord gateway is offline, so every
/// bot-backed group/channel admin route fails uniformly as a retryable `503`
/// without polluting the error tracker.
pub(crate) const DISCORD_BOT_OFFLINE: &str =
    "The Ultros Discord bot is not connected right now; try again in a moment";

/// Maximum candidate guild IDs accepted in one guild-discovery RPC. Discord
/// caps user guild memberships well below this limit (100, or 200 with Nitro).
pub(crate) const MAX_CANDIDATE_GUILDS: usize = 500;

/// Maximum Discord notification title / body byte lengths accepted on the
/// internal delivery proxy (aligned with Discord embed constraints).
pub(crate) const MAX_DELIVERY_TITLE_BYTES: usize = 512;
pub(crate) const MAX_DELIVERY_BODY_BYTES: usize = 4096;

/// Default RPC timeout for bot-admin calls. Guild/channel discovery can issue
/// several sequential Discord REST calls (including rate-limit waits), so this
/// is slightly higher than market-read timeouts while remaining strictly bounded.
const BOT_RPC_TIMEOUT: Duration = Duration::from_secs(30);
const FAST_BOT_RPC_TIMEOUT: Duration = Duration::from_secs(15);

/// Compact guild metadata returned by bot-side guild discovery or admin checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BotManageableGuild {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) icon_url: Option<String>,
}

/// Resolved Discord guild channel after bot-side admin verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BotResolvedChannel {
    pub(crate) channel_name: String,
    pub(crate) guild_id: i64,
    pub(crate) guild_name: String,
}

/// Importable Discord guild role metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BotGuildRole {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) position: i32,
    pub(crate) color: Option<String>,
}

/// Discord member candidate returned by guild member prefix search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BotGuildMember {
    pub(crate) user_id: i64,
    /// Global display name (never per-guild nickname; see
    /// `group_sync::global_display_name`).
    pub(crate) display_name: String,
    pub(crate) avatar_url: String,
}

/// Failure modes for internal Discord notification delivery (`test_endpoint` /
/// `resend_alert_event`), preserving exact offline vs. delivery-failure wording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BotDeliveryError {
    BotOffline,
    Failed(String),
}

/// Structured wire error returned by `/internal/bot/*` endpoints on
/// `ultros-market` so `ultros-web` reconstructs exact `ApiError` status codes
/// and error-tracker policies (`503` offline/transient without Sentry spam,
/// `502` misconfiguration with single-latch Sentry alerting, `500` internal).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum BotAdminError {
    BotOffline,
    DiscordMisconfigured { message: String },
    DiscordTransient { message: String },
    Internal { message: String },
}

impl BotAdminError {
    pub(crate) fn from_api_error(err: ApiError) -> Self {
        match err {
            ApiError::ServiceUnavailable(_) => Self::BotOffline,
            ApiError::Discord {
                message,
                kind: DiscordFailure::Misconfigured,
            } => Self::DiscordMisconfigured { message },
            ApiError::Discord {
                message,
                kind: DiscordFailure::Transient,
            } => Self::DiscordTransient { message },
            other => Self::Internal {
                message: other.to_string(),
            },
        }
    }

    pub(crate) fn into_api_error(self) -> ApiError {
        match self {
            Self::BotOffline => ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE),
            Self::DiscordMisconfigured { message } => ApiError::Discord {
                message,
                kind: DiscordFailure::Misconfigured,
            },
            Self::DiscordTransient { message } => ApiError::Discord {
                message,
                kind: DiscordFailure::Transient,
            },
            Self::Internal { message } => ApiError::from(anyhow::anyhow!("{message}")),
        }
    }

    pub(crate) fn status_code(&self) -> StatusCode {
        match self {
            Self::BotOffline | Self::DiscordTransient { .. } => StatusCode::SERVICE_UNAVAILABLE,
            Self::DiscordMisconfigured { .. } => StatusCode::BAD_GATEWAY,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for BotAdminError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        if status.is_server_error() && !matches!(self, Self::BotOffline) {
            warn!(%status, error = ?self, "internal bot-admin request failed");
        }
        let mut response = (status, Json(self)).into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}

/// Request payload for `POST /internal/bot/manageable-guilds` and
/// `POST /internal/bot/writable-guilds`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GuildDiscoveryRequest {
    pub(crate) user_id: i64,
    /// `Some(ids)` when `ultros-web` already intersected the user's OAuth
    /// `/users/@me/guilds` response with `bot_guild_ids()`; `None` when the
    /// OAuth grant lacks the `guilds` scope (403) and requires legacy bot-wide
    /// permission probing.
    pub(crate) candidate_guild_ids: Option<Vec<i64>>,
}

/// Request payload for `POST /internal/bot/require-manageable-guild`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RequireManageableGuildRequest {
    pub(crate) guild_id: i64,
    pub(crate) user_id: i64,
}

/// Request payload for `POST /internal/bot/resolve-channel`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResolveChannelRequest {
    pub(crate) channel_id: i64,
    pub(crate) user_id: i64,
}

/// Query parameters for `GET /internal/bot/guilds/{guild_id}/members/search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SearchGuildMembersQuery {
    pub(crate) q: String,
    pub(crate) limit: u64,
}

/// Request payload for `POST /internal/bot/guilds/{guild_id}/sync`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TriggerGroupSyncRequest {
    pub(crate) enforce_rate_limit: bool,
}

/// Response payload for `POST /internal/bot/guilds/{guild_id}/sync`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TriggerGroupSyncResponse {
    pub(crate) status: GroupSyncStatus,
}

/// Request payload for `POST /internal/bot/deliver-notification`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DeliverDiscordNotificationRequest {
    pub(crate) endpoint_id: i32,
    pub(crate) owner_id: i64,
    pub(crate) title: String,
    pub(crate) body: String,
}

/// Typed boundary for Discord-bot-backed web admin operations owned by the
/// `bot` role.
pub(crate) trait BotAdminClient: Send + Sync {
    /// Guild IDs currently known to the live Discord bot cache.
    fn bot_guild_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<i64>, ApiError>>;

    /// Guilds the user may turn into an Ultros group.
    fn manageable_guilds<'a>(
        &'a self,
        user_id: i64,
        candidate_guild_ids: Option<Vec<i64>>,
    ) -> BoxFuture<'a, Result<Vec<BotManageableGuild>, ApiError>>;

    /// Guilds the user manages along with text/news channels the bot can post
    /// embeds into.
    fn writable_guilds<'a>(
        &'a self,
        user_id: i64,
        candidate_guild_ids: Option<Vec<i64>>,
    ) -> BoxFuture<'a, Result<Vec<DiscordWritableGuild>, ApiError>>;

    /// Verify the user has `ADMINISTRATOR` or `MANAGE_GUILD` in `guild_id` and
    /// return live guild metadata.
    fn require_manageable_guild<'a>(
        &'a self,
        guild_id: i64,
        user_id: i64,
    ) -> BoxFuture<'a, Result<BotManageableGuild, ApiError>>;

    /// Resolve a Discord guild channel and verify `user_id` is an admin of its
    /// owning guild in one atomic bot-side call.
    fn resolve_and_verify_channel<'a>(
        &'a self,
        channel_id: i64,
        user_id: i64,
    ) -> BoxFuture<'a, Result<BotResolvedChannel, ApiError>>;

    /// Importable (non-managed) guild roles ordered by Discord display order.
    fn importable_guild_roles<'a>(
        &'a self,
        guild_id: i64,
    ) -> BoxFuture<'a, Result<Vec<BotGuildRole>, ApiError>>;

    /// Prefix-search guild members via Discord's member search endpoint.
    fn search_guild_members<'a>(
        &'a self,
        guild_id: i64,
        query: &'a str,
        limit: u64,
    ) -> BoxFuture<'a, Result<Vec<BotGuildMember>, ApiError>>;

    /// Trigger background Discord group membership reconciliation on the bot
    /// process, enforcing or recording against the bot's global rate limiter.
    fn trigger_group_sync<'a>(
        &'a self,
        guild_id: i64,
        enforce_rate_limit: bool,
    ) -> BoxFuture<'a, Result<GroupSyncStatus, ApiError>>;

    /// Deliver a test or resend notification to an existing Discord endpoint
    /// (`DiscordDm` or `DiscordChannel`) owned by `owner_id`.
    fn deliver_discord_notification<'a>(
        &'a self,
        endpoint_id: i32,
        owner_id: i64,
        title: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Result<(), BotDeliveryError>>;
}

/// Direct in-process [`BotAdminClient`] used whenever the `bot` role is active
/// in this binary (including `ULTROS_ROLES=all` and on `ultros-market` behind
/// `/internal/bot/*`), or as a graceful offline fallback when no remote bot URL
/// is configured.
#[derive(Clone)]
pub(crate) struct InProcessBotAdminClient {
    db: UltrosDb,
}

impl InProcessBotAdminClient {
    pub(crate) fn new(db: UltrosDb) -> Self {
        Self { db }
    }
}

impl BotAdminClient for InProcessBotAdminClient {
    fn bot_guild_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<i64>, ApiError>> {
        Box::pin(async move {
            let ctx = crate::alerts::delivery::get_serenity_ctx()
                .ok_or(ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE))?;
            let mut ids: Vec<i64> = ctx
                .cache
                .guilds()
                .into_iter()
                .filter_map(|id| i64::try_from(id.get()).ok())
                .collect();
            ids.sort_unstable();
            Ok(ids)
        })
    }

    fn manageable_guilds<'a>(
        &'a self,
        user_id: i64,
        candidate_guild_ids: Option<Vec<i64>>,
    ) -> BoxFuture<'a, Result<Vec<BotManageableGuild>, ApiError>> {
        Box::pin(async move {
            if let Some(ref ids) = candidate_guild_ids
                && ids.len() > MAX_CANDIDATE_GUILDS
            {
                return Err(ApiError::from(anyhow::anyhow!(
                    "too many candidate guild ids"
                )));
            }
            let ctx = crate::alerts::delivery::get_serenity_ctx()
                .ok_or(ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE))?;
            let guilds = discord_lookup::manageable_guilds_from_candidates(
                &ctx,
                user_id,
                candidate_guild_ids.as_deref(),
            )
            .await?;
            Ok(guilds
                .into_iter()
                .map(|(id, name, icon_url)| BotManageableGuild { id, name, icon_url })
                .collect())
        })
    }

    fn writable_guilds<'a>(
        &'a self,
        user_id: i64,
        candidate_guild_ids: Option<Vec<i64>>,
    ) -> BoxFuture<'a, Result<Vec<DiscordWritableGuild>, ApiError>> {
        Box::pin(async move {
            if let Some(ref ids) = candidate_guild_ids
                && ids.len() > MAX_CANDIDATE_GUILDS
            {
                return Err(ApiError::from(anyhow::anyhow!(
                    "too many candidate guild ids"
                )));
            }
            let ctx = crate::alerts::delivery::get_serenity_ctx()
                .ok_or(ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE))?;
            discord_lookup::writable_guilds_from_candidates(
                &ctx,
                user_id,
                candidate_guild_ids.as_deref(),
            )
            .await
        })
    }

    fn require_manageable_guild<'a>(
        &'a self,
        guild_id: i64,
        user_id: i64,
    ) -> BoxFuture<'a, Result<BotManageableGuild, ApiError>> {
        Box::pin(async move {
            let ctx = crate::alerts::delivery::get_serenity_ctx()
                .ok_or(ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE))?;
            let guild = discord_lookup::require_manageable_guild(&ctx, guild_id, user_id).await?;
            Ok(BotManageableGuild {
                id: guild.id.get() as i64,
                name: guild.name.clone(),
                icon_url: guild.icon_url(),
            })
        })
    }

    fn resolve_and_verify_channel<'a>(
        &'a self,
        channel_id: i64,
        user_id: i64,
    ) -> BoxFuture<'a, Result<BotResolvedChannel, ApiError>> {
        Box::pin(async move {
            let ctx = crate::alerts::delivery::get_serenity_ctx()
                .ok_or(ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE))?;
            let resolved = discord_lookup::resolve_channel(&ctx, channel_id).await?;
            discord_lookup::require_user_is_guild_admin(&ctx, resolved.guild_id, user_id).await?;
            Ok(BotResolvedChannel {
                channel_name: resolved.channel_name,
                guild_id: resolved.guild_id,
                guild_name: resolved.guild_name,
            })
        })
    }

    fn importable_guild_roles<'a>(
        &'a self,
        guild_id: i64,
    ) -> BoxFuture<'a, Result<Vec<BotGuildRole>, ApiError>> {
        Box::pin(async move {
            let ctx = crate::alerts::delivery::get_serenity_ctx()
                .ok_or(ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE))?;
            let roles = discord_lookup::importable_guild_roles(&ctx, guild_id).await?;
            Ok(roles
                .into_iter()
                .map(|(id, name, position, color)| BotGuildRole {
                    id,
                    name,
                    position,
                    color,
                })
                .collect())
        })
    }

    fn search_guild_members<'a>(
        &'a self,
        guild_id: i64,
        query: &'a str,
        limit: u64,
    ) -> BoxFuture<'a, Result<Vec<BotGuildMember>, ApiError>> {
        Box::pin(async move {
            let trimmed = query.trim();
            if trimmed.is_empty() {
                return Ok(Vec::new());
            }
            let clamped_limit = limit.clamp(1, 1000);
            let ctx = crate::alerts::delivery::get_serenity_ctx()
                .ok_or(ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE))?;
            let members =
                discord_lookup::search_guild_members(&ctx, guild_id, trimmed, clamped_limit)
                    .await?;
            Ok(members
                .iter()
                .map(|member| BotGuildMember {
                    user_id: member.user.id.get() as i64,
                    display_name: crate::group_sync::global_display_name(&member.user),
                    avatar_url: member.face(),
                })
                .collect())
        })
    }

    fn trigger_group_sync<'a>(
        &'a self,
        guild_id: i64,
        enforce_rate_limit: bool,
    ) -> BoxFuture<'a, Result<GroupSyncStatus, ApiError>> {
        Box::pin(async move {
            let _ctx = crate::alerts::delivery::get_serenity_ctx()
                .ok_or(ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE))?;
            let now = std::time::Instant::now();
            if enforce_rate_limit {
                if !crate::group_sync::sync_rate_limiter().try_acquire(guild_id, now) {
                    return Ok(GroupSyncStatus::RecentlySynced);
                }
            } else {
                crate::group_sync::sync_rate_limiter().record(guild_id, now);
            }
            crate::group_sync::spawn_reconcile(self.db.clone(), guild_id);
            Ok(GroupSyncStatus::Ran)
        })
    }

    fn deliver_discord_notification<'a>(
        &'a self,
        endpoint_id: i32,
        owner_id: i64,
        title: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Result<(), BotDeliveryError>> {
        Box::pin(async move {
            if title.len() > MAX_DELIVERY_TITLE_BYTES || body.len() > MAX_DELIVERY_BODY_BYTES {
                return Err(BotDeliveryError::Failed(
                    "notification payload exceeds maximum allowed size".into(),
                ));
            }
            // Verify ownership and Discord delivery method in Postgres before
            // touching Serenity so this endpoint can never be used to deliver
            // to foreign endpoints or non-Discord methods.
            let endpoint = self
                .db
                .get_endpoint_owned_by(owner_id, endpoint_id)
                .await
                .map_err(|e| BotDeliveryError::Failed(format!("{e}")))?;
            if !matches!(endpoint.method.as_str(), "DiscordDm" | "DiscordChannel") {
                return Err(BotDeliveryError::Failed(
                    "endpoint is not a Discord delivery method".into(),
                ));
            }
            let ctx =
                crate::alerts::delivery::get_serenity_ctx().ok_or(BotDeliveryError::BotOffline)?;
            let push = crate::alerts::delivery::PushOptions::immediate("/alerts");
            crate::alerts::delivery::deliver_to_endpoint(
                &endpoint, title, body, &push, &self.db, &ctx,
            )
            .await
            .map_err(|e| BotDeliveryError::Failed(format!("{e}")))
        })
    }
}

/// HTTP/2 [`BotAdminClient`] used by stateless `ULTROS_ROLES=web` replicas to
/// invoke `/internal/bot/*` on `ultros-market`.
#[derive(Clone)]
pub(crate) struct HttpBotAdminClient {
    base_url: Url,
    client: reqwest::Client,
    bearer_auth: Option<HeaderValue>,
}

impl HttpBotAdminClient {
    pub(crate) fn new(base_url: Url, bearer_token: Option<&str>) -> anyhow::Result<Self> {
        let mut builder = reqwest::Client::builder()
            // Private internal RPC must never route through ambient proxy env
            // vars (which break h2c and could leak ULTROS_MARKET_TOKEN).
            .no_proxy()
            .connect_timeout(Duration::from_secs(3))
            .timeout(BOT_RPC_TIMEOUT)
            .pool_idle_timeout(Duration::from_secs(90))
            .http2_keep_alive_interval(Some(Duration::from_secs(20)))
            .http2_keep_alive_timeout(Duration::from_secs(10))
            .http2_keep_alive_while_idle(true)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(crate::UNIVERSALIS_USER_AGENT);
        if base_url.scheme() == "http" {
            builder = builder.http2_prior_knowledge();
        }
        let client = builder.build()?;
        let bearer_auth =
            match bearer_token.map(str::trim).filter(|s| !s.is_empty()) {
                Some(token) => Some(HeaderValue::from_str(&format!("Bearer {token}")).map_err(
                    |e| anyhow::anyhow!("invalid ULTROS_MARKET_TOKEN header value: {e}"),
                )?),
                None => None,
            };
        Ok(Self {
            base_url,
            client,
            bearer_auth,
        })
    }

    fn endpoint_url(&self, segments: &[&str], query: &[(&str, String)]) -> Result<Url, ApiError> {
        let mut url = self.base_url.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("ULTROS_MARKET_URL cannot be a base"))?;
            path.pop_if_empty();
            for segment in segments {
                path.push(segment);
            }
        }
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        Ok(url)
    }

    fn with_bearer(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.bearer_auth {
            Some(auth) => req.header(header::AUTHORIZATION, auth.clone()),
            None => req,
        }
    }

    async fn decode_error(resp: reqwest::Response) -> ApiError {
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED {
            return ApiError::from(anyhow::anyhow!(
                "ultros-market rejected internal bot-admin credentials (401); check ULTROS_MARKET_TOKEN"
            ));
        }
        let body = resp.bytes().await.unwrap_or_default();
        if let Ok(bot_err) = serde_json::from_slice::<BotAdminError>(&body) {
            return bot_err.into_api_error();
        }
        if matches!(
            status,
            StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT
        ) {
            warn!(
                %status,
                "ultros-market returned transient status without structured BotAdminError"
            );
            return ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE);
        }
        warn!(%status, "ultros-market bot-admin RPC failed unexpectedly");
        ApiError::from(anyhow::anyhow!(
            "ultros-market bot-admin request failed with HTTP {status}"
        ))
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
        timeout: Duration,
    ) -> Result<T, ApiError> {
        let url = self.endpoint_url(segments, query)?;
        let resp = self
            .with_bearer(self.client.get(url).timeout(timeout))
            .send()
            .await
            .map_err(|err| {
                warn!(?err, "ultros-market bot-admin GET request failed");
                ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE)
            })?;
        if !resp.status().is_success() {
            return Err(Self::decode_error(resp).await);
        }
        resp.json::<T>().await.map_err(|err| {
            ApiError::from(anyhow::anyhow!(
                "failed to decode ultros-market bot-admin response: {err}"
            ))
        })
    }

    async fn post_json<Req: Serialize, Resp: for<'de> Deserialize<'de>>(
        &self,
        segments: &[&str],
        payload: &Req,
        timeout: Duration,
    ) -> Result<Resp, ApiError> {
        let url = self.endpoint_url(segments, &[])?;
        let resp = self
            .with_bearer(self.client.post(url).timeout(timeout).json(payload))
            .send()
            .await
            .map_err(|err| {
                warn!(?err, "ultros-market bot-admin POST request failed");
                ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE)
            })?;
        if !resp.status().is_success() {
            return Err(Self::decode_error(resp).await);
        }
        resp.json::<Resp>().await.map_err(|err| {
            ApiError::from(anyhow::anyhow!(
                "failed to decode ultros-market bot-admin response: {err}"
            ))
        })
    }
}

impl BotAdminClient for HttpBotAdminClient {
    fn bot_guild_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<i64>, ApiError>> {
        Box::pin(async move {
            self.get_json(&["internal", "bot", "guilds"], &[], FAST_BOT_RPC_TIMEOUT)
                .await
        })
    }

    fn manageable_guilds<'a>(
        &'a self,
        user_id: i64,
        candidate_guild_ids: Option<Vec<i64>>,
    ) -> BoxFuture<'a, Result<Vec<BotManageableGuild>, ApiError>> {
        Box::pin(async move {
            let payload = GuildDiscoveryRequest {
                user_id,
                candidate_guild_ids,
            };
            self.post_json(
                &["internal", "bot", "manageable-guilds"],
                &payload,
                BOT_RPC_TIMEOUT,
            )
            .await
        })
    }

    fn writable_guilds<'a>(
        &'a self,
        user_id: i64,
        candidate_guild_ids: Option<Vec<i64>>,
    ) -> BoxFuture<'a, Result<Vec<DiscordWritableGuild>, ApiError>> {
        Box::pin(async move {
            let payload = GuildDiscoveryRequest {
                user_id,
                candidate_guild_ids,
            };
            self.post_json(
                &["internal", "bot", "writable-guilds"],
                &payload,
                BOT_RPC_TIMEOUT,
            )
            .await
        })
    }

    fn require_manageable_guild<'a>(
        &'a self,
        guild_id: i64,
        user_id: i64,
    ) -> BoxFuture<'a, Result<BotManageableGuild, ApiError>> {
        Box::pin(async move {
            let payload = RequireManageableGuildRequest { guild_id, user_id };
            self.post_json(
                &["internal", "bot", "require-manageable-guild"],
                &payload,
                FAST_BOT_RPC_TIMEOUT,
            )
            .await
        })
    }

    fn resolve_and_verify_channel<'a>(
        &'a self,
        channel_id: i64,
        user_id: i64,
    ) -> BoxFuture<'a, Result<BotResolvedChannel, ApiError>> {
        Box::pin(async move {
            let payload = ResolveChannelRequest {
                channel_id,
                user_id,
            };
            self.post_json(
                &["internal", "bot", "resolve-channel"],
                &payload,
                FAST_BOT_RPC_TIMEOUT,
            )
            .await
        })
    }

    fn importable_guild_roles<'a>(
        &'a self,
        guild_id: i64,
    ) -> BoxFuture<'a, Result<Vec<BotGuildRole>, ApiError>> {
        Box::pin(async move {
            let guild_seg = guild_id.to_string();
            self.get_json(
                &["internal", "bot", "guilds", &guild_seg, "roles"],
                &[],
                FAST_BOT_RPC_TIMEOUT,
            )
            .await
        })
    }

    fn search_guild_members<'a>(
        &'a self,
        guild_id: i64,
        query: &'a str,
        limit: u64,
    ) -> BoxFuture<'a, Result<Vec<BotGuildMember>, ApiError>> {
        Box::pin(async move {
            let trimmed = query.trim();
            if trimmed.is_empty() {
                return Ok(Vec::new());
            }
            let guild_seg = guild_id.to_string();
            let params = [("q", trimmed.to_string()), ("limit", limit.to_string())];
            self.get_json(
                &["internal", "bot", "guilds", &guild_seg, "members", "search"],
                &params,
                FAST_BOT_RPC_TIMEOUT,
            )
            .await
        })
    }

    fn trigger_group_sync<'a>(
        &'a self,
        guild_id: i64,
        enforce_rate_limit: bool,
    ) -> BoxFuture<'a, Result<GroupSyncStatus, ApiError>> {
        Box::pin(async move {
            let guild_seg = guild_id.to_string();
            let payload = TriggerGroupSyncRequest { enforce_rate_limit };
            let resp: TriggerGroupSyncResponse = self
                .post_json(
                    &["internal", "bot", "guilds", &guild_seg, "sync"],
                    &payload,
                    FAST_BOT_RPC_TIMEOUT,
                )
                .await?;
            Ok(resp.status)
        })
    }

    fn deliver_discord_notification<'a>(
        &'a self,
        endpoint_id: i32,
        owner_id: i64,
        title: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Result<(), BotDeliveryError>> {
        Box::pin(async move {
            let url = self
                .endpoint_url(&["internal", "bot", "deliver-notification"], &[])
                .map_err(|e| BotDeliveryError::Failed(e.to_string()))?;
            let payload = DeliverDiscordNotificationRequest {
                endpoint_id,
                owner_id,
                title: title.to_string(),
                body: body.to_string(),
            };
            let resp = self
                .with_bearer(
                    self.client
                        .post(url)
                        .timeout(FAST_BOT_RPC_TIMEOUT)
                        .json(&payload),
                )
                .send()
                .await
                .map_err(|err| {
                    warn!(
                        ?err,
                        "ultros-market Discord notification delivery RPC failed"
                    );
                    BotDeliveryError::Failed(
                        "Discord bot service is temporarily unreachable; try again in a moment"
                            .into(),
                    )
                })?;
            let status = resp.status();
            if status.is_success() {
                return Ok(());
            }
            if status == StatusCode::UNAUTHORIZED {
                error!(
                    "ultros-market rejected internal bot-admin credentials (401) during Discord notification delivery; check ULTROS_MARKET_TOKEN"
                );
                return Err(BotDeliveryError::Failed(
                    "Discord delivery proxy authentication failed".into(),
                ));
            }
            let body_bytes = resp.bytes().await.unwrap_or_default();
            if let Ok(bot_err) = serde_json::from_slice::<BotAdminError>(&body_bytes) {
                return match bot_err {
                    BotAdminError::BotOffline => Err(BotDeliveryError::BotOffline),
                    BotAdminError::DiscordMisconfigured { message }
                    | BotAdminError::DiscordTransient { message }
                    | BotAdminError::Internal { message } => Err(BotDeliveryError::Failed(message)),
                };
            }
            Err(BotDeliveryError::Failed(format!(
                "Discord bot service returned HTTP {status}"
            )))
        })
    }
}
