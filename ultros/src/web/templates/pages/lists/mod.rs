use axum::{
    extract::{Path, State},
    response::Redirect,
};
use ultros_db::UltrosDb;

use crate::web::{error::WebError, oauth::AuthDiscordUser};

pub(crate) mod add;
pub(crate) mod item_add;
pub(crate) mod overview;
pub(crate) mod view;

pub(crate) async fn delete_list(
    user: AuthDiscordUser,
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
) -> Result<Redirect, WebError> {
    db.delete_list(id, user.id as i64).await?;
    Ok(Redirect::to("/list"))
}
