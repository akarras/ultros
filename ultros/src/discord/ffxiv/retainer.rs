use poise::{serenity_prelude::Color, CreateReply};
use std::fmt::Write;
use ultros_db::entity::active_listing;
use xiv_gen::ItemId;

use super::{Context, Error};

#[poise::command(
    slash_command,
    prefix_command,
    subcommands("list", "add", "check_listings")
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

/// Returns a list of your retainers & all of their marketboard listings
#[poise::command(slash_command)]
async fn check_listings(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let retainers = ctx
        .data()
        .db
        .get_retainers_for_discord_user(ctx.id())
        .await?;
    if retainers.is_empty() {
        ctx.say("No retainers found :(").await?;
    }
    let data = xiv_gen_db::decompress_data();
    let items = data.get_items();
    for (retainer, listings) in retainers {
        let mut msg_contents = String::new();
        for listing in listings {
            let item_name = items
                .get(&ItemId::new(listing.id))
                .map(|i| i.get_name())
                .unwrap_or_default();
            let active_listing::Model {
                id,
                world_id,
                item_id,
                retainer_id,
                price_per_unit,
                quantity,
                hq,
                timestamp,
            } = &listing;
            let hq = if *hq { '✅' } else { ' ' };
            write!(
                msg_contents,
                "{item_name} - {price_per_unit:>7} - {quantity:>4} - {hq}"
            );
        }
        ctx.send(|r| {
            r.embed(|e| {
                e.title(retainer.name)
                    .description(msg_contents)
                    .color(Color::from_rgb(123, 0, 123))
            })
        })
        .await?;
    }
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
