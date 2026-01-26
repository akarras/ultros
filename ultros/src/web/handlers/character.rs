use axum::Json;
use axum::extract::{Path, State};
use std::sync::Arc;
use ultros_api_types::{FfxivCharacter, FfxivCharacterVerification};
use ultros_db::UltrosDb;
use ultros_db::world_cache::WorldCache;

use crate::web::character_verifier_service::CharacterVerifierService;
use crate::web::error::ApiError;
use crate::web::oauth::AuthDiscordUser;

// #[debug_handler(state = WebState)]
pub(crate) async fn user_characters(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<FfxivCharacter>>, ApiError> {
    let characters = db
        .get_all_characters_for_discord_user(user.id as i64)
        .await?;
    // we can now strip the owned final fantasy character tag and convert to the API version
    Ok(Json(
        characters
            .into_iter()
            .flat_map(|(_, character)| character.map(|c| c.into()))
            .collect::<Vec<_>>(),
    ))
}

pub(crate) async fn pending_verifications(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<FfxivCharacterVerification>>, ApiError> {
    let verifications = db
        .get_all_pending_verification_challenges(user.id as i64)
        .await?;
    Ok(Json(
        verifications
            .into_iter()
            .flat_map(|(verification, character)| {
                character.map(|character| FfxivCharacterVerification {
                    id: verification.id,
                    character: character.into(),
                    verification_string: verification.challenge,
                })
            })
            .collect::<Vec<_>>(),
    ))
}

pub(crate) async fn character_search(
    _user: AuthDiscordUser, // user required just to prevent this endpoint from being abused.
    Path(name): Path<String>,
    State(cache): State<Arc<WorldCache>>,
) -> Result<Json<Vec<FfxivCharacter>>, ApiError> {
    let builder = lodestone::search::SearchBuilder::new().character(&name);
    // if let Some(world) = query.world {
    //     let world = cache.lookup_selector(&AnySelector::World(world))?;
    //     let world_name = world.get_name();
    //     builder = builder.server(Server::from_str(world_name)?);
    // }
    let client = reqwest::Client::new();
    let search_results = builder.send_async(&client).await?;

    let characters = search_results
        .into_iter()
        .flat_map(|r| {
            // world comes back as World [Datacenter], so strip the datacenter and parse the world
            let (world, _) = r.world.split_once(' ')?;
            let world = cache
                .lookup_value_by_name(world)
                .ok()
                .unwrap_or_else(|| panic!("World {} not found", world));
            let (first_name, last_name) = r
                .name
                .split_once(' ')
                .expect("Should always have first last name");
            Some(FfxivCharacter {
                id: r.user_id as i32,
                first_name: first_name.to_string(),
                last_name: last_name.to_string(),
                world_id: world.as_world().ok()?.id,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(characters))
}

pub(crate) async fn claim_character(
    user: AuthDiscordUser,
    Path(character_id): Path<u32>,
    State(verifier): State<CharacterVerifierService>,
) -> Result<Json<(i32, String)>, ApiError> {
    let result = verifier
        .start_verification(character_id, user.id as i64)
        .await?;
    // db.create_character_challenge(character_id, user.id as i64, challenge)
    Ok(Json(result))
}

// #[debug_handler(state = WebState)]
pub(crate) async fn unclaim_character(
    user: AuthDiscordUser,
    Path(character_id): Path<i32>,
    State(db): State<UltrosDb>,
) -> Result<Json<()>, ApiError> {
    db.delete_owned_character(user.id as i64, character_id)
        .await?;
    Ok(Json(()))
}

pub(crate) async fn verify_character(
    State(character): State<CharacterVerifierService>,
    Path(verification_id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<bool>, ApiError> {
    character
        .check_verification(verification_id, user.id as i64)
        .await?;
    Ok(Json(true))
}
