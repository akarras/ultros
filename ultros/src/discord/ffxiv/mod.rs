use super::{Context, Error};
pub(crate) mod alerts;
mod character;
mod retainer;

use character::character;
use retainer::retainer;

#[poise::command(slash_command, prefix_command, subcommands("character", "retainer"))]
pub(crate) async fn ffxiv(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Hello world").await?;
    Ok(())
}
