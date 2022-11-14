use axum::{
    extract::{Path, State},
    response::Redirect,
};
use lodestone::model::profile::Profile;
use ultros_db::UltrosDb;

use crate::web::error::WebError;

pub(crate) mod add_character;
pub(crate) mod claim_character;
pub(crate) mod verify_character;

pub(crate) async fn refresh_character(
    State(db): State<UltrosDb>,
    Path(character_id): Path<i32>,
) -> Result<Redirect, WebError> {
    let character = db
        .get_character(character_id)
        .await?
        .ok_or(anyhow::Error::msg("Character not in database"))?;
    let client = reqwest::Client::new();
    let profile = Profile::get_async(&client, character_id as u32).await?;
    let (first_name, last_name) = profile.name.split_once(" ").unwrap();
    db.update_character_name(character, first_name, last_name).await?;
    Ok(Redirect::to("/profile"))
}
