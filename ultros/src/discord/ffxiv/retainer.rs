use crate::EventType;
use poise::serenity_prelude::Color;
use std::sync::Arc;
use std::{collections::HashSet, fmt::Write};
use ultros_db::entity::active_listing;
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
        "add_undercut_alert"
    )
)]
pub(crate) async fn retainer(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Hello world").await?;
    Ok(())
}

/// Returns the users retainers
#[poise::command(slash_command, prefix_command)]
async fn list(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Hello world").await?;
    Ok(())
}

async fn autocomplete_retainer_id(
    ctx: Context<'_>,
    partial: &str,
) -> impl Iterator<Item = poise::AutocompleteChoice<i32>> {
    ctx.data()
        .db
        .search_retainers(partial)
        .await
        .unwrap_or_default()
        .into_iter()
        .flat_map(|(retainer, world)| {
            let world = world?;
            Some(poise::AutocompleteChoice {
                name: format!("{} - {}", retainer.name, world.name),
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
        .send(EventType::Add(Arc::new(alert)))?;
    ctx.say(&format!("Now sending alerts to this channel anytime someone undercuts your retainer by {margin_percent}%")).await?;
    Ok(())
}

/// Shows only listings where your retainers listing has been undercut by someone else
#[poise::command(slash_command)]
async fn check_undercuts(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let user_id = ctx.author().id.0;
    let (_, under_cut_items) = ctx.data().db.get_retainer_undercut_items(user_id).await?;
    let data = xiv_gen_db::decompress_data();
    let item_db = &data.items;
    ctx.send(|r| {
        for (retainer, items) in &under_cut_items {
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

/// Returns a list of your retainers & all of their marketboard listings
#[poise::command(slash_command)]
async fn check_listings(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let (_, retainers) = ctx
        .data()
        .db
        .get_retainer_listings_for_discord_user(ctx.author().id.0)
        .await?;
    // get data on how well each of the listings for the retainer are performing
    let item_and_world_ids: HashSet<(i32, i32)> = retainers
        .iter()
        .flat_map(|(_, listing)| {
            listing
                .iter()
                .map(|listing| (listing.item_id, listing.world_id))
        })
        .collect();
    if retainers.is_empty() {
        ctx.say("No retainers found :(").await?;
    }
    let data = xiv_gen_db::decompress_data();
    let items = &data.items;
    ctx.send(|r| {
        for (retainer, listings) in retainers {
            let mut msg_contents = String::new();
            msg_contents += "```";
            writeln!(
                msg_contents,
                "{:<30} {:>9} {:>4} {:>1}",
                "Item name", "price per item", "quantity", "hq"
            )
            .unwrap();
            for listing in listings {
                let item_name = items
                    .get(&ItemId(listing.item_id))
                    .map(|i| i.name.as_str())
                    .unwrap_or_default();
                let active_listing::Model {
                    id,
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
    ctx.data()
        .db
        .remove_owned_retainer(ctx.author().id.0, owned_retainer_id)
        .await?;
    ctx.say("Removed retainer successfully!").await?;
    Ok(())
}
