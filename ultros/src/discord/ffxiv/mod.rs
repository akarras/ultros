use super::{Context, Error};
pub mod admin;
mod alert;
mod analyze;
mod character;
mod helpers;
mod item_prices;
mod lists;
mod retainer;

use admin::rescan_market;
use alert::alert;
use analyze::analyze;
use character::character;
use item_prices::prices;
use lists::list;
use poise::serenity_prelude::Color;
use retainer::retainer;

pub(crate) const ULTROS_COLOR: Color = Color::DARK_PURPLE;

#[poise::command(
    slash_command,
    prefix_command,
    subcommands(
        "character",
        "retainer",
        "analyze",
        "list",
        "prices",
        "rescan_market",
        "alert"
    )
)]
pub(crate) async fn ffxiv(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Hello world").await?;
    Ok(())
}
