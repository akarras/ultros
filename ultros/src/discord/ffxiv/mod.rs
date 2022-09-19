use super::{Context, Error};
pub(crate) mod alerts;
mod analyze;
mod character;
mod retainer;

use analyze::analyze;
use character::character;
use retainer::retainer;

#[poise::command(
    slash_command,
    prefix_command,
    subcommands("character", "retainer", "analyze")
)]
pub(crate) async fn ffxiv(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Hello world").await?;
    Ok(())
}
