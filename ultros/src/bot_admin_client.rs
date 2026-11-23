//! Typed `BotAdminClient` boundary between `ultros-web` and `ultros-market`
//! for Discord-bot-backed web admin operations.
//!
//! Two implementations share one trait:
//! - [`InProcessBotAdminClient`]: direct in-process calls when `ULTROS_ROLES`
//!   includes `bot` (including the default `ULTROS_ROLES=all`), with zero
//!   extra network hop or serialization overhead.
//! - [`HttpBotAdminClient`]: multiplexed HTTP/2 client targeting
//!   `ULTROS_MARKET_URL` (`/internal/bot/*`, authenticated with a required
//!   non-blank `ULTROS_MARKET_TOKEN`) when running stateless `ULTROS_ROLES=web`
//!   replicas.
//!
//! Security boundary:
//! - `/internal/bot/*` ALWAYS fails closed (`401 Unauthorized`) unless a
//!   non-blank `ULTROS_MARKET_TOKEN` is configured on both ends.
//! - User Discord OAuth tokens (`discord_auth` cookie) NEVER cross this
//!   boundary; `ultros-web` queries Discord `GET /users/@me/guilds` locally and
//!   only passes intersected candidate guild IDs to `ultros-market`.
//! - Group sync passes `(group_id, owner_id, GroupSyncTrigger)` and never
//!   arbitrary guild IDs or caller-controlled rate-limit bypasses; `ultros-market`
//!   verifies group ownership, linked non-frozen guild state, and post-import
//!   eligibility directly in Postgres.
//! - Discord notification test/resend calls pass `(endpoint_id, owner_id)` and
//!   never arbitrary channel or user IDs; `ultros-market` independently loads
//!   and verifies endpoint ownership and Discord delivery method in Postgres.

use std::time::{Duration, Instant};

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};
use ultros_api_types::{alert::DiscordWritableGuild, user::GroupSyncStatus};
use ultros_db::{
    UltrosDb,
    entity::{notification_endpoint, user_group},
    group_roles::{GroupError, PostImportSyncClaim},
};
use url::Url;

use crate::web::{
    api::discord_lookup::{self, parse_positive_discord_id},
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

/// Server-controlled burst window for [`GroupSyncTrigger::PostRoleImport`] so
/// repeated post-import triggers cannot spam full-guild reconciles.
const POST_IMPORT_BURST_COOLDOWN: Duration = Duration::from_secs(15);

/// Default RPC timeouts for bot-admin calls. Guild/channel discovery can issue
/// several sequential Discord REST calls (including bounded legacy scans up to
/// 15s), so this stays strictly above the legacy scan budget while remaining
/// bounded.
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

/// Why a Discord group membership sync was requested.
///
/// Server-side rate-limit policy is strictly controlled by `ultros-market`:
/// `Manual` enforces the full 5-minute per-guild cooldown, while
/// `PostRoleImport` only permits a bounded immediate reconcile when Postgres
/// confirms the group actually has an unsynced imported Discord role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GroupSyncTrigger {
    Manual,
    PostRoleImport,
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
/// and error-tracker policies (`400` bad request, `403` forbidden, `404` not
/// found, `503` offline/transient without Sentry spam, `502` misconfiguration
/// with single-latch Sentry alerting, `500` internal).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum BotAdminError {
    BotOffline,
    BadRequest { message: String },
    Forbidden { message: String },
    NotFound { message: String },
    DiscordMisconfigured { message: String },
    DiscordTransient { message: String },
    Internal { message: String },
}

impl BotAdminError {
    pub(crate) fn from_api_error(err: ApiError) -> Self {
        match err {
            ApiError::ServiceUnavailable(_) => Self::BotOffline,
            ApiError::BadRequest(message) => Self::BadRequest {
                message: message.to_string(),
            },
            ApiError::BadRequestMessage(message) => Self::BadRequest { message },
            ApiError::Forbidden(message) => Self::Forbidden {
                message: message.to_string(),
            },
            ApiError::Discord {
                message,
                kind: DiscordFailure::Misconfigured,
            } => Self::DiscordMisconfigured { message },
            ApiError::Discord {
                message,
                kind: DiscordFailure::Transient,
            } => Self::DiscordTransient { message },
            ApiError::AnyhowError(e) => {
                if let Some(group_err) = e.downcast_ref::<GroupError>() {
                    match group_err {
                        GroupError::Forbidden(msg) => Self::Forbidden {
                            message: (*msg).to_string(),
                        },
                        GroupError::NotFound | GroupError::RoleNotFound => Self::NotFound {
                            message: group_err.to_string(),
                        },
                        GroupError::BadRequest(msg) => Self::BadRequest {
                            message: (*msg).to_string(),
                        },
                        GroupError::ManagedByDiscord | GroupError::RoleManagedByDiscord => {
                            Self::BadRequest {
                                message: group_err.to_string(),
                            }
                        }
                    }
                } else {
                    Self::Internal {
                        message: e.to_string(),
                    }
                }
            }
            other => Self::Internal {
                message: other.to_string(),
            },
        }
    }

    pub(crate) fn into_api_error(self) -> ApiError {
        match self {
            Self::BotOffline => ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE),
            Self::BadRequest { message } => ApiError::BadRequestMessage(message),
            Self::Forbidden { .. } => ApiError::Forbidden("forbidden"),
            Self::NotFound { .. } => ApiError::from(anyhow::Error::from(GroupError::NotFound)),
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
            Self::BadRequest { .. } => StatusCode::BAD_REQUEST,
            Self::Forbidden { .. } => StatusCode::FORBIDDEN,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
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

/// Request payload for `POST /internal/bot/groups/{group_id}/sync`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TriggerGroupSyncRequest {
    pub(crate) owner_id: i64,
    pub(crate) trigger: GroupSyncTrigger,
}

/// Response payload for `POST /internal/bot/groups/{group_id}/sync`.
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
    /// process after verifying ownership, linked non-frozen guild state, and
    /// server-controlled rate-limit policy.
    fn trigger_group_sync<'a>(
        &'a self,
        group_id: i32,
        owner_id: i64,
        trigger: GroupSyncTrigger,
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

/// Validate that a candidate guild ID list is within bounds and contains only
/// positive Discord snowflakes.
#[allow(clippy::result_large_err)]
fn validate_candidate_guild_ids(candidate_guild_ids: Option<&[i64]>) -> Result<(), ApiError> {
    if let Some(ids) = candidate_guild_ids {
        if ids.len() > MAX_CANDIDATE_GUILDS {
            return Err(ApiError::BadRequest("too many candidate guild ids"));
        }
        for &id in ids {
            parse_positive_discord_id(id, "invalid guild_id")?;
        }
    }
    Ok(())
}

/// Validate that an owned group row is actively linked to a positive Discord
/// guild ID and not frozen before touching Serenity or rate limiters.
#[allow(clippy::result_large_err)]
fn validate_sync_target_group(group: &user_group::Model) -> Result<i64, ApiError> {
    group
        .guild_id
        .filter(|&id| id > 0 && group.frozen_reason.is_none())
        .ok_or(ApiError::BadRequest(
            "That group is not linked to a Discord server",
        ))
}

/// Validate that an endpoint row belongs to `expected_owner_id` and uses a
/// Discord delivery method before touching Serenity.
fn validate_delivery_endpoint(
    endpoint: &notification_endpoint::Model,
    expected_owner_id: i64,
) -> Result<(), BotDeliveryError> {
    if endpoint.user_id != expected_owner_id {
        return Err(BotDeliveryError::Failed(
            "endpoint is not owned by caller".into(),
        ));
    }
    if !matches!(endpoint.method.as_str(), "DiscordDm" | "DiscordChannel") {
        return Err(BotDeliveryError::Failed(
            "endpoint is not a Discord delivery method".into(),
        ));
    }
    Ok(())
}

/// Direct in-process [`BotAdminClient`] used whenever the `bot` role is active
/// in this binary (including `ULTROS_ROLES=all` and on `ultros-market` behind
/// `/internal/bot/*`), or as a graceful offline fallback when no remote bot URL
/// is configured.
#[derive(Clone)]
pub(crate) struct InProcessBotAdminClient {
    db: UltrosDb,
    shutdown: CancellationToken,
}

impl InProcessBotAdminClient {
    pub(crate) fn new(db: UltrosDb, shutdown: CancellationToken) -> Self {
        Self { db, shutdown }
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
            parse_positive_discord_id(user_id, "invalid user_id")?;
            validate_candidate_guild_ids(candidate_guild_ids.as_deref())?;
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
            parse_positive_discord_id(user_id, "invalid user_id")?;
            validate_candidate_guild_ids(candidate_guild_ids.as_deref())?;
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
            parse_positive_discord_id(guild_id, "invalid guild_id")?;
            parse_positive_discord_id(user_id, "invalid user_id")?;
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
            parse_positive_discord_id(channel_id, "channel_id must be positive")?;
            parse_positive_discord_id(user_id, "invalid user_id")?;
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
            parse_positive_discord_id(guild_id, "invalid guild_id")?;
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
            parse_positive_discord_id(guild_id, "invalid guild_id")?;
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
        group_id: i32,
        owner_id: i64,
        trigger: GroupSyncTrigger,
    ) -> BoxFuture<'a, Result<GroupSyncStatus, ApiError>> {
        Box::pin(async move {
            if group_id <= 0 {
                return Err(ApiError::BadRequest("invalid group_id"));
            }
            parse_positive_discord_id(owner_id, "invalid user_id")?;
            let group = self.db.get_owned_group(group_id, owner_id).await?;
            let guild_id = validate_sync_target_group(&group)?;
            let _ctx = crate::alerts::delivery::get_serenity_ctx()
                .ok_or(ApiError::ServiceUnavailable(DISCORD_BOT_OFFLINE))?;
            let now = Instant::now();
            match trigger {
                GroupSyncTrigger::Manual => {
                    if !crate::group_sync::sync_rate_limiter().try_acquire(guild_id, now) {
                        return Ok(GroupSyncStatus::RecentlySynced);
                    }
                    crate::group_sync::spawn_reconcile(self.db.clone(), guild_id);
                    Ok(GroupSyncStatus::Ran)
                }
                GroupSyncTrigger::PostRoleImport => {
                    // Only allow bypassing the 5-minute manual cooldown when
                    // Postgres confirms this group actually has an unsynced
                    // imported Discord role, and still guard against rapid
                    // repeated triggers.
                    let has_unsynced_discord_role = self
                        .db
                        .get_group_roles(group_id, owner_id)
                        .await?
                        .into_iter()
                        .any(
                            |ultros_db::common_type_conversions::GroupRoleReturn(role, _)| {
                                role.discord_role_id.is_some() && role.last_synced_at.is_none()
                            },
                        );
                    if !has_unsynced_discord_role {
                        if !crate::group_sync::sync_rate_limiter().try_acquire(guild_id, now) {
                            return Ok(GroupSyncStatus::RecentlySynced);
                        }
                        crate::group_sync::spawn_reconcile(self.db.clone(), guild_id);
                        return Ok(GroupSyncStatus::Ran);
                    }
                    match self
                        .db
                        .claim_post_import_sync(guild_id, POST_IMPORT_BURST_COOLDOWN)
                        .await?
                    {
                        PostImportSyncClaim::ClaimedNow => {
                            crate::group_sync::sync_rate_limiter().record(guild_id, now);
                            crate::group_sync::spawn_reconcile(self.db.clone(), guild_id);
                            Ok(GroupSyncStatus::Ran)
                        }
                        PostImportSyncClaim::Deferred { retry_after } => {
                            crate::group_sync::schedule_post_import_follow_up(
                                self.db.clone(),
                                guild_id,
                                retry_after,
                                self.shutdown.clone(),
                            );
                            Ok(GroupSyncStatus::RecentlySynced)
                        }
                    }
                }
            }
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
            if endpoint_id <= 0 || owner_id <= 0 {
                return Err(BotDeliveryError::Failed(
                    "invalid endpoint_id or owner_id".into(),
                ));
            }
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
            validate_delivery_endpoint(&endpoint, owner_id)?;
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
    bearer_auth: HeaderValue,
    rpc_timeout: Duration,
    fast_rpc_timeout: Duration,
}

impl HttpBotAdminClient {
    pub(crate) fn new(base_url: Url, bearer_token: Option<&str>) -> anyhow::Result<Self> {
        Self::with_timeouts(
            base_url,
            bearer_token,
            BOT_RPC_TIMEOUT,
            FAST_BOT_RPC_TIMEOUT,
        )
    }

    fn with_timeouts(
        base_url: Url,
        bearer_token: Option<&str>,
        rpc_timeout: Duration,
        fast_rpc_timeout: Duration,
    ) -> anyhow::Result<Self> {
        let token = bearer_token
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "ULTROS_MARKET_TOKEN must be set to a non-empty value for HttpBotAdminClient"
                )
            })?;
        let bearer_auth = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|e| anyhow::anyhow!("invalid ULTROS_MARKET_TOKEN header value: {e}"))?;
        let mut builder = reqwest::Client::builder()
            // Private internal RPC must never route through ambient proxy env
            // vars (which break h2c and could leak ULTROS_MARKET_TOKEN).
            .no_proxy()
            .connect_timeout(Duration::from_secs(3))
            .timeout(rpc_timeout)
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
        Ok(Self {
            base_url,
            client,
            bearer_auth,
            rpc_timeout,
            fast_rpc_timeout,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_timeouts_for_test(
        base_url: Url,
        bearer_token: Option<&str>,
        timeout: Duration,
    ) -> anyhow::Result<Self> {
        Self::with_timeouts(base_url, bearer_token, timeout, timeout)
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
        req.header(header::AUTHORIZATION, self.bearer_auth.clone())
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
            self.get_json(&["internal", "bot", "guilds"], &[], self.fast_rpc_timeout)
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
                self.rpc_timeout,
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
                self.rpc_timeout,
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
                self.fast_rpc_timeout,
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
                self.fast_rpc_timeout,
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
                self.fast_rpc_timeout,
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
            parse_positive_discord_id(guild_id, "invalid guild_id")?;
            let trimmed = query.trim();
            if trimmed.is_empty() {
                return Ok(Vec::new());
            }
            let guild_seg = guild_id.to_string();
            let params = [("q", trimmed.to_string()), ("limit", limit.to_string())];
            self.get_json(
                &["internal", "bot", "guilds", &guild_seg, "members", "search"],
                &params,
                self.fast_rpc_timeout,
            )
            .await
        })
    }

    fn trigger_group_sync<'a>(
        &'a self,
        group_id: i32,
        owner_id: i64,
        trigger: GroupSyncTrigger,
    ) -> BoxFuture<'a, Result<GroupSyncStatus, ApiError>> {
        Box::pin(async move {
            let group_seg = group_id.to_string();
            let payload = TriggerGroupSyncRequest { owner_id, trigger };
            let resp: TriggerGroupSyncResponse = self
                .post_json(
                    &["internal", "bot", "groups", &group_seg, "sync"],
                    &payload,
                    self.fast_rpc_timeout,
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
                        .timeout(self.fast_rpc_timeout)
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
            // Strictly require `204 No Content` (the internal delivery contract)
            // so a misrouted `200 OK` from an unrelated upstream can never be
            // mistaken for confirmed Discord delivery.
            if status == StatusCode::NO_CONTENT {
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
                    BotAdminError::BadRequest { message }
                    | BotAdminError::Forbidden { message }
                    | BotAdminError::NotFound { message }
                    | BotAdminError::DiscordMisconfigured { message }
                    | BotAdminError::DiscordTransient { message }
                    | BotAdminError::Internal { message } => Err(BotDeliveryError::Failed(message)),
                };
            }
            Err(BotDeliveryError::Failed(format!(
                "Discord bot service returned unexpected HTTP {status} (expected 204 No Content)"
            )))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_group(guild_id: Option<i64>, frozen_reason: Option<&str>) -> user_group::Model {
        user_group::Model {
            id: 7,
            name: "Raid Static".into(),
            owner_id: 99,
            guild_id,
            guild_icon_url: None,
            source: 1,
            frozen_reason: frozen_reason.map(str::to_owned),
            sync_revision: 1,
        }
    }

    fn test_endpoint(user_id: i64, method: &str) -> notification_endpoint::Model {
        notification_endpoint::Model {
            id: 11,
            user_id,
            name: "Test".into(),
            method: method.into(),
            config: serde_json::json!({}),
            created_at: chrono::Utc::now(),
            disabled_at: None,
            last_error: None,
        }
    }

    #[test]
    fn validate_sync_target_group_rejects_unlinked_frozen_and_zero_guild_ids() {
        assert_eq!(
            validate_sync_target_group(&test_group(None, None))
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            validate_sync_target_group(&test_group(Some(123), Some("bot removed")))
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            validate_sync_target_group(&test_group(Some(0), None))
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            validate_sync_target_group(&test_group(Some(-42), None))
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            validate_sync_target_group(&test_group(Some(12345), None)).unwrap(),
            12345
        );
    }

    #[test]
    fn validate_delivery_endpoint_enforces_owner_and_discord_method() {
        assert!(validate_delivery_endpoint(&test_endpoint(99, "DiscordDm"), 99).is_ok());
        assert!(validate_delivery_endpoint(&test_endpoint(99, "DiscordChannel"), 99).is_ok());

        // Foreign owner rejected.
        assert!(matches!(
            validate_delivery_endpoint(&test_endpoint(100, "DiscordDm"), 99),
            Err(BotDeliveryError::Failed(msg)) if msg.contains("not owned")
        ));

        // Non-Discord methods rejected.
        for method in ["Webhook", "WebPush", "InApp", "InGame"] {
            assert!(
                matches!(
                    validate_delivery_endpoint(&test_endpoint(99, method), 99),
                    Err(BotDeliveryError::Failed(msg)) if msg.contains("not a Discord delivery method")
                ),
                "expected rejection for method {method}"
            );
        }
    }

    #[test]
    fn validate_candidate_guild_ids_rejects_zero_negative_and_oversized_lists() {
        assert!(validate_candidate_guild_ids(None).is_ok());
        assert!(validate_candidate_guild_ids(Some(&[1, 2, 3])).is_ok());
        assert_eq!(
            validate_candidate_guild_ids(Some(&[1, 0]))
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            validate_candidate_guild_ids(Some(&[-1]))
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
        let too_many: Vec<i64> = (1..=(MAX_CANDIDATE_GUILDS as i64 + 1)).collect();
        assert_eq!(
            validate_candidate_guild_ids(Some(&too_many))
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
}
