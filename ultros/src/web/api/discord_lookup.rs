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

use poise::serenity_prelude::{
    self as serenity, ChannelId, ChannelType, GuildId, Permissions, UserId,
};
use ultros_api_types::alert::{DiscordWritableChannel, DiscordWritableGuild};

use crate::web::error::ApiError;

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
/// Computation is intentionally manual — we fetch the guild + member and walk
/// the role list because serenity's cache-based [`serenity::Member::permissions`]
/// requires a populated cache, which is not guaranteed for the user's guild
/// when they are creating an endpoint via the web UI.
pub(crate) async fn require_user_is_guild_admin(
    ctx: &serenity::Context,
    guild_id: i64,
    user_id: i64,
) -> Result<(), ApiError> {
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
        return Ok(());
    }

    let member = guild
        .member(&ctx.http, UserId::new(user_id))
        .await
        .map_err(|e| {
            ApiError::from(anyhow::anyhow!(
                "you do not appear to be a member of that Discord server (guild lookup failed: {e})"
            ))
        })?;

    let mut perms = Permissions::empty();
    for role_id in member.roles.iter() {
        if let Some(role) = partial.roles.get(role_id) {
            perms |= role.permissions;
        }
    }

    if perms.contains(Permissions::ADMINISTRATOR) || perms.contains(Permissions::MANAGE_GUILD) {
        Ok(())
    } else {
        Err(ApiError::from(anyhow::anyhow!(
            "you must have Administrator or Manage Server permission in that Discord \
             server to bind alerts to its channels"
        )))
    }
}

/// Return guilds that:
///
/// - the bot is in,
/// - the authenticated web user is a member of,
/// - the authenticated web user can administer or manage,
/// - and the bot can post embeds into at least one text/news channel.
///
/// This powers the web "Discord channel" endpoint picker. It intentionally
/// uses the bot token only: the user's OAuth session currently has `identify`,
/// not `guilds`, and the bot can answer the shared-guild question by probing
/// its own guilds for the user member.
pub(crate) async fn writable_guilds_for_user(
    ctx: &serenity::Context,
    user_id: i64,
) -> Result<Vec<DiscordWritableGuild>, ApiError> {
    let user_id =
        u64::try_from(user_id).map_err(|_| ApiError::from(anyhow::anyhow!("invalid user_id")))?;
    let user_id = UserId::new(user_id);
    let bot_user_id = ctx.cache.current_user().id;
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
