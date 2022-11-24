use axum::{
    extract::{self, Path, State},
    response::Redirect,
};
use serde::Deserialize;
use serde_with::{serde_as, DisplayFromStr};
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

#[serde_as]
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RetainerData {
    #[serde_as(as = "DisplayFromStr")]
    owned_retainer_id: i32,
    #[serde_as(as = "DisplayFromStr")]
    order: i32,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct Retainers(Vec<RetainerData>);

pub(crate) async fn reorder_retainer(
    State(ultros_db): State<UltrosDb>,
    user: AuthDiscordUser,
    extract::Json(retainers): extract::Json<Retainers>,
) -> Result<(), WebError> {
    futures::future::try_join_all(retainers.0.into_iter().map(|r| {
        ultros_db.update_owned_retainer(user.id as i64, r.owned_retainer_id, move |mut u| {
            u.weight = ActiveValue::Set(Some(r.order));
            u
        })
    }))
    .await?;

    Ok(())
}
