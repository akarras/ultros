use crate::EventType;
use crate::ffxiv::helpers::{
    discord_locale_to_xiv_language, localized_item_name, name_matches_lowered_ascii, truncate_100,
};
use poise::serenity_prelude::Color;
use std::fmt::Write;
use ultros_db::{entity::active_listing, world_data::world_cache::AnySelector};

use super::{Context, Error};

#[poise::command(
    slash_command,
    prefix_command,
    subcommands(
        "list",
        "add",
        "remove",
        "check_listings",
        "check_undercuts",
        "add_undercut_alert",
        "remove_undercut_alert",
        "add_sale_alert",
        "remove_sale_alert"
    )
)]
pub(crate) async fn retainer(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("Retainers")
                .description(
                    "Track your retainers across every character and world.\n\n\
                     **Setup:**\n\
                     1. `/ffxiv retainer add` — search by name and world\n\
                     2. Optionally assign or add a character in the setup menu\n\
                     3. `/ffxiv retainer list` — organize, add more, or set up alerts\n\n\
                     **See also:** `/ffxiv retainer list`, `check_listings`, `check_undercuts`, `add_sale_alert`.",
                ),
        ),
    )
    .await?;
    Ok(())
}

/// View and organize your retainers by character and world
#[poise::command(slash_command, prefix_command)]
async fn list(ctx: Context<'_>) -> Result<(), Error> {
    super::retainer_setup::list(ctx).await
}

async fn autocomplete_retainer(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<poise::serenity_prelude::AutocompleteChoice> {
    let world = match ctx {
        poise::Context::Application(app) => app.args.iter().find_map(|argument| {
            if argument.name != "world" {
                return None;
            }
            let poise::serenity_prelude::ResolvedValue::String(name) = argument.value else {
                return None;
            };
            ctx.data()
                .world_cache
                .lookup_value_by_name(name)
                .ok()?
                .as_world()
                .ok()
                .map(|world| world.id)
        }),
        _ => None,
    };
    match ctx
        .data()
        .db
        .search_retainers_for_discord(partial, world)
        .await
    {
        Ok(retainers) => retainers
            .into_iter()
            .take(25)
            .map(|retainer| {
                let world = ctx
                    .data()
                    .world_cache
                    .lookup_selector(&AnySelector::World(retainer.world_id))
                    .map(|world| world.get_name().to_owned())
                    .unwrap_or_else(|_| format!("World {}", retainer.world_id));
                poise::serenity_prelude::AutocompleteChoice::new(
                    truncate_100(&format!("{} · {world}", retainer.name)),
                    format!("retainer:{}", retainer.id),
                )
            })
            .collect(),
        Err(error) => {
            tracing::warn!(
                ?error,
                "Discord retainer autocomplete failed; submitted searches can be retried"
            );
            vec![]
        }
    }
}

async fn autocomplete_world(ctx: Context<'_>, partial: &str) -> Vec<String> {
    let partial = partial.to_ascii_lowercase();
    let mut worlds: Vec<_> = ctx
        .data()
        .world_cache
        .get_all_worlds()
        .filter(|world| world.name.to_ascii_lowercase().contains(&partial))
        .map(|world| world.name.clone())
        .collect();
    worlds.sort();
    worlds.truncate(25);
    worlds
}

async fn autocomplete_character(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<poise::serenity_prelude::AutocompleteChoice> {
    let partial = partial.to_ascii_lowercase();
    match ctx
        .data()
        .db
        .get_all_characters_for_discord_user(ctx.author().id.get() as i64)
        .await
    {
        Ok(characters) => characters
            .into_iter()
            .filter_map(|(_, c)| c)
            .filter_map(|c| {
                let world = ctx
                    .data()
                    .world_cache
                    .lookup_selector(&AnySelector::World(c.world_id))
                    .ok()?;
                let name = format!("{} {} · {}", c.first_name, c.last_name, world.get_name());
                name.to_ascii_lowercase().contains(&partial).then(|| {
                    poise::serenity_prelude::AutocompleteChoice::new(truncate_100(&name), c.id)
                })
            })
            .take(25)
            .collect(),
        Err(error) => {
            tracing::warn!(?error, "Discord character autocomplete failed");
            vec![]
        }
    }
}

#[poise::command(slash_command)]
async fn add_undercut_alert(
    ctx: Context<'_>,
    #[description = "Margin to send an alert for. Range 0-200. 0 = 0%, and will always notify for undercuts."]
    margin_percent: i32,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if !(0..=200).contains(&margin_percent) {
        ctx.say("Margin percent must be between 0 and 200.").await?;
        return Ok(());
    }
    let alert = ctx
        .data()
        .db
        .add_discord_retainer_alert(
            ctx.channel_id().get() as i64,
            ctx.author().id.get() as i64,
            margin_percent,
        )
        .await?;
    let related_alert_id = alert.alert_id;
    ctx.data()
        .event_senders
        .retainer_undercut
        .send(EventType::added(alert))?;
    if let Some(related_alert) = ctx.data().db.get_alert(related_alert_id).await? {
        ctx.data()
            .event_senders
            .alerts
            .send(EventType::added(related_alert))?;
    }
    ctx.say(&format!("Now sending alerts to this channel for all your tracked retainers, across every character and world, anytime someone undercuts you by {margin_percent}%")).await?;
    Ok(())
}

#[poise::command(slash_command)]
async fn remove_undercut_alert(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    // find the alert for this channel and then delete it
    let channel_id = ctx.channel_id();
    let discord_id = ctx.author().id;
    let (alert, undercuts) = ctx
        .data()
        .db
        .delete_discord_alert(channel_id.get() as i64, discord_id.get() as i64)
        .await?;
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
    Ok(())
}

/// Notify this channel when one of your claimed retainers' listings sells
#[poise::command(slash_command)]
async fn add_sale_alert(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let alert = ctx
        .data()
        .db
        .add_discord_retainer_sale_alert(
            ctx.channel_id().get() as i64,
            ctx.author().id.get() as i64,
        )
        .await?;
    ctx.data()
        .event_senders
        .alerts
        .send(EventType::added(alert))?;
    ctx.say(
        "Now sending alerts to this channel when one of your tracked retainers' listings sells, across every character and world. \
         Sales are inferred from market data: a listing that disappears alongside a matching \
         sale. Same-price listings from other sellers are skipped rather than guessed, so \
         some sales may go unreported.",
    )
    .await?;
    Ok(())
}

/// Stop sale notifications in this channel
#[poise::command(slash_command)]
async fn remove_sale_alert(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let alert = ctx
        .data()
        .db
        .delete_discord_sale_alert(ctx.channel_id().get() as i64, ctx.author().id.get() as i64)
        .await?;
    ctx.data()
        .event_senders
        .alerts
        .send(EventType::removed(alert))?;
    ctx.say("Sale alerts for this channel removed.").await?;
    Ok(())
}

/// Shows only listings where your retainers listing has been undercut by someone else
#[poise::command(slash_command)]
async fn check_undercuts(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let user_id = ctx.author().id.get();
    let under_cut_items = ctx.data().db.get_retainer_undercut_items(user_id).await?;
    let user_lang = discord_locale_to_xiv_language(ctx.locale());
    let mut embeds = Vec::new();
    for (_, retainer, items) in &under_cut_items {
        if !items.is_empty() {
            let item = items.iter().fold(
                format!(
                    "```{:>30}{:>10}->{:<10}{}\n",
                    "name", "price", "target price", "behind"
                ),
                |mut s, (listing, undercut)| {
                    let item_name = localized_item_name(listing.item_id, user_lang);
                    let _ = writeln!(
                        s,
                        "{:>30} {:>10}->{:>10} {:>5}",
                        item_name,
                        listing.price_per_unit,
                        undercut.price_to_beat - 1,
                        undercut.number_behind
                    );
                    s
                },
            ) + "```";
            embeds.push(
                poise::serenity_prelude::CreateEmbed::new()
                    .title(&retainer.name)
                    .description(item)
                    .color(Color::from_rgb(123, 0, 123)),
            );
        }
    }

    let _content = if embeds.is_empty() {
        "No undercuts found!"
    } else {
        ""
    };
    let mut reply = poise::CreateReply::default().content(if under_cut_items.is_empty() {
        "No undercuts found!"
    } else {
        ""
    });
    for embed in embeds {
        reply = reply.embed(embed);
    }
    ctx.send(reply).await?;
    Ok(())
}

/// Returns a list of your retainers & all of their market board listings
#[poise::command(slash_command)]
async fn check_listings(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let retainers = ctx
        .data()
        .db
        .get_retainer_listings_for_discord_user(ctx.author().id.get())
        .await?;
    if retainers.is_empty() {
        ctx.say("No retainers found :(").await?;
    }
    let user_lang = discord_locale_to_xiv_language(ctx.locale());
    let embeds = retainers
        .into_iter()
        .map(|(_, retainer, listings)| {
            let mut msg_contents = String::new();
            msg_contents += "```";
            writeln!(
                msg_contents,
                "{:<30} {:>9} {:>4} {:>1}",
                "Item name", "price per item", "Qty.", "hq"
            )
            .unwrap();
            for listing in listings {
                let item_name = localized_item_name(listing.item_id, user_lang);
                let active_listing::Model {
                    price_per_unit,
                    quantity,
                    hq,
                    ..
                } = &listing;
                let hq = if *hq { '✅' } else { ' ' };
                writeln!(
                    msg_contents,
                    "{item_name:<30} {price_per_unit:>9} {quantity:<4} {hq}"
                )
                .unwrap();
            }
            msg_contents += "```";

            poise::serenity_prelude::CreateEmbed::new()
                .title(retainer.name)
                .description(msg_contents)
                .color(Color::from_rgb(123, 0, 123))
        })
        .collect::<Vec<_>>();

    let mut reply = poise::CreateReply::default();
    for embed in embeds {
        reply = reply.embed(embed);
    }
    ctx.send(reply).await?;
    Ok(())
}

/// Track a retainer on any world, then optionally assign a character
#[poise::command(slash_command)]
async fn add(
    ctx: Context<'_>,
    #[description = "Retainer name; leave blank to open setup"]
    #[autocomplete = "autocomplete_retainer"]
    #[max_length = 100]
    retainer: Option<String>,
    #[description = "Search this world, or leave blank for all worlds"]
    #[autocomplete = "autocomplete_world"]
    #[max_length = 100]
    world: Option<String>,
    #[description = "Optional character to group a new retainer under"]
    #[autocomplete = "autocomplete_character"]
    character: Option<i32>,
) -> Result<(), Error> {
    super::retainer_setup::add(ctx, retainer, world, character).await
}

async fn owned_retainer_auto_complete(
    ctx: Context<'_>,
    partial: &str,
) -> impl Iterator<Item = poise::serenity_prelude::AutocompleteChoice> {
    let partial = partial.to_ascii_lowercase();
    let worlds = ctx.data().world_cache.clone();
    ctx.data()
        .db
        .get_owned_retainers(ctx.author().id.get(), ctx.author().name.clone())
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(move |(_o, r)| {
            r.as_ref()
                .is_some_and(|retainer| name_matches_lowered_ascii(&retainer.name, &partial))
        })
        .flat_map(move |(owned, retainer)| {
            let retainer = retainer?;
            let world = worlds
                .lookup_selector(&AnySelector::World(retainer.world_id))
                .ok()?;
            Some(poise::serenity_prelude::AutocompleteChoice::new(
                truncate_100(&format!("{} · {}", retainer.name, world.get_name())),
                owned.id,
            ))
        })
        .take(25)
}

/// Removes a retainer from your profile
#[poise::command(slash_command)]
async fn remove(
    ctx: Context<'_>,
    #[autocomplete = "owned_retainer_auto_complete"]
    #[description = "Retainer to remove"]
    owned_retainer_id: i32,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let removed = ctx
        .data()
        .db
        .remove_owned_retainer(ctx.author().id.get(), owned_retainer_id)
        .await?;
    ctx.say("Removed retainer successfully!").await?;
    let _ = ctx
        .data()
        .event_senders
        .retainers
        .send(EventType::removed(removed));
    Ok(())
}
