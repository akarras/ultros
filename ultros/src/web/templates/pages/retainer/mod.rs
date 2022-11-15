use axum::{
    extract::{Path, State},
    response::Redirect,
};
use ultros_db::{ActiveValue, UltrosDb};

use crate::web::{error::WebError, oauth::AuthDiscordUser};

pub(crate) mod add_retainer;
pub(crate) mod edit_retainer;
pub(crate) mod generic_retainer_page;
pub(crate) mod user_retainers_page;

pub(crate) async fn add_retainer_to_character(
    Path((owned_retainer_id, character_id)): Path<(i32, i32)>,
    State(ultros_db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Redirect, WebError> {
    ultros_db
        .update_owned_retainer(user.id as i64, owned_retainer_id, move |mut u| {
            u.character_id = ActiveValue::Set(Some(character_id));
            u
        })
        .await?;
    Ok(Redirect::to("/retainers/edit"))
}

pub(crate) async fn remove_retainer_from_character(
    Path(owned_retainer_id): Path<i32>,
    State(ultros_db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Redirect, WebError> {
    ultros_db
        .update_owned_retainer(user.id as i64, owned_retainer_id, move |mut u| {
            u.character_id = ActiveValue::Set(None);
            u
        })
        .await?;
    Ok(Redirect::to("/retainers/edit"))
}

pub(crate) async fn increase_weight_retainer(
    Path(owned_retainer_id): Path<i32>,
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Redirect, WebError> {
    db.update_owned_retainer(user.id as i64, owned_retainer_id, move |mut u| {
        let current_weight = u.weight.take().unwrap_or_default().unwrap_or_default();
        u.weight = ActiveValue::Set(Some(current_weight + 1));
        u
    })
    .await?;
    Ok(Redirect::to("/retainers/edit"))
}

pub(crate) async fn decrease_weight_retainer(
    Path(owned_retainer_id): Path<i32>,
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Redirect, WebError> {
    db.update_owned_retainer(user.id as i64, owned_retainer_id, move |mut u| {
        let current_weight = u.weight.take().unwrap_or_default().unwrap_or_default();
        u.weight = ActiveValue::Set(Some(current_weight - 1));
        u
    })
    .await?;
    Ok(Redirect::to("/retainers/edit"))
}
