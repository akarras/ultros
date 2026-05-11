use anyhow::anyhow;
use itertools::Itertools;
use poise::serenity_prelude::CreateAttachment;
use ultros_db::world_data::world_cache::AnySelector;
use xiv_gen::ItemId;

use crate::discord::ffxiv::ULTROS_COLOR;
use crate::discord::ffxiv::helpers::{name_matches_lowered, top_n_cheapest_listings};
use crate::web::item_card::generate_image;

use super::{Context, Error};

/// Lookup price information from the market board
#[poise::command(slash_command, prefix_command, subcommands("current", "history"))]
pub(crate) async fn prices(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

async fn autocomplete_item<'a>(
    _ctx: Context<'_>,
    partial: &'a str,
) -> impl Iterator<Item = poise::serenity_prelude::AutocompleteChoice> + 'a {
    let partial = partial.to_lowercase();
    xiv_gen_db::data()
        .items
        .values()
        .filter(move |item| name_matches_lowered(&item.name, &partial))
        .map(|item| {
            poise::serenity_prelude::AutocompleteChoice::new(item.name.to_string(), item.key_id.0)
        })
        .take(99)
}

async fn autocomplete_world<'a>(
    ctx: Context<'a>,
    partial: &'a str,
) -> impl Iterator<Item = String> + 'a {
    let partial = partial.to_lowercase();
    ctx.data()
        .world_cache
        .get_all_results()
        .filter(move |w| name_matches_lowered(w.get_name(), &partial))
        .map(|w| w.get_name().to_string())
        .take(99)
}

/// Get the real time prices from highest to lowest
#[poise::command(slash_command, prefix_command)]
async fn current(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_item"] item: i32,
    #[autocomplete = "autocomplete_world"] world: String,
    hq_only: Option<bool>,
) -> Result<(), Error> {
    ctx.defer().await?;
    let worlds = &ctx.data().world_cache;
    let world = worlds.lookup_value_by_name(&world)?;
    let world_ids = worlds
        .get_all_worlds_in(&world)
        .ok_or(anyhow!("bad world data"))?;
    let item_data = xiv_gen_db::data()
        .items
        .get(&ItemId(item))
        .ok_or(anyhow!("bad item id"))?;
    let listings = ctx
        .data()
        .db
        .get_all_listings_in_worlds(&world_ids, universalis::ItemId(item))
        .await?;
    let listings = top_n_cheapest_listings(listings, hq_only, 10)
        .into_iter()
        .format_with("\n", |l, f| {
            f(&format_args!(
                "{:<10} {:3} {:<7} {}",
                l.price_per_unit,
                if l.hq { "✅" } else { "" },
                l.quantity,
                worlds
                    .lookup_selector(&AnySelector::World(l.world_id))
                    .as_ref()
                    .map(|w| w.get_name())
                    .unwrap_or_default()
            ))
        })
        .to_string();
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title(&item_data.name)
                .description(format!(
                    "```\n{:<10} {:3} {:<7} {}\n{}\n```",
                    "price", "hq", "quantity", "world", listings,
                )),
        ),
    )
    .await?;
    Ok(())
}

/// Get the recent prices for an item
#[poise::command(slash_command, prefix_command)]
async fn history(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_item"]
    #[description = "Item to get the price history for"]
    item: i32,
    #[description = "World, Datacenter, or Region to get prices for"]
    #[autocomplete = "autocomplete_world"]
    world: String,
) -> Result<(), Error> {
    ctx.defer().await?;
    let data = ctx.data();
    let item = xiv_gen_db::data()
        .items
        .get(&ItemId(item))
        .ok_or(anyhow!("Item not found"))?;
    let world = data
        .world_helper
        .lookup_world_by_name(&world)
        .ok_or(anyhow!("Unable to find world"))?;
    let png = generate_image(&data.db, &data.world_helper, item, &world).await?;
    let attachment = CreateAttachment::bytes(png, "chart.png");
    ctx.send(
        poise::CreateReply::default()
            .embed(
                poise::serenity_prelude::CreateEmbed::new()
                    .title([&item.name, " - ", world.get_name()].concat())
                    .color(ULTROS_COLOR)
                    .image("attachment://chart.png"),
            )
            .attachment(attachment),
    )
    .await?;
    Ok(())
}
