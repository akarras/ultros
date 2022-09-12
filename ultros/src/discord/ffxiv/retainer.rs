use poise::CreateReply;

use super::{Context, Error};

#[poise::command(slash_command, prefix_command, subcommands("list"))]
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
async fn add(
    ctx: Context<'_>,
    #[description = "Retainer name"]
    #[autocomplete = "autocomplete_retainer_id"]
    retainer_id: i32,
) -> Result<(), Error> {
    // TODO
    Ok(())
}
