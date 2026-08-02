use super::{Context, Error};
use crate::character_claim::CharacterClaimService;
use std::time::Duration;

#[poise::command(slash_command, prefix_command, subcommands("register"))]
pub(crate) async fn character(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default().embed(
            poise::serenity_prelude::CreateEmbed::new()
                .title("FFXIV Characters")
                .description(
                    "Look up your character on the Lodestone and add it to your account.\n\n\
                     `/ffxiv character register name:<First Last>` — search and select.\n\n\
                     Characters group your retainers; adding one doesn't claim it \
                     exclusively, so several accounts can add the same character.",
                ),
        ),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub(crate) async fn register(
    ctx: Context<'_>,
    #[description = "name of your ffxiv character"] name: String,
    #[description = "world your character is on"] home_world: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let mut search = lodestone::search::SearchBuilder::new().character(&name);

    if let Some(world) = home_world {
        let world: lodestone::model::server::Server = world.parse()?;
        search = search.server(world);
    }
    let profiles = search.send_async(&ctx.data().lodestone_client).await?;
    let options = profiles
        .iter()
        .map(|search_result| {
            poise::serenity_prelude::CreateSelectMenuOption::new(
                format!("{}\n{}", search_result.name, search_result.world),
                search_result.user_id.to_string(),
            )
            .description(search_result.world.clone())
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
        if let poise::serenity_prelude::ComponentInteractionDataKind::StringSelect { values } =
            &msg.data.kind
        {
            let selected_user_id = values[0].parse::<u32>()?;
            let data = ctx.data();
            // The Discord login already establishes who the user is, so the
            // selection is the whole flow — there's no Lodestone bio challenge
            // to complete any more.
            data.db
                .get_or_create_discord_user(ctx.author().id.get(), ctx.author().name.clone())
                .await?;
            let claim = CharacterClaimService {
                db: data.db.clone(),
                client: data.lodestone_client.clone(),
                world_cache: data.world_cache.clone(),
            };
            let character = claim
                .claim_character(selected_user_id, ctx.author().id.get() as i64)
                .await?;

            ctx.say(format!(
                "Added {} {} to your characters.",
                character.first_name, character.last_name
            ))
            .await?;
        }
    } else {
        ctx.say("No choice selected").await?;
        return Ok(());
    };
    Ok(())
}
