use crate::EventType;
use crate::discord::ffxiv::helpers::{
    discord_locale_to_xiv_language, localized_item_name, name_matches_lowered_ascii,
};
use itertools::Itertools;
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
        "remove_undercut_alert"
    )
)]
pub(crate) async fn retainer(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("Retainers")
                .description(
                    "Manage retainers tied to your verified FFXIV character.\n\n\
                     **Setup:**\n\
                     1. Verify your character at https://ultros.app\n\
                     2. `/ffxiv retainer add` — claim one of your retainers\n\
                     3. `/ffxiv retainer add_undercut_alert` — get alerts in this channel\n\n\
                     **See also:** `/ffxiv retainer list`, `check_listings`, `check_undercuts`.",
                ),
        ),
    )
    .await?;
    Ok(())
}

/// Returns the users retainers
#[poise::command(slash_command, prefix_command)]
async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let retainers = ctx
        .data()
        .db
        .get_retainer_listings_for_discord_user(ctx.author().id.get())
        .await?;
    let embed_text = retainers.into_iter().format_with("\n", |(_r, d, l), f| {
        f(&format_args!("{} - {} listings", d.name, l.len()))
    });
    let embed_text = format!(
        "Use `/ffxiv retainer add ` to add more retainers to your list. Or check your undercuts with `/ffxiv retainer undercuts`\n\n```\n{embed_text}\n```"
    );
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("Retainers")
                .description(embed_text),
        ),
    )
    .await?;
    Ok(())
}

async fn autocomplete_retainer_id(
    ctx: Context<'_>,
    partial: &str,
) -> impl Iterator<Item = poise::serenity_prelude::AutocompleteChoice> {
    let world_cache = ctx.data().world_cache.clone();
    let partial = partial.to_ascii_lowercase();
    ctx.data()
        .db
        .get_retainers_for_user_characters(ctx.author().id.get())
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(move |retainer| retainer.name.to_ascii_lowercase().contains(&partial))
        .flat_map(move |retainer| {
            Some(poise::serenity_prelude::AutocompleteChoice::new(
                format!(
                    "{} - {}",
                    retainer.name,
                    world_cache
                        .lookup_selector(&AnySelector::World(retainer.world_id))
                        .ok()?
                        .get_name()
                ),
                retainer.id,
            ))
        })
}

#[poise::command(slash_command)]
async fn add_undercut_alert(
    ctx: Context<'_>,
    #[description = "Margin to send an alert for. Range 0-200. 0 = 0%, and will always notify for undercuts."]
    margin_percent: i32,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let alert = ctx
        .data()
        .db
        .add_discord_retainer_alert(
            ctx.channel_id().get() as i64,
            ctx.author().id.get() as i64,
            margin_percent,
        )
        .await?;
    ctx.data()
        .event_senders
        .retainer_undercut
        .send(EventType::added(alert))?;
    ctx.say(&format!("Now sending alerts to this channel anytime someone undercuts your retainer by {margin_percent}%")).await?;
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

/// Adds a retainer to your profile (requires a verified FFXIV character)
#[poise::command(slash_command)]
async fn add(
    ctx: Context<'_>,
    #[description = "Retainer name"]
    #[autocomplete = "autocomplete_retainer_id"]
    retainer_id: i32,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let claimable = ctx
        .data()
        .db
        .get_retainers_for_user_characters(ctx.author().id.get())
        .await?;
    if !claimable.iter().any(|r| r.id == retainer_id) {
        // claimable may be empty for two reasons: the user has no verified
        // characters at all, or they have characters but the retainer isn't on
        // any of those characters' worlds. Disambiguate only on the error path
        // so we can show the right next-step text.
        let has_characters = !ctx
            .data()
            .db
            .get_all_characters_for_discord_user(ctx.author().id.get() as i64)
            .await?
            .is_empty();
        let (title, description) = if !has_characters {
            (
                "Verify a character first",
                "Claiming a retainer requires a verified FFXIV character. \
                 Visit https://ultros.app and link your character via the \
                 Lodestone challenge, then come back and run this command.",
            )
        } else {
            (
                "Retainer not claimable",
                "That retainer doesn't belong to any of your verified characters. \
                 If this is your retainer, make sure the character it belongs to \
                 is verified on ultros.app.",
            )
        };
        ctx.send(
            poise::CreateReply::default().embed(
                poise::serenity_prelude::CreateEmbed::new()
                    .title(title)
                    .description(description)
                    .color(Color::from_rgb(200, 80, 80)),
            ),
        )
        .await?;
        return Ok(());
    }
    let _register_retainer = ctx
        .data()
        .db
        .register_retainer(
            retainer_id,
            ctx.author().id.get(),
            ctx.author().name.clone(),
        )
        .await?;
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("Added retainer")
                .description(
                    "Retainer claimed! Run `/ffxiv retainer check_listings` to see your \
                     active listings, or `/ffxiv retainer add_undercut_alert` to start \
                     receiving undercut alerts in this channel.",
                )
                .color(Color::from_rgb(123, 0, 123)),
        ),
    )
    .await?;
    Ok(())
}

async fn owned_retainer_auto_complete(
    ctx: Context<'_>,
    partial: &str,
) -> impl Iterator<Item = poise::serenity_prelude::AutocompleteChoice> {
    let partial = partial.to_ascii_lowercase();
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
        .flat_map(|(owned, retainer)| {
            let retainer = retainer?;
            Some(poise::serenity_prelude::AutocompleteChoice::new(
                retainer.name,
                owned.id,
            ))
        })
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
