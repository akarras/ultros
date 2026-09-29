use anyhow::anyhow;
use itertools::Itertools;
use poise::CreateReply;
use poise::serenity_prelude::CreateEmbed;
use ultros_api_types::alert::{
    BACK_IN_STOCK_MIN_EMPTY_SECS, BELOW_MEDIAN_MIN_SAMPLES, BELOW_MEDIAN_PERCENT_RANGE,
};
use ultros_api_types::list::ListPermission;
use ultros_db::NewMarketTriggerAlert;

use crate::discord::ffxiv::helpers;
use crate::discord::ffxiv::helpers::{discord_locale_to_xiv_language, localized_item_name};
use crate::event::EventType;

use super::{Context, Error, ULTROS_COLOR};

/// Manage price alerts and delivery endpoints.
//
// Discord slash commands support at most two levels of nesting (command > group >
// subcommand). `ffxiv > alert` already consumes both levels, so the inner `endpoint`
// group cannot be a subcommand group of its own — Discord rejects registration with
// `Invalid Form Body (... .options.N: Value of field "type" must be one of (1,).)`.
// We flatten the former `endpoint` group into `endpoint-list`/`endpoint-remove`/
// `endpoint-here` siblings here.
#[poise::command(
    slash_command,
    prefix_command,
    subcommands(
        "price",
        "below_median",
        "back_in_stock",
        "list",
        "list_subscribe",
        "list_updates",
        "mute",
        "unmute",
        "remove",
        "endpoint_list",
        "endpoint_remove",
        "endpoint_here",
        "webhook"
    )
)]
pub(crate) async fn alert(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(
        "Use one of: `price`, `below-median`, `back-in-stock`, `list`, `list-subscribe`, `list-updates`, `mute`, `unmute`, `remove`, `endpoint-list`, `endpoint-remove`, `endpoint-here`, `webhook`.\n\
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
    ctx.data()
        .event_senders
        .alerts
        .send(EventType::added(alert.clone()))?;

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

/// Alert when a listing is at least `percent`% under the item's 30-day median.
#[poise::command(slash_command, prefix_command, rename = "below-median")]
async fn below_median(
    ctx: Context<'_>,
    #[description = "Item name"]
    #[autocomplete = "helpers::autocomplete_item"]
    item: String,
    #[description = "Alert when a listing is at least this % below the 30-day median (5-90)"]
    percent: i32,
    #[description = "Only match HQ listings (default: any)"] hq: Option<bool>,
    #[description = "World/DC/region to watch (default: your home world)"] world: Option<String>,
    #[description = "Min seconds between repeats (60-86400, default 3600)"] cooldown: Option<i32>,
) -> Result<(), Error> {
    if !BELOW_MEDIAN_PERCENT_RANGE.contains(&percent) {
        ctx.say(format!(
            "Percent must be between {} and {}.",
            BELOW_MEDIAN_PERCENT_RANGE.start(),
            BELOW_MEDIAN_PERCENT_RANGE.end()
        ))
        .await?;
        return Ok(());
    }
    let hq_only = hq.unwrap_or(false);
    let alert = create_market_alert(&ctx, &item, world, hq_only, cooldown, Some(percent)).await?;
    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .color(ULTROS_COLOR)
                .title("Below-median alert created")
                .description(format!(
                    "**{item}** at least {percent}% below its 30-day median{hq_str}. Alert id: `{id}`.\n\
                     Items with fewer than {min} recent sales are skipped until they have more history.\n\
                     Delivery: Discord DM to you.\nCooldown: {cooldown}s.",
                    hq_str = if hq_only { " (HQ only)" } else { "" },
                    min = BELOW_MEDIAN_MIN_SAMPLES,
                    id = alert.id,
                    cooldown = alert.cooldown_seconds,
                )),
        ),
    )
    .await?;
    Ok(())
}

/// Alert when an item that has been sold out for a while is listed again.
#[poise::command(slash_command, prefix_command, rename = "back-in-stock")]
async fn back_in_stock(
    ctx: Context<'_>,
    #[description = "Item name"]
    #[autocomplete = "helpers::autocomplete_item"]
    item: String,
    #[description = "Only count HQ listings (default: any)"] hq: Option<bool>,
    #[description = "World/DC/region to watch (default: your home world)"] world: Option<String>,
    #[description = "Min seconds between repeats (60-86400, default 3600)"] cooldown: Option<i32>,
) -> Result<(), Error> {
    let hq_only = hq.unwrap_or(false);
    let alert = create_market_alert(&ctx, &item, world, hq_only, cooldown, None).await?;
    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .color(ULTROS_COLOR)
                .title("Back-in-stock alert created")
                .description(format!(
                    "**{item}**{hq_str} coming back after at least {mins} minutes with no listings. \
                     Alert id: `{id}`.\nDelivery: Discord DM to you.\nCooldown: {cooldown}s.",
                    hq_str = if hq_only { " (HQ)" } else { "" },
                    mins = BACK_IN_STOCK_MIN_EMPTY_SECS / 60,
                    id = alert.id,
                    cooldown = alert.cooldown_seconds,
                )),
        ),
    )
    .await?;
    Ok(())
}

/// Shared body of `below-median` / `back-in-stock`: resolve the item and
/// scope, bind the caller's DM endpoint, create the alert (`percent_below`
/// picks the variant), and wake the market-trigger listener.
async fn create_market_alert(
    ctx: &Context<'_>,
    item: &str,
    world: Option<String>,
    hq_only: bool,
    cooldown: Option<i32>,
    percent_below: Option<i32>,
) -> Result<ultros_db::entity::alert::Model, Error> {
    let owner = ctx.author().id.get() as i64;
    let item_id = helpers::resolve_item_id(item).ok_or_else(|| anyhow!("unknown item: {item}"))?;
    let world_selector = match world {
        Some(s) => helpers::parse_world_selector(ctx, &s).await?,
        None => helpers::user_home_world_selector(ctx).await?,
    };
    let dm_endpoint = ctx
        .data()
        .db
        .get_or_create_dm_endpoint(owner, &format!("DM to {}", ctx.author().name))
        .await?;
    let endpoint_ids = [dm_endpoint];
    let new = NewMarketTriggerAlert {
        owner,
        item_id,
        world_selector_json: serde_json::to_value(world_selector)?,
        hq_only,
        cooldown_seconds: cooldown.unwrap_or(3600).clamp(60, 86400),
        endpoint_ids: &endpoint_ids,
    };
    let alert = match percent_below {
        Some(percent) => {
            ctx.data()
                .db
                .create_below_median_alert(new, percent)
                .await?
                .0
        }
        None => ctx.data().db.create_back_in_stock_alert(new).await?.0,
    };
    ctx.data()
        .event_senders
        .alerts
        .send(EventType::added(alert.clone()))?;
    Ok(alert)
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
    ctx.data()
        .event_senders
        .alerts
        .send(EventType::added(alert.clone()))?;

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

/// Subscribe to list edits; fires when a shared list or one of its rows changes.
#[poise::command(slash_command, prefix_command, rename = "list-updates")]
async fn list_updates(
    ctx: Context<'_>,
    #[description = "List id (find it on the list page URL)"] list_id: i32,
    #[description = "Min seconds between repeats (60-86400, default 3600)"] cooldown: Option<i32>,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
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
        .create_list_update_alert(owner, list_id, cooldown, &[dm_endpoint])
        .await?;
    ctx.data()
        .event_senders
        .alerts
        .send(EventType::added(alert.clone()))?;

    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .color(ULTROS_COLOR)
                .title("List update subscription created")
                .description(format!(
                    "Watching list `#{list_id}` for list edits and item changes. \
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
    let list_threshold_rows = ctx.data().db.get_user_list_threshold_alerts(owner).await?;
    let list_update_rows = ctx.data().db.get_user_list_update_alerts(owner).await?;
    let retainer_rows = ctx
        .data()
        .db
        .get_user_retainer_undercut_alerts(owner)
        .await?;
    let median_rows = ctx.data().db.get_user_below_median_alerts(owner).await?;
    let stock_rows = ctx.data().db.get_user_back_in_stock_alerts(owner).await?;
    if rows.is_empty()
        && list_threshold_rows.is_empty()
        && list_update_rows.is_empty()
        && retainer_rows.is_empty()
        && median_rows.is_empty()
        && stock_rows.is_empty()
    {
        ctx.say("You have no alerts. Create one with `/ffxiv alert price`.")
            .await?;
        return Ok(());
    }
    let user_lang = discord_locale_to_xiv_language(ctx.locale());
    let mut lines = rows
        .into_iter()
        .map(|(a, t)| {
            let item_name = localized_item_name(t.item_id, user_lang);
            let item_name = if item_name.is_empty() {
                "?".to_string()
            } else {
                item_name
            };
            let status = if a.enabled { "✅" } else { "⏸" };
            format!(
                "{status} `#{id}` {name} ≤ {price} gil{hq}",
                id = a.id,
                name = item_name,
                price = t.price_threshold,
                hq = if t.hq_only { " (HQ)" } else { "" },
            )
        })
        .collect::<Vec<_>>();
    lines.extend(list_threshold_rows.into_iter().map(|(a, t)| {
        let status = if a.enabled { "✅" } else { "⏸" };
        format!(
            "{status} `#{}` list #{} target-price subscription",
            a.id, t.list_id
        )
    }));
    lines.extend(list_update_rows.into_iter().map(|(a, t)| {
        let status = if a.enabled { "✅" } else { "⏸" };
        format!(
            "{status} `#{}` list #{} update subscription",
            a.id, t.list_id
        )
    }));
    lines.extend(retainer_rows.into_iter().map(|(a, t)| {
        let status = if a.enabled { "✅" } else { "⏸" };
        format!(
            "{status} `#{}` retainer undercut alerts over {}%",
            a.id, t.margin_percent
        )
    }));
    let item_label = |item_id: i32| {
        let name = localized_item_name(item_id, user_lang);
        if name.is_empty() {
            "?".to_string()
        } else {
            name
        }
    };
    lines.extend(median_rows.into_iter().map(|(a, t)| {
        let status = if a.enabled { "✅" } else { "⏸" };
        format!(
            "{status} `#{}` {} ≥ {}% below 30-day median{}",
            a.id,
            item_label(t.item_id),
            t.percent_below,
            if t.hq_only { " (HQ)" } else { "" },
        )
    }));
    lines.extend(stock_rows.into_iter().map(|(a, t)| {
        let status = if a.enabled { "✅" } else { "⏸" };
        format!(
            "{status} `#{}` {} back in stock{}",
            a.id,
            item_label(t.item_id),
            if t.hq_only { " (HQ)" } else { "" },
        )
    }));
    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .color(ULTROS_COLOR)
                .title("Your alerts")
                .description(lines.into_iter().join("\n")),
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
    if let Some(alert) = ctx.data().db.get_alert(id).await? {
        ctx.data()
            .event_senders
            .alerts
            .send(EventType::updated(alert))?;
    }
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
    if let Some(alert) = ctx.data().db.get_alert(id).await? {
        ctx.data()
            .event_senders
            .alerts
            .send(EventType::updated(alert))?;
    }
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
    let alert = ctx
        .data()
        .db
        .get_alert(id)
        .await?
        .ok_or_else(|| anyhow!("alert not found"))?;
    let undercuts = ctx
        .data()
        .db
        .get_retainer_alerts_for_related_alert_id(id)
        .await?;
    ctx.data().db.delete_alert_owned_by(owner, id).await?;
    ctx.data()
        .event_senders
        .alerts
        .send(EventType::removed(alert))?;
    for undercut in undercuts {
        ctx.data()
            .event_senders
            .retainer_undercut
            .send(EventType::removed(undercut))?;
    }
    ctx.say(format!("Alert `#{id}` deleted.")).await?;
    Ok(())
}

// ----- endpoint flat subcommands -----
// Previously these lived under a nested `endpoint` subcommand group, but Discord
// doesn't allow a subcommand group inside a subcommand group (see comment on
// `alert` above). They're flat siblings of the other alert subcommands now.

/// List your delivery endpoints.
#[poise::command(slash_command, prefix_command, rename = "endpoint-list")]
async fn endpoint_list(ctx: Context<'_>) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    let endpoints = ctx.data().db.list_endpoints(owner).await?;
    if endpoints.is_empty() {
        ctx.say(
            "You have no endpoints. Create one with `/ffxiv alert endpoint-here` or \
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
#[poise::command(slash_command, prefix_command, rename = "endpoint-remove")]
async fn endpoint_remove(
    ctx: Context<'_>,
    #[description = "Endpoint id (see `/ffxiv alert endpoint-list`)"] id: i32,
) -> Result<(), Error> {
    let owner = ctx.author().id.get() as i64;
    ctx.data().db.delete_endpoint(owner, id).await?;
    ctx.say(format!("Endpoint `#{id}` deleted.")).await?;
    Ok(())
}

/// Register the current channel as a delivery endpoint and bind it to one of your alerts.
#[poise::command(slash_command, prefix_command, rename = "endpoint-here")]
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
             Use `/ffxiv alert endpoint-here bind_to:<alert_id>` to attach it to an alert."
        ))
        .await?;
    }
    Ok(())
}
