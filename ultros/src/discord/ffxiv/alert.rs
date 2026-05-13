use anyhow::anyhow;
use itertools::Itertools;
use poise::CreateReply;
use poise::serenity_prelude::CreateEmbed;
use ultros_api_types::list::ListPermission;
use xiv_gen::ItemId;

use crate::discord::ffxiv::helpers;

use super::{Context, Error, ULTROS_COLOR};

/// Manage price alerts and delivery endpoints.
#[poise::command(
    slash_command,
    prefix_command,
    subcommands(
        "price",
        "list",
        "list_subscribe",
        "mute",
        "unmute",
        "remove",
        "endpoint",
        "webhook"
    )
)]
pub(crate) async fn alert(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(
        "Use one of: `price`, `list`, `list-subscribe`, `mute`, `unmute`, `remove`, `endpoint`, `webhook`.\n\
         e.g. `/ffxiv alert price item:Tsai_tou_Vounou price:50000`",
    )
    .await?;
    Ok(())
}

/// Create a price alert. Sends a Discord DM when the item is listed below the threshold.
#[poise::command(slash_command, prefix_command)]
async fn price(
    ctx: Context<'_>,
    #[description = "Item name"]
    #[autocomplete = "helpers::autocomplete_item"]
    item: String,
    #[description = "Alert when price ≤ this gil"] price: i32,
    #[description = "Only match HQ listings (default: any)"] hq: Option<bool>,
    #[description = "World/DC/region to watch (default: your home world)"] world: Option<String>,
    #[description = "Min seconds between repeats (60-86400, default 3600)"] cooldown: Option<i32>,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    if price <= 0 {
        ctx.say("Price must be positive.").await?;
        return Ok(());
    }

    // Resolve item name → item_id via the existing helper.
    let item_id = helpers::resolve_item_id(&item).ok_or_else(|| anyhow!("unknown item: {item}"))?;

    // Resolve world arg → AnySelector. If absent, use the user's home world; if no home
    // world is known, error out.
    let world_selector = match world {
        Some(s) => helpers::parse_world_selector(&ctx, &s).await?,
        None => helpers::user_home_world_selector(&ctx).await?,
    };

    let world_json = serde_json::to_value(world_selector)?;
    let cooldown = cooldown.unwrap_or(3600).clamp(60, 86400);

    // Auto-create the user's DM endpoint if missing.
    let dm_endpoint = ctx
        .data()
        .db
        .get_or_create_dm_endpoint(owner, &format!("DM to {}", ctx.author().name))
        .await?;

    // Create the alert (no inline endpoint) and bind the rule.
    let alert = ctx
        .data()
        .db
        .create_threshold_alert_without_endpoint(
            owner,
            item_id,
            world_json,
            price,
            hq.unwrap_or(false),
            cooldown,
        )
        .await?;
    ctx.data()
        .db
        .set_alert_rules(owner, alert.id, &[dm_endpoint])
        .await?;

    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .color(ULTROS_COLOR)
                .title("Price alert created")
                .description(format!(
                    "**{item}** ≤ {price} gil{hq_str}. Alert id: `{id}`.\nDelivery: Discord DM to you.\nCooldown: {cooldown}s.",
                    hq_str = if hq.unwrap_or(false) { " (HQ only)" } else { "" },
                    id = alert.id,
                )),
        ),
    )
    .await?;
    Ok(())
}

// Mirrors the web "Notify me on this list" flow — fires per-item when a list
// row drops to or below its target_price.
/// Subscribe to a list; fires when any item drops to its target price.
#[poise::command(slash_command, prefix_command, rename = "list-subscribe")]
async fn list_subscribe(
    ctx: Context<'_>,
    #[description = "List id (find it on the list page URL)"] list_id: i32,
    #[description = "Min seconds between repeats (60-86400, default 3600)"] cooldown: Option<i32>,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;

    // Same permission gate as the web `create_alert` handler — Read or higher
    // is enough since shared lists are explicitly subscribable.
    let permission = ctx.data().db.get_permission(list_id, owner).await?;
    if permission < ListPermission::Read {
        return Err(anyhow!(
            "you don't have access to list {list_id}; ask the owner to share it with you"
        )
        .into());
    }

    let dm_endpoint = ctx
        .data()
        .db
        .get_or_create_dm_endpoint(owner, &format!("DM to {}", ctx.author().name))
        .await?;
    let cooldown = cooldown.unwrap_or(3600).clamp(60, 86400);
    let alert = ctx
        .data()
        .db
        .create_list_threshold_alert(owner, list_id, cooldown, &[dm_endpoint])
        .await?;

    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .color(ULTROS_COLOR)
                .title("List subscription created")
                .description(format!(
                    "Watching list `#{list_id}` for items dropping to their target price. \
                     Alert id: `{}`.\nDelivery: Discord DM to you.\nCooldown: {cooldown}s.",
                    alert.id,
                )),
        ),
    )
    .await?;
    Ok(())
}

// Mirrors the web "Add endpoint → Webhook URL" flow.
/// Register a Discord webhook URL as a delivery endpoint.
#[poise::command(slash_command, prefix_command)]
async fn webhook(
    ctx: Context<'_>,
    #[description = "Discord webhook URL (https://discord.com/api/webhooks/...)"] url: String,
    #[description = "Friendly name for this endpoint"] name: Option<String>,
    #[description = "Alert id to also bind this webhook to (optional)"] bind_to: Option<i32>,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;

    // Light validation — full host/path checking lives in the web handler so we don't
    // duplicate the constants. We at least require the obvious Discord shape so users
    // don't bind a generic HTTP endpoint by accident.
    if !url.starts_with("https://discord.com/api/webhooks/")
        && !url.starts_with("https://discordapp.com/api/webhooks/")
        && !url.starts_with("https://ptb.discord.com/api/webhooks/")
        && !url.starts_with("https://canary.discord.com/api/webhooks/")
    {
        return Err(anyhow!(
            "webhook URL must be a Discord webhook (https://discord.com/api/webhooks/...)"
        )
        .into());
    }

    let display_name = name.unwrap_or_else(|| {
        // Use the trailing path component as a label; falls back to "Webhook".
        url.rsplit('/')
            .find(|s| !s.is_empty())
            .map(|tail| format!("Webhook {tail}"))
            .unwrap_or_else(|| "Webhook".to_string())
    });
    let endpoint_id = ctx
        .data()
        .db
        .create_endpoint(
            owner,
            &display_name,
            "Webhook",
            serde_json::json!({ "url": url }),
        )
        .await?;

    if let Some(alert_id) = bind_to {
        let mut current = ctx.data().db.list_endpoint_ids_for_alert(alert_id).await?;
        if !current.contains(&endpoint_id) {
            current.push(endpoint_id);
        }
        ctx.data()
            .db
            .set_alert_rules(owner, alert_id, &current)
            .await?;
        ctx.say(format!(
            "Webhook endpoint `#{endpoint_id}` registered and bound to alert `#{alert_id}`."
        ))
        .await?;
    } else {
        ctx.say(format!(
            "Webhook endpoint `#{endpoint_id}` registered. \
             Use `/ffxiv alert webhook url:<...> bind_to:<alert_id>` to attach it to an alert."
        ))
        .await?;
    }
    Ok(())
}

/// List your active alerts.
#[poise::command(slash_command, prefix_command)]
async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    let rows = ctx.data().db.get_user_threshold_alerts(owner).await?;
    if rows.is_empty() {
        ctx.say("You have no alerts. Create one with `/ffxiv alert price`.")
            .await?;
        return Ok(());
    }
    let items = &xiv_gen_db::data().items;
    let lines = rows
        .into_iter()
        .map(|(a, t)| {
            let item_name = items
                .get(&ItemId(t.item_id))
                .map(|it| it.name.as_str())
                .unwrap_or("?");
            let status = if a.enabled { "✅" } else { "⏸" };
            format!(
                "{status} `#{id}` {name} ≤ {price} gil{hq}",
                id = a.id,
                name = item_name,
                price = t.price_threshold,
                hq = if t.hq_only { " (HQ)" } else { "" },
            )
        })
        .join("\n");
    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .color(ULTROS_COLOR)
                .title("Your alerts")
                .description(lines),
        ),
    )
    .await?;
    Ok(())
}

/// Disable an alert. Use `unmute` to re-enable.
#[poise::command(slash_command, prefix_command)]
async fn mute(
    ctx: Context<'_>,
    #[description = "Alert id (see `/ffxiv alert list`)"] id: i32,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    ctx.data().db.set_alert_enabled(owner, id, false).await?;
    ctx.say(format!("Alert `#{id}` muted.")).await?;
    Ok(())
}

/// Re-enable an alert.
#[poise::command(slash_command, prefix_command)]
async fn unmute(
    ctx: Context<'_>,
    #[description = "Alert id (see `/ffxiv alert list`)"] id: i32,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    ctx.data().db.set_alert_enabled(owner, id, true).await?;
    ctx.say(format!("Alert `#{id}` unmuted.")).await?;
    Ok(())
}

/// Delete an alert.
#[poise::command(slash_command, prefix_command)]
async fn remove(
    ctx: Context<'_>,
    #[description = "Alert id (see `/ffxiv alert list`)"] id: i32,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    ctx.data().db.delete_alert_owned_by(owner, id).await?;
    ctx.say(format!("Alert `#{id}` deleted.")).await?;
    Ok(())
}

// ----- endpoint sub-subcommand -----

/// Manage delivery endpoints used by your alerts.
#[poise::command(
    slash_command,
    prefix_command,
    subcommands("endpoint_list", "endpoint_remove", "endpoint_here")
)]
async fn endpoint(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Use one of: `list`, `remove`, `here`.").await?;
    Ok(())
}

/// List your delivery endpoints.
#[poise::command(slash_command, prefix_command, rename = "list")]
async fn endpoint_list(ctx: Context<'_>) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    let endpoints = ctx.data().db.list_endpoints(owner).await?;
    if endpoints.is_empty() {
        ctx.say(
            "You have no endpoints. Create one with `/ffxiv alert endpoint here` or \
             `/ffxiv alert price` (which creates a DM endpoint automatically).",
        )
        .await?;
        return Ok(());
    }
    let lines = endpoints
        .iter()
        .map(|e| {
            let kind = match e.method.as_str() {
                "DiscordDm" => "DM",
                "DiscordChannel" => "channel",
                "Webhook" => "webhook",
                other => other,
            };
            format!("`#{}` [{kind}] {}", e.id, e.name)
        })
        .join("\n");
    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .color(ULTROS_COLOR)
                .title("Your endpoints")
                .description(lines),
        ),
    )
    .await?;
    Ok(())
}

/// Delete an endpoint (also unlinks it from any alert).
#[poise::command(slash_command, prefix_command, rename = "remove")]
async fn endpoint_remove(
    ctx: Context<'_>,
    #[description = "Endpoint id (see `/ffxiv alert endpoint list`)"] id: i32,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    ctx.data().db.delete_endpoint(owner, id).await?;
    ctx.say(format!("Endpoint `#{id}` deleted.")).await?;
    Ok(())
}

/// Register the current channel as a delivery endpoint and bind it to one of your alerts.
#[poise::command(slash_command, prefix_command, rename = "here")]
async fn endpoint_here(
    ctx: Context<'_>,
    #[description = "Alert id to also bind to this channel (optional)"] bind_to: Option<i32>,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    let channel_id_raw = ctx.channel_id();
    let channel_id = channel_id_raw.get() as i64;

    // Look up channel + guild metadata so the endpoint stores something more
    // descriptive than "Channel <numeric id>". Falls back gracefully if either
    // call returns nothing useful (DM channels have no guild, etc.).
    let serenity_ctx = ctx.serenity_context();
    let channel_meta = channel_id_raw
        .to_channel(&serenity_ctx.http)
        .await
        .ok()
        .and_then(|c| c.guild());
    let (channel_name, guild_id, guild_name) = match channel_meta {
        Some(gc) => {
            let gid = i64::try_from(gc.guild_id.get()).ok();
            let gname = gc.guild_id.name(&serenity_ctx.cache);
            (Some(gc.name), gid, gname)
        }
        None => (None, None, None),
    };
    let display_name = match (&channel_name, &guild_name) {
        (Some(cn), Some(gn)) => format!("#{cn} ({gn})"),
        (Some(cn), None) => format!("#{cn}"),
        _ => format!("Channel {channel_id_raw}"),
    };
    let endpoint_id = ctx
        .data()
        .db
        .get_or_create_channel_endpoint(
            owner,
            channel_id,
            &display_name,
            channel_name.as_deref(),
            guild_id,
            guild_name.as_deref(),
        )
        .await?;
    if let Some(alert_id) = bind_to {
        // Replace the rules so this channel is also included.
        let mut current = ctx.data().db.list_endpoint_ids_for_alert(alert_id).await?;
        if !current.contains(&endpoint_id) {
            current.push(endpoint_id);
        }
        ctx.data()
            .db
            .set_alert_rules(owner, alert_id, &current)
            .await?;
        ctx.say(format!(
            "Endpoint `#{endpoint_id}` registered for this channel and bound to alert `#{alert_id}`."
        ))
        .await?;
    } else {
        ctx.say(format!(
            "Endpoint `#{endpoint_id}` registered for this channel. \
             Use `/ffxiv alert endpoint here bind_to:<alert_id>` to attach it to an alert."
        ))
        .await?;
    }
    Ok(())
}
