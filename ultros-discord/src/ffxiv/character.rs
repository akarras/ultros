use super::{Context, Error};
use crate::character_claim::CharacterClaimService;

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
    #[description = "name of your ffxiv character"]
    #[max_length = 100]
    name: String,
    #[description = "world your character is on"]
    #[max_length = 100]
    home_world: Option<String>,
) -> Result<(), Error> {
    super::retainer_setup::register_character(ctx, name, home_world).await
}

pub(super) async fn search(
    ctx: Context<'_>,
    name: &str,
    world: Option<&str>,
) -> Result<Vec<lodestone::search::ProfileSearchResult>, Error> {
    let mut search = lodestone::search::SearchBuilder::new().character(name.trim());

    if let Some(world) = world.filter(|world| !world.trim().is_empty()) {
        search = search.server(
            world
                .trim()
                .parse::<lodestone::model::server::Server>()
                .map_err(|_| {
                    anyhow::anyhow!("Unknown home world. Check its spelling or leave it blank.")
                })?,
        );
    }

    search
        .send_async(&ctx.data().lodestone_client)
        .await
        .map_err(|error| {
            tracing::warn!(?error, "Discord character search failed");
            anyhow::anyhow!("Character search is temporarily unavailable. Retry or keep your retainer unassigned.").into()
        })
}

pub(super) async fn claim(
    ctx: Context<'_>,
    character_id: u32,
) -> Result<ultros_db::entity::final_fantasy_character::Model, Error> {
    let data = ctx.data();

    data.db
        .get_or_create_discord_user(ctx.author().id.get(), ctx.author().name.clone())
        .await
        .map_err(|error| {
            tracing::warn!(?error, "Discord character setup could not load the account");
            anyhow::anyhow!(
                "I couldn't add that character just now. Retry or keep your retainer unassigned."
            )
        })?;

    let claim = CharacterClaimService {
        db: data.db.clone(),
        client: data.lodestone_client.clone(),
        world_cache: data.world_cache.clone(),
    };

    claim
        .claim_character(character_id, ctx.author().id.get() as i64)
        .await
        .map_err(|error| {
            tracing::warn!(?error, "Discord character claim failed");
            anyhow::anyhow!(
                "I couldn't add that character just now. Retry or keep your retainer unassigned."
            )
            .into()
        })
}
