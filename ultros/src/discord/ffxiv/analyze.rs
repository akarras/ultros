use chrono::Duration;
use poise::serenity_prelude::Color;
use std::fmt::Write;
use xiv_gen::ItemId;

use super::{Context, Error};

#[poise::command(slash_command, prefix_command, subcommands("profit"))]
pub(crate) async fn analyze(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Hello world").await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub(crate) async fn profit(
    ctx: Context<'_>,
    #[description = "World you want to try and sell items on"] world: String,
    #[description = "Number of items required to be sold within the threshold"]
    number_recently_sold: i32,
    #[description = "Length of the threshold in days"] threshold_days: i32,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let world = ctx.data().db.get_world(&world).await?;
    let world = world.id;
    let threshold = number_recently_sold;
    let window = Duration::days(threshold_days.into());
    let xiv_data = xiv_gen_db::decompress_data();
    let items = &xiv_data.items;
    let sales = ctx
        .data()
        .db
        .get_best_item_to_resell_on_world(world, threshold, window)
        .await?;
    ctx.send(|reply| {
        reply.embed(|e| {
            let mut content = format!("`{:<40} |  margin  | profit`\n", "item name");
            for sale in sales {
                let item_name = items
                    .get(&ItemId(sale.item_id))
                    .map(|i| i.name.as_str())
                    .unwrap_or_default();
                writeln!(
                    &mut content,
                    "`{item_name:<40} | {:7.2}% | {:<10}` [url](https://universalis.app/market/{})",
                    sale.margin, sale.profit, sale.item_id
                )
                .unwrap();
            }
            e.title("Price Analyzer")
                .color(Color::from_rgb(123, 0, 123))
                .description(content)
        })
    })
    .await?;

    Ok(())
}
