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
    self as serenity, ChannelId, ChannelType, GuildId, GuildPagination, Http, Permissions, UserId,
};
use std::collections::HashSet;
use ultros_api_types::alert::{DiscordWritableChannel, DiscordWritableGuild};

use crate::web::error::ApiError;
use crate::web::oauth::AuthUserCache;

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
    if channel_id <= 0 {
        return Err(ApiError::from(anyhow::anyhow!(
            "channel_id must be positive"
        )));
    }
    let channel = ChannelId::new(channel_id as u64)
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
    let guild_id =
        u64::try_from(guild_id).map_err(|_| ApiError::from(anyhow::anyhow!("invalid guild_id")))?;
    let user_id =
        u64::try_from(user_id).map_err(|_| ApiError::from(anyhow::anyhow!("invalid user_id")))?;
    let guild = GuildId::new(guild_id);

    // Owner shortcut: skip the role walk, they always have everything. Fetching
    // the partial guild also surfaces "bot is not in the guild" with a clearer
    // message than the member fetch would.
    let partial = guild.to_partial_guild(&ctx.http).await.map_err(|e| {
        ApiError::from(anyhow::anyhow!(
            "Discord could not load guild {guild_id}: {e}. \
             The bot must be a member of the guild."
        ))
    })?;
    if partial.owner_id == UserId::new(user_id) {
        return Ok(partial);
    }

    let member = guild
        .member(&ctx.http, UserId::new(user_id))
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

/// Compatibility for sessions issued before we requested the `guilds` scope.
/// Keep this bot-wide scan only for Discord's explicit missing-access response.
async fn legacy_guilds_user_manages(
    ctx: &serenity::Context,
    user_id: UserId,
) -> Vec<serenity::PartialGuild> {
    let mut guilds = Vec::new();
    for guild_id in ctx.cache.guilds() {
        let partial = match guild_id.to_partial_guild(&ctx.http).await {
            Ok(guild) => guild,
            Err(e) => {
                tracing::warn!(
                    guild_id = guild_id.get(),
                    "failed to load Discord guild: {e}"
                );
                continue;
            }
        };

        // A miss here is the overwhelmingly common case (the user is not in
        // most of the bot's guilds), so it is not worth logging.
        let user_member = match partial.member(&ctx.http, user_id).await {
            Ok(member) => member,
            Err(_) => continue,
        };
        let user_permissions = partial.member_permissions(&user_member);
        if !user_permissions.contains(Permissions::ADMINISTRATOR)
            && !user_permissions.contains(Permissions::MANAGE_GUILD)
        {
            continue;
        }
        guilds.push(partial);
    }
    guilds
}

fn shared_manageable_guild(guild: &serenity::GuildInfo, bot_guilds: &HashSet<GuildId>) -> bool {
    bot_guilds.contains(&guild.id)
        && (guild.owner
            || guild
                .permissions
                .intersects(Permissions::ADMINISTRATOR | Permissions::MANAGE_GUILD))
}

/// Discover via the user's OAuth grant and intersect with the bot's guilds.
/// Guild mutations still independently check live bot-side permissions.
async fn guilds_user_manages(
    ctx: &serenity::Context,
    user_id: UserId,
    cookies: &PrivateCookieJar,
    cache: &AuthUserCache,
) -> Result<Vec<serenity::PartialGuild>, ApiError> {
    let token = cookies
        .get(crate::web::oauth::DISCORD_AUTH_COOKIE)
        .ok_or(ApiError::NoAuthCookie)?;
    let http = Http::new(&format!("Bearer {}", token.value()));
    let bot_guilds: HashSet<_> = ctx.cache.guilds().into_iter().collect();
    let Some(candidates) = oauth_manageable_guild_ids(&http, cookies, &bot_guilds, cache).await?
    else {
        return Ok(legacy_guilds_user_manages(ctx, user_id).await);
    };
    let mut guilds = Vec::new();
    for id in candidates {
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
                return Err(ApiError::DiscordTokenInvalid(cookies.clone()));
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
/// user can manage them. Unlike [`writable_guilds_for_user`] this does not
/// probe channels — a group has nothing to post, so requiring a bot-writable
/// channel would wrongly exclude valid servers (and the channel fetch is the
/// expensive part of that function).
pub(crate) async fn manageable_guilds_for_user(
    ctx: &serenity::Context,
    user_id: i64,
    cookies: &PrivateCookieJar,
    cache: &AuthUserCache,
) -> Result<Vec<(i64, String, Option<String>)>, ApiError> {
    let user_id =
        u64::try_from(user_id).map_err(|_| ApiError::from(anyhow::anyhow!("invalid user_id")))?;
    let mut guilds = guilds_user_manages(ctx, UserId::new(user_id), cookies, cache)
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
    guilds.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
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
pub(crate) async fn writable_guilds_for_user(
    ctx: &serenity::Context,
    user_id: i64,
    cookies: &PrivateCookieJar,
    cache: &AuthUserCache,
) -> Result<Vec<DiscordWritableGuild>, ApiError> {
    let user_id =
        u64::try_from(user_id).map_err(|_| ApiError::from(anyhow::anyhow!("invalid user_id")))?;
    let user_id = UserId::new(user_id);
    let bot_user_id = ctx.cache.current_user().id;
    let mut guilds = Vec::new();

    for partial in guilds_user_manages(ctx, user_id, cookies, cache).await? {
        let guild_id = partial.id;
        let bot_member = match partial.member(&ctx.http, bot_user_id).await {
            Ok(member) => member,
            Err(e) => {
                tracing::warn!(
                    guild_id = guild_id.get(),
                    "failed to load bot member for Discord guild: {e}"
                );
                continue;
            }
        };

        let mut channels = partial
            .channels(&ctx.http)
            .await
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
        channels.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        guilds.push(DiscordWritableGuild {
            id: partial.id.get() as i64,
            name: partial.name.clone(),
            icon_url: partial.icon_url(),
            channels,
        });
    }

    guilds.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(guilds)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
