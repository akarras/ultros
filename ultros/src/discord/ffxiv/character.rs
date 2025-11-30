use super::{Context, Error};
use std::time::Duration;

#[poise::command(slash_command, prefix_command, subcommands("register"))]
pub(crate) async fn character(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Hello world").await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub(crate) async fn register(
    ctx: Context<'_>,
    #[description = "name of your ffxiv character"] name: String,
    #[description = "world your character is on"] home_world: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    // TODO check if the user has a pending registration
    let mut search = lodestone::search::SearchBuilder::new().character(&name);

    if let Some(world) = home_world {
        let world: lodestone::model::server::Server = world.parse()?;
        search = search.server(world);
    }
    let profiles = search.send_async(&ctx.data().lodestone_client).await?;
    let options = profiles
        .into_iter()
        .map(|search_result| {
            poise::serenity_prelude::CreateSelectMenuOption::new(
                format!("{}\n{}", search_result.name, search_result.world),
                search_result.user_id.to_string(),
            )
            .description("test")
        })
        .collect();

    let select_menu = poise::serenity_prelude::CreateSelectMenu::new(
        "RegisterCharacterSelect",
        poise::serenity_prelude::CreateSelectMenuKind::String { options },
    );
    let action_row = poise::serenity_prelude::CreateActionRow::SelectMenu(select_menu);

    let msg = ctx
        .send(poise::CreateReply::default().components(vec![action_row]))
        .await?;
    if let Some(msg) = msg
        .message()
        .await?
        .await_component_interaction(ctx.serenity_context())
        .timeout(Duration::from_secs(5 * 60))
        .await
    {
        if let poise::serenity_prelude::ComponentInteractionDataKind::StringSelect { values } = &msg.data.kind {
             ctx.say(format!("selected {}", values[0])).await?;
             // TODO lookup what value was selected from the list of interactions
        }
    } else {
        ctx.say("No choice selected").await?;
        return Ok(());
    };
    Ok(())
}
