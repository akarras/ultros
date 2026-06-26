use anyhow::anyhow;
use itertools::Itertools;
use poise::serenity_prelude::CreateAttachment;
use ultros_db::world_data::world_cache::AnySelector;
use xiv_gen::ItemId;

use crate::discord::ffxiv::ULTROS_COLOR;
use crate::discord::ffxiv::helpers::{
    discord_locale_to_xiv_language, localized_item_matches, localized_item_name,
    name_matches_lowered, top_n_cheapest_listings, truncate_100,
};
use crate::web::item_card::generate_image;

use super::{Context, Error};

/// Lookup price information from the market board
#[poise::command(slash_command, prefix_command, subcommands("current", "history"))]
pub(crate) async fn prices(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

async fn autocomplete_item<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> impl Iterator<Item = poise::serenity_prelude::AutocompleteChoice> + 'a {
    let user_lang = discord_locale_to_xiv_language(ctx.locale());
    localized_item_matches(partial, user_lang)
        .into_iter()
        .map(|m| poise::serenity_prelude::AutocompleteChoice::new(m.label, m.item_id))
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
        .map(|w| truncate_100(w.get_name()))
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
    let user_lang = discord_locale_to_xiv_language(ctx.locale());
    let worlds = &ctx.data().world_cache;
    let world = worlds.lookup_value_by_name(&world)?;
    let world_ids = worlds
        .get_all_worlds_in(&world)
        .ok_or(anyhow!("bad world data"))?;
    // Verify the id is real before issuing the DB call; the title uses the
    // user's locale, falling back to English.
    xiv_gen_db::data_for(xiv_gen::Language::En)
        .items
        .get(&ItemId(item))
        .ok_or(anyhow!("bad item id"))?;
    let item_display_name = localized_item_name(item, user_lang);
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
                .title(&item_display_name)
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
    let user_lang = discord_locale_to_xiv_language(ctx.locale());
    let data = ctx.data();
    // The image generator needs the canonical English `Item` reference for its
    // metadata lookups; the embed title uses the user's locale for display.
    let item_en = xiv_gen_db::data_for(xiv_gen::Language::En)
        .items
        .get(&ItemId(item))
        .ok_or(anyhow!("Item not found"))?;
    let item_display_name = localized_item_name(item, user_lang);
    let world = data
        .world_helper
        .lookup_world_by_name(&world)
        .ok_or(anyhow!("Unable to find world"))?;
    let png = generate_image(&data.db, &data.world_helper, item_en, &world).await?;
    let attachment = CreateAttachment::bytes(png, "chart.png");
    ctx.send(
        poise::CreateReply::default()
            .embed(
                poise::serenity_prelude::CreateEmbed::new()
                    .title([&item_display_name, " - ", world.get_name()].concat())
                    .color(ULTROS_COLOR)
                    .image("attachment://chart.png"),
            )
            .attachment(attachment),
    )
    .await?;
    Ok(())
}
