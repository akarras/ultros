use poise::serenity_prelude::Color;
use std::fmt::Write;
use xiv_gen::ItemId;

use crate::analyzer_service::{ResaleOptions, SoldAmount, SoldWithin};

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
    #[description = "Minimum profit"] minimum_profit: i32,
    #[description = "Number of items required to be sold within the threshold"]
    number_recently_sold: i32,
    #[description = "Length of the threshold in days"] threshold_days: i32,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let world = ctx.data().world_cache.lookup_value_by_name(&world)?;
    let world_id = world.as_world()?.id;
    let region_id = ctx
        .data()
        .world_cache
        .get_region(&world)
        .ok_or(anyhow::anyhow!("World not in a region?"))?
        .id;

    let amount = SoldAmount(number_recently_sold.clamp(0, 255) as u8);
    let filter_sale = if threshold_days <= 1 {
        SoldWithin::Today(amount)
    } else if threshold_days <= 7 {
        SoldWithin::Week(amount)
    } else if threshold_days <= 30 {
        SoldWithin::Month(amount)
    } else if threshold_days <= 365 {
        SoldWithin::Year(amount)
    } else {
        SoldWithin::YearsAgo(((threshold_days / 365).clamp(1, 255)) as u8, amount)
    };

    let xiv_data = xiv_gen_db::data();
    let items = &xiv_data.items;
    let resale = ResaleOptions {
        minimum_profit: Some(minimum_profit),
        filter_sale: Some(filter_sale),
        ..Default::default()
    };
    let sales = ctx
        .data()
        .analyzer_service
        .get_best_resale(world_id, region_id, resale, &ctx.data().world_cache)
        .await
        .ok_or(anyhow::anyhow!("Unable to get resale results"))?;
    ctx.send(poise::CreateReply::default().embed(
        poise::serenity_prelude::CreateEmbed::new()
            .title("Flip Finder")
            .color(Color::from_rgb(123, 0, 123))
            .description({
                let mut content = format!("`{:<40} |  roi  | profit`\n", "item name");
                for sale in sales {
                    let item_name = items
                        .get(&ItemId(sale.item_id))
                        .map(|i| i.name.as_str())
                        .unwrap_or_default();
                    writeln!(
                        &mut content,
                        "`{item_name:<40} | {:7.2}% | {:<10}` [url](https://universalis.app/market/{})",
                        sale.return_on_investment, sale.profit, sale.item_id
                    )
                    .unwrap();
                }
                content
            })
    ))
    .await?;

    Ok(())
}
