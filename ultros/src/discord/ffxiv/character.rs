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
    let msg = ctx
        .send(|reply| {
            reply.components(|components| {
                components.create_action_row(|row| {
                    row.create_select_menu(|select| {
                        select.custom_id("RegisterCharacterSelect").options(|o| {
                            for search_result in profiles {
                                o.create_option(|o| {
                                    o.description("test")
                                        .label(format!(
                                            "{}\n{}",
                                            search_result.name, search_result.world
                                        ))
                                        .value(search_result.user_id)
                                });
                            }
                            o
                        })
                    })
                })
            })
        })
        .await?;
    if let Some(msg) = msg
        .message()
        .await?
        .await_component_interaction(ctx.discord())
        .timeout(Duration::from_secs(5 * 60))
        .await
    {
        ctx.say(format!("selected {}", msg.data.values[0])).await?;
        // TODO lookup what value was selected from the list of interactions
    } else {
        ctx.say("No choice selected").await?;
        return Ok(());
    };
    Ok(())
}
