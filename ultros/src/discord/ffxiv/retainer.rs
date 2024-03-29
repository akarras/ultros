use crate::EventType;
use itertools::Itertools;
use poise::serenity_prelude::Color;
use std::fmt::Write;
use ultros_db::{entity::active_listing, world_cache::AnySelector};
use xiv_gen::ItemId;

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
    ctx.say(
        "Use one of the subcommands `list`,
        `add`,
        `remove`,
        `check_listings`,
        `check_undercuts`,
        `add_undercut_alert`
        `remove_undercut_alert`",
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
        .get_retainer_listings_for_discord_user(ctx.author().id.0)
        .await?;
    let embed_text = retainers.into_iter().format_with("\n", |(_r, d, l), f| {
        f(&format_args!("{} - {} listings", d.name, l.len()))
    });
    let embed_text = format!("Use `/ffxiv retainer add ` to add more retainers to your list. Or check your undercuts with `/ffxiv retainer undercuts`\n\n```\n{embed_text}\n```");
    ctx.send(|reply| reply.embed(|e| e.title("Retainers").description(embed_text)))
        .await?;
    Ok(())
}

async fn autocomplete_retainer_id(
    ctx: Context<'_>,
    partial: &str,
) -> impl Iterator<Item = poise::AutocompleteChoice<i32>> {
    let world_cache = ctx.data().world_cache.clone();
    ctx.data()
        .db
        .search_retainers(partial)
        .await
        .unwrap_or_default()
        .into_iter()
        .flat_map(move |retainer| {
            Some(poise::AutocompleteChoice {
                name: format!(
                    "{} - {}",
                    retainer.name,
                    world_cache
                        .lookup_selector(&AnySelector::World(retainer.world_id))
                        .ok()?
                        .get_name()
                ),
                value: retainer.id,
            })
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
            ctx.channel_id().0 as i64,
            ctx.author().id.0 as i64,
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
        .delete_discord_alert(channel_id.0 as i64, discord_id.0 as i64)
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
    let user_id = ctx.author().id.0;
    let under_cut_items = ctx.data().db.get_retainer_undercut_items(user_id).await?;
    let data = xiv_gen_db::data();
    let item_db = &data.items;
    ctx.send(|r| {
        for (_, retainer, items) in &under_cut_items {
            if !items.is_empty() {
                r.embed(|e| {
                    let item = items.iter().fold(
                        format!(
                            "```{:>30}{:>10}->{:<10}{}\n",
                            "name", "price", "target price", "behind"
                        ),
                        |mut s, (listing, undercut)| {
                            let item_id = ItemId(listing.item_id);
                            let item_name = item_db
                                .get(&item_id)
                                .map(|i| i.name.as_str())
                                .unwrap_or_default();
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
                    e.title(&retainer.name)
                        .description(item)
                        .color(Color::from_rgb(123, 0, 123))
                });
            }
        }
        if r.embeds.is_empty() {
            r.content("No undercuts found!");
        }
        r
    })
    .await?;
    Ok(())
}

/// Returns a list of your retainers & all of their market board listings
#[poise::command(slash_command)]
async fn check_listings(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let retainers = ctx
        .data()
        .db
        .get_retainer_listings_for_discord_user(ctx.author().id.0)
        .await?;
    if retainers.is_empty() {
        ctx.say("No retainers found :(").await?;
    }
    let data = xiv_gen_db::data();
    let items = &data.items;
    ctx.send(|r| {
        for (_, retainer, listings) in retainers {
            let mut msg_contents = String::new();
            msg_contents += "```";
            writeln!(
                msg_contents,
                "{:<30} {:>9} {:>4} {:>1}",
                "Item name", "price per item", "Qty.", "hq"
            )
            .unwrap();
            for listing in listings {
                let item_name = items
                    .get(&ItemId(listing.item_id))
                    .map(|i| i.name.as_str())
                    .unwrap_or_default();
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

            r.embed(|e| {
                e.title(retainer.name)
                    .description(msg_contents)
                    .color(Color::from_rgb(123, 0, 123))
            });
        }
        r
    })
    .await?;
    Ok(())
}

/// Adds a retainer to your profile
#[poise::command(slash_command)]
async fn add(
    ctx: Context<'_>,
    #[description = "Retainer name"]
    #[autocomplete = "autocomplete_retainer_id"]
    retainer_id: i32,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let _register_retainer = ctx
        .data()
        .db
        .register_retainer(retainer_id, ctx.author().id.0, ctx.author().name.clone())
        .await?;
    ctx.send(|r| {
        r.embed(|e| {
            e.title("Added retainer")
                .description("added retainer!")
                .color(Color::from_rgb(123, 0, 123))
        })
    })
    .await?;
    Ok(())
}

async fn owned_retainer_auto_complete(
    ctx: Context<'_>,
    partial: &str,
) -> impl Iterator<Item = poise::AutocompleteChoice<i32>> {
    let partial = partial.to_string();
    ctx.data()
        .db
        .get_owned_retainers(ctx.author().id.0, ctx.author().name.clone())
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(move |(_o, r)| {
            if let Some(retainer) = r {
                retainer
                    .name
                    .to_ascii_lowercase()
                    .contains(&partial.to_ascii_lowercase())
            } else {
                false
            }
        })
        .flat_map(|(owned, retainer)| {
            let retainer = retainer?;
            Some(poise::AutocompleteChoice {
                name: retainer.name,
                value: owned.id,
            })
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
        .remove_owned_retainer(ctx.author().id.0, owned_retainer_id)
        .await?;
    ctx.say("Removed retainer successfully!").await?;
    let _ = ctx
        .data()
        .event_senders
        .retainers
        .send(EventType::removed(removed));
    Ok(())
}
