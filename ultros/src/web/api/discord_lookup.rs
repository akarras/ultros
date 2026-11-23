//! Helpers for resolving Discord channels + verifying guild admin permissions
//! from the web layer. These bridge the live serenity context (set once at
//! Discord bot startup, see [`crate::alerts::delivery::set_serenity_ctx`]) with
//! HTTP handlers that need to check Discord state — most importantly when a
//! user creates a `DiscordChannel` notification endpoint.
//!
//! Everything in here makes a live HTTP call to Discord. We never rely on
//! serenity's cache for permission decisions, since the cache may be empty for
//! guilds the bot has just joined or for users who haven't appeared in any
//! gateway event yet.

use axum_extra::extract::PrivateCookieJar;
use poise::serenity_prelude::{
    self as serenity, ChannelId, ChannelType, GuildId, GuildPagination, Http, Member, Permissions,
    Role, UserId,
};
use std::collections::HashSet;
use std::time::Duration;
use ultros_api_types::alert::{DiscordWritableChannel, DiscordWritableGuild};

use crate::web::error::{ApiError, DiscordFailure};
use crate::web::oauth::AuthUserCache;

/// Wall-clock budget for the legacy OAuth fallback (sessions issued without the
/// `guilds` scope that receive HTTP 403 from `/users/@me/guilds`). Kept well
/// inside the 30-second internal bot-admin RPC timeout so a slow legacy scan
/// returns an actionable reauthorization prompt instead of timing out the RPC
/// and falsely reporting a healthy bot as offline.
pub(crate) const LEGACY_GUILD_SCAN_BUDGET: Duration = Duration::from_secs(15);
pub(crate) const LEGACY_OAUTH_REAUTH_MESSAGE: &str = "Your Discord login predates server-list permissions and could not finish scanning in time; please sign out and sign in again to refresh Discord permissions.";

/// Validate a Discord snowflake ID before passing it to Serenity's `NonZeroU64`
/// constructors (`GuildId::new`, `UserId::new`, `ChannelId::new`), which panic
/// on `0`.
#[allow(clippy::result_large_err)]
pub(crate) fn parse_positive_discord_id(id: i64, message: &'static str) -> Result<u64, ApiError> {
    if id <= 0 {
        return Err(ApiError::BadRequest(message));
    }
    Ok(id as u64)
}

/// Resolved metadata for a Discord channel that is bound to a notification
/// endpoint. Channel-name and guild-name are display-only; `guild_id` is also
/// load-bearing for admin checks.
pub(crate) struct ResolvedChannel {
    pub channel_name: String,
    pub guild_id: i64,
    pub guild_name: String,
}

/// Look up a channel by id and return its display name + owning guild. Errors
/// when the bot cannot see the channel (not in the guild, channel deleted) or
/// when the channel is a DM (no guild → cannot run admin check, so we treat
/// it as a misconfiguration).
pub(crate) async fn resolve_channel(
    ctx: &serenity::Context,
    channel_id: i64,
) -> Result<ResolvedChannel, ApiError> {
    let channel_id_u64 = parse_positive_discord_id(channel_id, "channel_id must be positive")?;
    let channel = ChannelId::new(channel_id_u64)
        .to_channel(&ctx.http)
        .await
        .map_err(|e| {
            ApiError::from(anyhow::anyhow!(
                "Discord could not resolve channel {channel_id}: {e}. \
                 The bot must be a member of the guild containing this channel."
            ))
        })?;

    // Only guild channels can be bound to notifications (DMs are owned by a
    // single user, so there's no "admin" concept — and the user wouldn't be
    // sending themselves a notification through a foreign DM channel anyway).
    let guild_channel = channel.guild().ok_or_else(|| {
        ApiError::from(anyhow::anyhow!(
            "channel {channel_id} is not in a guild; only server channels can be \
             used for notifications"
        ))
    })?;

    let guild_id_i64 = i64::try_from(guild_channel.guild_id.get())
        .map_err(|_| ApiError::from(anyhow::anyhow!("guild_id overflowed i64 (impossible)")))?;
    let guild_name = guild_channel
        .guild_id
        .name(&ctx.cache)
        .unwrap_or_else(|| format!("Guild {}", guild_channel.guild_id.get()));

    Ok(ResolvedChannel {
        channel_name: guild_channel.name,
        guild_id: guild_id_i64,
        guild_name,
    })
}

/// Verify the given user has at least one of [`Permissions::ADMINISTRATOR`] or
/// [`Permissions::MANAGE_GUILD`] in the given guild. Errors with a user-facing
/// message otherwise (member not in the guild, missing perms, Discord HTTP
/// failure).
///
/// Fetch the guild and member to compute permissions from their current roles,
/// because serenity's cache-based [`serenity::Member::permissions`]
/// requires a populated cache, which is not guaranteed for the user's guild
/// when they are creating an endpoint via the web UI.
pub(crate) async fn require_user_is_guild_admin(
    ctx: &serenity::Context,
    guild_id: i64,
    user_id: i64,
) -> Result<(), ApiError> {
    require_manageable_guild(ctx, guild_id, user_id)
        .await
        .map(|_| ())
}

/// As [`require_user_is_guild_admin`], but hands back the guild it already had
/// to fetch, so callers that need the guild's name or icon don't pay for a
/// second round trip.
pub(crate) async fn require_manageable_guild(
    ctx: &serenity::Context,
    guild_id: i64,
    user_id: i64,
) -> Result<serenity::PartialGuild, ApiError> {
    require_manageable_guild_via(&ctx.http, guild_id, user_id).await
}

async fn require_manageable_guild_via(
    http: &Http,
    guild_id: i64,
    user_id: i64,
) -> Result<serenity::PartialGuild, ApiError> {
    let guild_id = parse_positive_discord_id(guild_id, "invalid guild_id")?;
    let user_id = parse_positive_discord_id(user_id, "invalid user_id")?;
    let guild = GuildId::new(guild_id);

    // Owner shortcut: skip the role walk, they always have everything. Fetching
    // the partial guild also surfaces "bot is not in the guild" with a clearer
    // message than the member fetch would.
    let partial = guild.to_partial_guild(http).await.map_err(|e| {
        ApiError::from(anyhow::anyhow!(
            "Discord could not load guild {guild_id}: {e}. \
             The bot must be a member of the guild."
        ))
    })?;
    if partial.owner_id == UserId::new(user_id) {
        return Ok(partial);
    }

    let member = guild
        .member(http, UserId::new(user_id))
        .await
        .map_err(|e| {
            ApiError::from(anyhow::anyhow!(
                "you do not appear to be a member of that Discord server (guild lookup failed: {e})"
            ))
        })?;

    // Includes @everyone as well as the member's explicit roles.
    let perms = partial.member_permissions(&member);

    if perms.contains(Permissions::ADMINISTRATOR) || perms.contains(Permissions::MANAGE_GUILD) {
        Ok(partial)
    } else {
        Err(ApiError::from(anyhow::anyhow!(
            "you must have Administrator or Manage Server permission in that Discord server"
        )))
    }
}

/// Whether a guild role is one a group may import.
///
/// Managed roles belong to a bot, an integration, or Nitro boosting: Discord
/// itself decides who holds them and refuses to let anyone else be given one,
/// so importing them would produce a group nobody can be added to. `@everyone`
/// deliberately survives this filter — importing it is how the spec expresses
/// "sync the whole server".
fn is_importable_role(role: &Role) -> bool {
    !role.managed && role.tags.bot_id.is_none() && role.tags.integration_id.is_none()
}

/// Discord leaves an uncoloured role at `0`, which is not black — it means
/// "inherit", so the picker must render its own default rather than a swatch.
fn role_colour(role: &Role) -> Option<String> {
    (role.colour.0 != 0).then(|| format!("#{:06x}", role.colour.0))
}

/// Turn a failed Discord call into an error whose message reaches the client.
///
/// The `kind` is what decides the status and whether an operator is paged;
/// classification follows the repo's `is_transient` convention so this layer
/// and the background sync task agree on what counts as weather.
///
/// Deliberately *not* `anyhow::anyhow!("{message}")`: `ApiError::AnyhowError`
/// has no arm in `as_api_error`, so every message built here used to be
/// replaced by "Internal server error" at the response boundary — including
/// the 403 text the spec requires to be diagnosable from the UI.
fn discord_error(message: String, error: &serenity::Error) -> ApiError {
    let kind = if crate::group_sync::reconcile::is_transient(error) {
        DiscordFailure::Transient
    } else if matches!(error, serenity::Error::Http(_)) {
        DiscordFailure::Misconfigured
    } else {
        // Decode, model and format errors are schema drift or a bug in this
        // process — not something Discord told us, and not something an
        // operator can fix by flipping a setting. They keep the 500 and the
        // issue in the error tracker, because somebody here has to look.
        return ApiError::from(anyhow::anyhow!("{message}"));
    };
    ApiError::Discord { message, kind }
}

/// The guild's importable roles, highest first, the order they appear in
/// Discord's own role list.
pub(crate) async fn importable_guild_roles(
    ctx: &serenity::Context,
    guild_id: i64,
) -> Result<Vec<(i64, String, i32, Option<String>)>, ApiError> {
    importable_guild_roles_via(&ctx.http, guild_id).await
}

/// [`importable_guild_roles`] against a bare [`Http`], so the failure path can
/// be driven end to end from a test: a `serenity::Context` cannot be built
/// outside a running gateway connection, but an `Http` can be pointed at a
/// mock.
async fn importable_guild_roles_via(
    http: &Http,
    guild_id: i64,
) -> Result<Vec<(i64, String, i32, Option<String>)>, ApiError> {
    let guild_id = parse_positive_discord_id(guild_id, "invalid guild_id")?;
    let roles = GuildId::new(guild_id).roles(http).await.map_err(|e| {
        discord_error(
            format!(
                "Discord could not load the roles for that server: {e}. \
                 The bot must still be a member of it."
            ),
            &e,
        )
    })?;
    let mut roles: Vec<_> = roles
        .into_values()
        .filter(is_importable_role)
        .map(|role| {
            let colour = role_colour(&role);
            (
                role.id.get() as i64,
                role.name,
                i32::from(role.position),
                colour,
            )
        })
        .collect();
    // Discord numbers positions upwards, so descending here is the order the
    // server settings screen shows. Name breaks ties: `@everyone` and a fresh
    // role both sit at position 0.
    roles.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.cmp(&b.1)));
    Ok(roles)
}

/// Prefix-search the guild's members through Discord's own search endpoint,
/// which matches username and nickname and needs no privileged intent.
///
/// A 403 still names the Server Members intent: that is what a deploy which
/// forgot to enable it looks like from here, and the message has to be
/// diagnosable from the UI rather than showing up as an opaque failure.
pub(crate) async fn search_guild_members(
    ctx: &serenity::Context,
    guild_id: i64,
    query: &str,
    limit: u64,
) -> Result<Vec<Member>, ApiError> {
    search_guild_members_via(&ctx.http, guild_id, query, limit).await
}

/// [`search_guild_members`] against a bare [`Http`] — see
/// [`importable_guild_roles_via`] for why the split exists.
async fn search_guild_members_via(
    http: &Http,
    guild_id: i64,
    query: &str,
    limit: u64,
) -> Result<Vec<Member>, ApiError> {
    let guild_id = parse_positive_discord_id(guild_id, "invalid guild_id")?;
    http.search_guild_members(GuildId::new(guild_id), query, Some(limit))
        .await
        .map_err(|e| discord_error(member_search_error(&e), &e))
}

fn member_search_error(error: &serenity::Error) -> String {
    let status = match error {
        serenity::Error::Http(http) => http.status_code().map(|status| status.as_u16()),
        _ => None,
    };
    member_search_message(status, error)
}

/// Split out from [`member_search_error`] so the wording is testable:
/// serenity's `ErrorResponse` is `#[non_exhaustive]`, so a 403 cannot be
/// constructed from outside that crate.
fn member_search_message(status: Option<u16>, error: impl std::fmt::Display) -> String {
    match status {
        Some(403) => "Discord refused the member search (403). The Ultros bot needs the \
                      Server Members intent enabled in the Discord developer portal."
            .to_string(),
        _ => format!("Discord member search failed: {error}"),
    }
}

/// Compatibility for sessions issued before we requested the `guilds` scope.
/// Keep this bot-wide scan only for Discord's explicit missing-access response,
/// and bound every call directly by `deadline` so:
/// - a slow-but-within-budget Discord call still completes and is never silently
///   omitted from the returned list, and
/// - once the overall scan budget is exhausted, the caller gets an actionable
///   reauthorization prompt instead of tripping the 30s RPC timeout or seeing a
///   misleading empty `200` list.
async fn legacy_guilds_user_manages_via(
    http: &Http,
    bot_guilds: &[GuildId],
    user_id: UserId,
    deadline: tokio::time::Instant,
) -> Result<Vec<serenity::PartialGuild>, ApiError> {
    let mut guilds = Vec::new();
    for &guild_id in bot_guilds {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(ApiError::BadRequest(LEGACY_OAUTH_REAUTH_MESSAGE));
        }
        let partial = match tokio::time::timeout(remaining, guild_id.to_partial_guild(http)).await {
            Ok(Ok(guild)) => guild,
            Ok(Err(e)) => {
                tracing::warn!(
                    guild_id = guild_id.get(),
                    "failed to load Discord guild: {e}"
                );
                continue;
            }
            Err(_) => return Err(ApiError::BadRequest(LEGACY_OAUTH_REAUTH_MESSAGE)),
        };

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(ApiError::BadRequest(LEGACY_OAUTH_REAUTH_MESSAGE));
        }
        // A miss here is the overwhelmingly common case (the user is not in
        // most of the bot's guilds), so it is not worth logging.
        let user_member = match tokio::time::timeout(remaining, partial.member(http, user_id)).await
        {
            Ok(Ok(member)) => member,
            Ok(Err(_)) => continue,
            Err(_) => return Err(ApiError::BadRequest(LEGACY_OAUTH_REAUTH_MESSAGE)),
        };
        let user_permissions = partial.member_permissions(&user_member);
        if !user_permissions.contains(Permissions::ADMINISTRATOR)
            && !user_permissions.contains(Permissions::MANAGE_GUILD)
        {
            continue;
        }
        guilds.push(partial);
    }
    Ok(guilds)
}

fn shared_manageable_guild(guild: &serenity::GuildInfo, bot_guilds: &HashSet<GuildId>) -> bool {
    bot_guilds.contains(&guild.id)
        && (guild.owner
            || guild
                .permissions
                .intersects(Permissions::ADMINISTRATOR | Permissions::MANAGE_GUILD))
}

/// Discover candidate guild IDs shared between the authenticated user's OAuth
/// grant and the bot's guilds, without letting the user's OAuth token leave
/// `ultros-web`.
///
/// Returns `Ok(None)` when Discord replies `403` (grant issued without the
/// `guilds` scope), signalling the bot side to fall back to the legacy
/// bot-wide scan.
pub(crate) async fn discover_oauth_candidate_guild_ids(
    bot_guild_ids: &[i64],
    cookies: &PrivateCookieJar,
    cache: &AuthUserCache,
) -> Result<Option<Vec<i64>>, ApiError> {
    let token = cookies
        .get(crate::web::oauth::DISCORD_AUTH_COOKIE)
        .ok_or(ApiError::NoAuthCookie)?;
    let http = Http::new(&format!("Bearer {}", token.value()));
    let bot_guilds: HashSet<GuildId> = bot_guild_ids
        .iter()
        .copied()
        .filter(|&id| id > 0)
        .map(|id| GuildId::new(id as u64))
        .collect();
    let Some(candidates) = oauth_manageable_guild_ids(&http, cookies, &bot_guilds, cache).await?
    else {
        return Ok(None);
    };
    let mut ids: Vec<i64> = candidates
        .into_iter()
        .filter_map(|id| i64::try_from(id.get()).ok())
        .collect();
    ids.sort_unstable();
    Ok(Some(ids))
}

/// Resolve partial guilds from pre-intersected OAuth candidate guild IDs (when
/// `Some`) or fall back to the bounded legacy bot-wide permission scan (when
/// `None`).
///
/// Intersects `candidate_guild_ids` against `ctx.cache.guilds()` again so the
/// bot never probes guilds it is not in, even if the cache changed between
/// requests.
async fn guilds_from_candidates(
    ctx: &serenity::Context,
    user_id: UserId,
    candidate_guild_ids: Option<&[i64]>,
    legacy_deadline: Option<tokio::time::Instant>,
) -> Result<Vec<serenity::PartialGuild>, ApiError> {
    let Some(candidate_ids) = candidate_guild_ids else {
        let bot_guilds = ctx.cache.guilds();
        let deadline = legacy_deadline
            .unwrap_or_else(|| tokio::time::Instant::now() + LEGACY_GUILD_SCAN_BUDGET);
        return legacy_guilds_user_manages_via(&ctx.http, &bot_guilds, user_id, deadline).await;
    };
    let bot_guilds: HashSet<GuildId> = ctx.cache.guilds().into_iter().collect();
    let mut seen = HashSet::new();
    let mut guilds = Vec::new();
    for &raw_id in candidate_ids {
        let id_u64 = parse_positive_discord_id(raw_id, "invalid guild_id")?;
        let id = GuildId::new(id_u64);
        if !bot_guilds.contains(&id) || !seen.insert(id) {
            continue;
        }
        match id.to_partial_guild(&ctx.http).await {
            Ok(guild) => guilds.push(guild),
            Err(error) => {
                tracing::warn!(guild_id = id.get(), "failed to load Discord guild: {error}")
            }
        }
    }
    Ok(guilds)
}

/// `None` requests the compatibility path for grants without `guilds`.
async fn oauth_manageable_guild_ids(
    http: &Http,
    cookies: &PrivateCookieJar,
    bot_guilds: &HashSet<GuildId>,
    cache: &AuthUserCache,
) -> Result<Option<HashSet<GuildId>>, ApiError> {
    let mut after = None;
    let mut candidates = HashSet::new();
    loop {
        let page = match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            http.get_guilds(after.map(GuildPagination::After), Some(200)),
        )
        .await
        {
            Ok(Ok(page)) => page,
            Ok(Err(serenity::Error::Http(error)))
                if error
                    .status_code()
                    .is_some_and(|status| status.as_u16() == 403) =>
            {
                return Ok(None);
            }
            Ok(Err(serenity::Error::Http(error)))
                if error
                    .status_code()
                    .is_some_and(|status| status.as_u16() == 401) =>
            {
                if let Some(token) = cookies.get(crate::web::oauth::DISCORD_AUTH_COOKIE) {
                    cache.remove_token(token.value()).await;
                }
                return Err(ApiError::DiscordTokenInvalid(Box::new(cookies.clone())));
            }
            Ok(Err(error)) => return Err(anyhow::anyhow!(error).into()),
            Err(_) => return Err(anyhow::anyhow!("Discord guild discovery timed out").into()),
        };
        candidates.extend(
            page.iter()
                .filter(|guild| shared_manageable_guild(guild, bot_guilds))
                .map(|guild| guild.id),
        );
        if page.len() < 200 {
            break;
        }
        let next = page.iter().map(|guild| guild.id).max();
        if next <= after {
            return Err(anyhow::anyhow!("Discord guild pagination did not advance").into());
        }
        after = next;
    }

    Ok(Some(candidates))
}

/// Guilds the user may turn into an Ultros group: the bot is in them and the
/// user can manage them. Unlike [`writable_guilds_from_candidates`] this does
/// not probe channels — a group has nothing to post, so requiring a
/// bot-writable channel would wrongly exclude valid servers (and the channel
/// fetch is the expensive part of that function).
pub(crate) async fn manageable_guilds_from_candidates(
    ctx: &serenity::Context,
    user_id: i64,
    candidate_guild_ids: Option<&[i64]>,
) -> Result<Vec<(i64, String, Option<String>)>, ApiError> {
    let user_id = parse_positive_discord_id(user_id, "invalid user_id")?;
    let mut guilds = guilds_from_candidates(ctx, UserId::new(user_id), candidate_guild_ids, None)
        .await?
        .into_iter()
        .map(|partial| {
            (
                partial.id.get() as i64,
                partial.name.clone(),
                partial.icon_url(),
            )
        })
        .collect::<Vec<_>>();
    guilds.sort_by_key(|a| a.1.to_lowercase());
    Ok(guilds)
}

/// Return guilds that:
///
/// - the bot is in,
/// - the authenticated web user is a member of,
/// - the authenticated web user can administer or manage,
/// - and the bot can post embeds into at least one text/news channel.
///
/// This powers the web "Discord channel" endpoint picker.
pub(crate) async fn writable_guilds_from_candidates(
    ctx: &serenity::Context,
    user_id: i64,
    candidate_guild_ids: Option<&[i64]>,
) -> Result<Vec<DiscordWritableGuild>, ApiError> {
    let user_id = parse_positive_discord_id(user_id, "invalid user_id")?;
    let user_id = UserId::new(user_id);
    let bot_user_id = ctx.cache.current_user().id;
    let legacy_deadline = candidate_guild_ids
        .is_none()
        .then(|| tokio::time::Instant::now() + LEGACY_GUILD_SCAN_BUDGET);
    let partials =
        guilds_from_candidates(ctx, user_id, candidate_guild_ids, legacy_deadline).await?;
    writable_guilds_from_partials_via(&ctx.http, partials, bot_user_id, legacy_deadline).await
}

async fn writable_guilds_from_partials_via(
    http: &Http,
    partials: Vec<serenity::PartialGuild>,
    bot_user_id: UserId,
    legacy_deadline: Option<tokio::time::Instant>,
) -> Result<Vec<DiscordWritableGuild>, ApiError> {
    let mut guilds = Vec::new();

    for partial in partials {
        let guild_id = partial.id;
        let bot_member = if let Some(deadline) = legacy_deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(ApiError::BadRequest(LEGACY_OAUTH_REAUTH_MESSAGE));
            }
            match tokio::time::timeout(remaining, partial.member(http, bot_user_id)).await {
                Ok(Ok(member)) => member,
                Ok(Err(e)) => {
                    tracing::warn!(
                        guild_id = guild_id.get(),
                        "failed to load bot member for Discord guild: {e}"
                    );
                    continue;
                }
                Err(_) => return Err(ApiError::BadRequest(LEGACY_OAUTH_REAUTH_MESSAGE)),
            }
        } else {
            match partial.member(http, bot_user_id).await {
                Ok(member) => member,
                Err(e) => {
                    tracing::warn!(
                        guild_id = guild_id.get(),
                        "failed to load bot member for Discord guild: {e}"
                    );
                    continue;
                }
            }
        };

        let raw_channels = if let Some(deadline) = legacy_deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(ApiError::BadRequest(LEGACY_OAUTH_REAUTH_MESSAGE));
            }
            tokio::time::timeout(remaining, partial.channels(http))
                .await
                .map_err(|_| ApiError::BadRequest(LEGACY_OAUTH_REAUTH_MESSAGE))?
        } else {
            partial.channels(http).await
        };

        let mut channels = raw_channels
            .map_err(|e| {
                ApiError::from(anyhow::anyhow!(
                    "Discord could not load channels for guild {}: {e}",
                    partial.name
                ))
            })?
            .into_values()
            .filter(|channel| matches!(channel.kind, ChannelType::Text | ChannelType::News))
            .filter(|channel| {
                let permissions = partial.user_permissions_in(channel, &bot_member);
                permissions.contains(Permissions::VIEW_CHANNEL)
                    && permissions.contains(Permissions::SEND_MESSAGES)
                    && permissions.contains(Permissions::EMBED_LINKS)
            })
            .map(|channel| DiscordWritableChannel {
                id: channel.id.get() as i64,
                name: channel.name,
            })
            .collect::<Vec<_>>();
        channels.sort_by_key(|a| a.name.to_lowercase());

        guilds.push(DiscordWritableGuild {
            id: partial.id.get() as i64,
            name: partial.name.clone(),
            icon_url: partial.icon_url(),
            channels,
        });
    }

    guilds.sort_by_key(|a| a.name.to_lowercase());
    Ok(guilds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    async fn mock_discord(app: axum::Router) -> (Http, tokio::task::JoinHandle<()>) {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = serenity::HttpBuilder::new("Bearer test-token")
            .proxy(format!("http://{address}"))
            .ratelimiter_disabled(true)
            .build();
        (http, server)
    }

    #[tokio::test]
    async fn only_missing_scope_uses_legacy_discovery() {
        for status in [401, 403, 429, 500] {
            let app = axum::Router::new().fallback(move || async move {
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    axum::Json(serde_json::json!({"code": 0, "message": "test response"})),
                )
            });
            let (http, server) = mock_discord(app).await;
            let cookies = PrivateCookieJar::new(axum_extra::extract::cookie::Key::generate())
                .add(crate::web::oauth::discord_auth_cookie("test-token".into()));
            let cache = AuthUserCache::new();
            cache
                .store_user(
                    "test-token",
                    crate::web::oauth::AuthDiscordUser {
                        id: 1,
                        name: "test".into(),
                        avatar_url: String::new(),
                    },
                )
                .await;
            let result = oauth_manageable_guild_ids(&http, &cookies, &HashSet::new(), &cache).await;
            server.abort();
            assert_eq!(
                cache.get_user("test-token").await.is_none(),
                status == 401,
                "only a confirmed invalid token should evict cached authentication"
            );
            match status {
                403 => assert!(matches!(result, Ok(None))),
                401 => assert!(matches!(result, Err(ApiError::DiscordTokenInvalid(_)))),
                _ => assert!(result.is_err()),
            }
        }
    }

    #[tokio::test]
    async fn oauth_discovery_paginates_before_intersecting_bot_guilds() {
        let app = axum::Router::new().fallback(
            |axum::extract::Query(query): axum::extract::Query<
                std::collections::HashMap<String, String>,
            >| async move {
                assert_eq!(query.get("limit").map(String::as_str), Some("200"));
                let ids = match query.get("after").map(String::as_str) {
                    None => 1..=200,
                    Some("200") => 201..=201,
                    other => panic!("unexpected cursor {other:?}"),
                };
                axum::Json(
                    ids.map(|id| guild(id, false, Permissions::MANAGE_GUILD))
                        .collect::<Vec<_>>(),
                )
            },
        );
        let (http, server) = mock_discord(app).await;
        let cookies = PrivateCookieJar::new(axum_extra::extract::cookie::Key::generate());
        let bot_guilds = HashSet::from([GuildId::new(2), GuildId::new(201), GuildId::new(999)]);
        let result =
            oauth_manageable_guild_ids(&http, &cookies, &bot_guilds, &AuthUserCache::new()).await;
        server.abort();
        assert_eq!(
            result.unwrap().unwrap(),
            HashSet::from([GuildId::new(2), GuildId::new(201)])
        );
    }

    fn role(value: serde_json::Value) -> Role {
        let mut base = serde_json::json!({
            "id": "1", "guild_id": "1", "color": 0, "colors": {"primary_color": 0},
            "hoist": false, "managed": false, "mentionable": false,
            "name": "A role", "permissions": "0", "position": 1, "tags": {}
        });
        let serde_json::Value::Object(overrides) = value else {
            panic!("role overrides must be an object");
        };
        for (key, value) in overrides {
            base[key] = value;
        }
        serde_json::from_value(base).unwrap()
    }

    /// A managed role — a bot's own role, an integration's, or the Nitro
    /// booster role — cannot be handed to anyone by us, so importing it would
    /// build a group nobody can join.
    #[test]
    fn managed_and_bot_roles_are_not_importable() {
        assert!(is_importable_role(&role(serde_json::json!({}))));
        assert!(!is_importable_role(&role(
            serde_json::json!({"managed": true})
        )));
        assert!(!is_importable_role(&role(
            serde_json::json!({"tags": {"bot_id": "42"}})
        )));
        assert!(!is_importable_role(&role(
            serde_json::json!({"tags": {"integration_id": "42"}})
        )));
    }

    /// `@everyone` carries the guild's own id and is an ordinary unmanaged
    /// role. It has to stay importable: the spec makes importing it the way to
    /// sync a whole server, so filtering it out would remove that feature.
    #[test]
    fn the_everyone_role_stays_importable() {
        assert!(is_importable_role(&role(
            serde_json::json!({"id": "1", "name": "@everyone", "position": 0})
        )));
    }

    /// Discord uses `0` for "no colour", which is inherit, not black.
    #[test]
    fn only_a_coloured_role_reports_a_colour() {
        assert_eq!(role_colour(&role(serde_json::json!({"color": 0}))), None);
        assert_eq!(
            role_colour(&role(serde_json::json!({"color": 0x5865f2}))),
            Some("#5865f2".to_string())
        );
        // Leading zeroes have to survive, or the hex is a different colour.
        assert_eq!(
            role_colour(&role(serde_json::json!({"color": 0x00ff00}))),
            Some("#00ff00".to_string())
        );
    }

    /// A misconfigured deploy has to be diagnosable from the UI, so the one
    /// status that means "the intent is off" says so by name.
    #[test]
    fn a_forbidden_member_search_names_the_server_members_intent() {
        let message = member_search_message(Some(403), "Missing Access");
        assert!(
            message.contains("Server Members intent"),
            "unexpected message: {message}"
        );

        // Every other failure keeps Discord's own words rather than sending an
        // operator to check a setting that is fine.
        for status in [None, Some(429), Some(500)] {
            let message = member_search_message(status, "connection reset");
            assert!(!message.contains("Server Members intent"), "{status:?}");
            assert!(message.contains("connection reset"), "{status:?}");
        }
    }

    /// Serve `status` for every request and hand back the response the given
    /// Discord call turns into, all the way through `IntoResponse`.
    ///
    /// Building the message is only half the job — the half the old tests
    /// covered while the bug was live. What matters is that it survives
    /// `ApiError::into_response`, so this asserts on the bytes on the wire.
    async fn failure_response<F, Fut>(status: u16, call: F) -> (axum::http::StatusCode, String)
    where
        F: FnOnce(Http) -> Fut,
        Fut: std::future::Future<Output = Result<(), ApiError>>,
    {
        let app = axum::Router::new().fallback(move || async move {
            (
                axum::http::StatusCode::from_u16(status).unwrap(),
                axum::Json(serde_json::json!({"code": 0, "message": "Missing Access"})),
            )
        });
        let (http, server) = mock_discord(app).await;
        let error = call(http).await.expect_err("Discord returned an error");
        server.abort();
        let response = error.into_response();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    /// The 403 text is worth nothing if it does not reach the browser.
    ///
    /// It used to not: the message was built, wrapped in `anyhow`, and then
    /// replaced with "Internal server error" by `ApiError::as_api_error`,
    /// which downcasts only to the list/retainer/group error types. The
    /// message-building test above passed the whole time.
    #[tokio::test]
    async fn a_forbidden_member_search_reaches_the_client_intact() {
        let (status, body) = failure_response(403, |http| async move {
            search_guild_members_via(&http, 1, "ali", 25)
                .await
                .map(|_| ())
        })
        .await;
        assert_eq!(
            status,
            axum::http::StatusCode::BAD_GATEWAY,
            "a refusal from Discord is an upstream failure, not the caller's fault"
        );
        assert!(
            body.contains("Server Members intent"),
            "the response body must name the intent: {body}"
        );
        assert!(
            !body.contains("Internal server error"),
            "the spec'd message must not be swallowed: {body}"
        );
    }

    /// The role import picker fails through the same path, and the spec asks
    /// for a clear message on all three of import, search and sync.
    #[tokio::test]
    async fn a_forbidden_role_listing_reaches_the_client_intact() {
        let (status, body) = failure_response(403, |http| async move {
            importable_guild_roles_via(&http, 1).await.map(|_| ())
        })
        .await;
        assert_eq!(status, axum::http::StatusCode::BAD_GATEWAY);
        assert!(
            body.contains("must still be a member"),
            "the response body must explain itself: {body}"
        );
        assert!(!body.contains("Internal server error"), "{body}");
    }

    /// A rate limit is weather. It answers 503 so the client knows to retry,
    /// and it still says what happened rather than "Internal server error".
    #[tokio::test]
    async fn a_rate_limited_member_search_is_a_retryable_503() {
        for status in [429, 500] {
            let (status, body) = failure_response(status, |http| async move {
                search_guild_members_via(&http, 1, "ali", 25)
                    .await
                    .map(|_| ())
            })
            .await;
            assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
            assert!(body.contains("Discord member search failed"), "{body}");
            // A transient blip must not send an operator to check a setting
            // that is fine.
            assert!(!body.contains("Server Members intent"), "{body}");
        }
    }

    /// The carve-out is only for failures Discord actually reported. Schema
    /// drift in serenity's own decoding is a bug on this side, so it keeps the
    /// 500 and the tracker entry rather than being dressed up as an upstream
    /// problem an operator is supposed to fix.
    #[test]
    fn a_local_decoding_bug_is_not_blamed_on_discord() {
        let error = discord_error(
            "Discord member search failed: bad payload".to_string(),
            &serenity::Error::Other("unexpected shape"),
        );
        assert!(!matches!(error, ApiError::Discord { .. }));
        assert_eq!(
            error.into_response().status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    fn guild(id: u64, owner: bool, permissions: Permissions) -> serenity::GuildInfo {
        serde_json::from_value(serde_json::json!({
            "id": id.to_string(), "name": "A server", "icon": null,
            "owner": owner, "permissions": permissions.bits().to_string(), "features": []
        }))
        .unwrap()
    }

    #[test]
    fn discovery_requires_bot_membership_and_server_management_rights() {
        let bot_guilds = HashSet::from([GuildId::new(1)]);
        assert!(shared_manageable_guild(
            &guild(1, true, Permissions::empty()),
            &bot_guilds
        ));
        assert!(shared_manageable_guild(
            &guild(1, false, Permissions::ADMINISTRATOR),
            &bot_guilds
        ));
        assert!(shared_manageable_guild(
            &guild(1, false, Permissions::MANAGE_GUILD),
            &bot_guilds
        ));
        assert!(!shared_manageable_guild(
            &guild(1, false, Permissions::MANAGE_CHANNELS),
            &bot_guilds
        ));
        assert!(!shared_manageable_guild(
            &guild(2, true, Permissions::ADMINISTRATOR),
            &bot_guilds
        ));
    }

    #[tokio::test]
    async fn zero_and_negative_discord_ids_are_rejected_without_panicking() {
        let app = axum::Router::new().fallback(|| async { axum::http::StatusCode::OK });
        let (http, server) = mock_discord(app).await;

        for invalid in [0i64, -1, i64::MIN] {
            let err = importable_guild_roles_via(&http, invalid)
                .await
                .unwrap_err();
            assert_eq!(
                err.into_response().status(),
                axum::http::StatusCode::BAD_REQUEST
            );

            let err = search_guild_members_via(&http, invalid, "ali", 10)
                .await
                .unwrap_err();
            assert_eq!(
                err.into_response().status(),
                axum::http::StatusCode::BAD_REQUEST
            );

            let err = require_manageable_guild_via(&http, invalid, 123)
                .await
                .unwrap_err();
            assert_eq!(
                err.into_response().status(),
                axum::http::StatusCode::BAD_REQUEST
            );

            let err = require_manageable_guild_via(&http, 123, invalid)
                .await
                .unwrap_err();
            assert_eq!(
                err.into_response().status(),
                axum::http::StatusCode::BAD_REQUEST
            );
        }

        server.abort();
    }

    #[tokio::test]
    async fn legacy_guild_scan_returns_reauth_error_when_budget_expires() {
        let app = axum::Router::new().fallback(|| async {
            tokio::time::sleep(Duration::from_millis(120)).await;
            (
                axum::http::StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({"code": 10004, "message": "Unknown Guild"})),
            )
        });
        let (http, server) = mock_discord(app).await;

        let bot_guilds = vec![GuildId::new(1), GuildId::new(2), GuildId::new(3)];
        let deadline = tokio::time::Instant::now() + Duration::from_millis(60);
        let err = legacy_guilds_user_manages_via(&http, &bot_guilds, UserId::new(42), deadline)
            .await
            .unwrap_err();
        server.abort();

        let resp = err.into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_text = std::str::from_utf8(&body).unwrap();
        assert!(
            body_text.contains("sign out and sign in again"),
            "expected actionable reauth message, got: {body_text}"
        );
    }

    fn mock_partial_guild_json(id: u64, admin_role_id: u64) -> serde_json::Value {
        serde_json::json!({
            "id": id.to_string(),
            "name": format!("Guild {id}"),
            "icon": null,
            "splash": null,
            "discovery_splash": null,
            "owner_id": "999",
            "afk_timeout": 300,
            "verification_level": 0,
            "default_message_notifications": 0,
            "explicit_content_filter": 0,
            "roles": [{
                "id": admin_role_id.to_string(),
                "guild_id": id.to_string(),
                "color": 0,
                "colors": {"primary_color": 0},
                "hoist": false,
                "managed": false,
                "mentionable": false,
                "name": "Admin",
                "permissions": Permissions::ADMINISTRATOR.bits().to_string(),
                "position": 1,
                "tags": {}
            }],
            "emojis": [],
            "features": [],
            "mfa_level": 0,
            "system_channel_flags": 0,
            "premium_tier": 0,
            "preferred_locale": "en-US",
            "nsfw_level": 0,
            "stickers": [],
            "premium_progress_bar_enabled": false
        })
    }

    fn mock_member_json(user_id: u64, role_id: u64) -> serde_json::Value {
        serde_json::json!({
            "user": {
                "id": user_id.to_string(),
                "username": "admin",
                "discriminator": "0",
                "avatar": null,
                "bot": false
            },
            "roles": [role_id.to_string()],
            "joined_at": "2024-01-01T00:00:00Z",
            "deaf": false,
            "mute": false,
            "flags": 0
        })
    }

    fn mock_text_channel_json(channel_id: u64, guild_id: u64) -> serde_json::Value {
        serde_json::json!({
            "id": channel_id.to_string(),
            "type": 0,
            "guild_id": guild_id.to_string(),
            "position": 0,
            "permission_overwrites": [],
            "name": "alerts",
            "nsfw": false
        })
    }

    #[tokio::test]
    async fn legacy_member_fetch_timeout_returns_reauth_instead_of_empty_list() {
        let app = axum::Router::new()
            .route(
                "/api/v10/guilds/1",
                axum::routing::get(|| async { axum::Json(mock_partial_guild_json(1, 10)) }),
            )
            .route(
                "/api/v10/guilds/1/members/42",
                axum::routing::get(|| async {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    axum::Json(mock_member_json(42, 10))
                }),
            );
        let (http, server) = mock_discord(app).await;

        let err = legacy_guilds_user_manages_via(
            &http,
            &[GuildId::new(1)],
            UserId::new(42),
            tokio::time::Instant::now() + Duration::from_millis(60),
        )
        .await
        .unwrap_err();
        server.abort();
        assert_eq!(
            err.into_response().status(),
            axum::http::StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn legacy_writable_discovery_completes_within_budget_and_fails_when_channel_fetch_exceeds_budget()
     {
        let app = axum::Router::new()
            .route(
                "/api/v10/guilds/1",
                axum::routing::get(|| async {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    axum::Json(mock_partial_guild_json(1, 10))
                }),
            )
            .route(
                "/api/v10/guilds/1/members/42",
                axum::routing::get(|| async {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    axum::Json(mock_member_json(42, 10))
                }),
            )
            .route(
                "/api/v10/guilds/1/members/1000",
                axum::routing::get(|| async {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    axum::Json(mock_member_json(1000, 10))
                }),
            )
            .route(
                "/api/v10/guilds/1/channels",
                axum::routing::get(|| async {
                    tokio::time::sleep(Duration::from_millis(80)).await;
                    axum::Json(vec![mock_text_channel_json(500, 1)])
                }),
            );
        let (http, server) = mock_discord(app).await;

        // With enough budget, slow calls complete and return the writable guild + channel.
        let generous = tokio::time::Instant::now() + Duration::from_secs(2);
        let partials =
            legacy_guilds_user_manages_via(&http, &[GuildId::new(1)], UserId::new(42), generous)
                .await
                .unwrap();
        assert_eq!(partials.len(), 1);
        let writable = writable_guilds_from_partials_via(
            &http,
            partials.clone(),
            UserId::new(1000),
            Some(generous),
        )
        .await
        .unwrap();
        assert_eq!(writable.len(), 1);
        assert_eq!(writable[0].channels.len(), 1);
        assert_eq!(writable[0].channels[0].name, "alerts");

        // When the channel fetch exceeds the remaining budget, return the reauth BadRequest.
        let tight = tokio::time::Instant::now() + Duration::from_millis(55);
        let err =
            writable_guilds_from_partials_via(&http, partials, UserId::new(1000), Some(tight))
                .await
                .unwrap_err();
        server.abort();
        assert_eq!(
            err.into_response().status(),
            axum::http::StatusCode::BAD_REQUEST
        );
    }
}
