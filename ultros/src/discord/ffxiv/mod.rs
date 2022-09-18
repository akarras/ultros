use super::{Context, Error};
pub(crate) mod alerts;
mod character;
mod retainer;
mod analyze;

use character::character;
use retainer::retainer;
use analyze::analyze;

#[poise::command(slash_command, prefix_command, subcommands("character", "retainer", "analyze"))]
pub(crate) async fn ffxiv(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Hello world").await?;
    Ok(())
}
