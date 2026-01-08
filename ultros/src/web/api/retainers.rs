use axum::extract::{Path, State};
use axum::response::Redirect;
use axum::Json;
use futures::future::try_join_all;
use ultros_api_types::retainer::RetainerListings;
use ultros_api_types::user::{OwnedRetainer, UserRetainerListings, UserRetainers};
use ultros_api_types::{ActiveListing, FfxivCharacter, Retainer};
use ultros_db::{ActiveValue, UltrosDb};

use crate::web::error::{ApiError, WebError};
use crate::web::oauth::AuthDiscordUser;

pub(crate) async fn add_retainer(
    State(db): State<UltrosDb>,
    current_user: AuthDiscordUser,
    Path(retainer_id): Path<i32>,
) -> Result<Redirect, ApiError> {
    let _register_retainer = db
        .register_retainer(retainer_id, current_user.id, current_user.name)
        .await?;
    Ok(Redirect::to("/retainers/edit"))
}

pub(crate) async fn remove_owned_retainer(
    State(db): State<UltrosDb>,
    current_user: AuthDiscordUser,
    Path(retainer_id): Path<i32>,
) -> Result<Redirect, WebError> {
    db.remove_owned_retainer(current_user.id, retainer_id)
        .await?;
    Ok(Redirect::to("/retainers/edit"))
}

pub(crate) async fn retainer_listings(
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
) -> Result<Json<RetainerListings>, ApiError> {
    let (retainer, listings) = db.get_retainer_listings(id).await?;
    let listings = RetainerListings {
        retainer: retainer.into(),
        listings: listings.into_iter().map(ActiveListing::from).collect(),
    };
    Ok(Json(listings))
}

pub(crate) async fn user_retainers(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<UserRetainers>, ApiError> {
    // load the retainer/character details from the database and then extract it into the shared API types.
    let retainers = UserRetainers {
        retainers: db
            .get_all_owned_retainers_and_character(user.id)
            .await?
            .into_iter()
            .map(|(character, retainers)| {
                (
                    character.map(FfxivCharacter::from),
                    retainers
                        .into_iter()
                        .map(|(owned, retainer)| {
                            (OwnedRetainer::from(owned), Retainer::from(retainer))
                        })
                        .collect(),
                )
            })
            .collect(),
    };
    Ok(Json(retainers))
}

pub(crate) async fn user_retainer_listings(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<UserRetainerListings>, ApiError> {
    let db = &db;
    // Get a list of all the user's retainers, convert them to the appropriate type for our API call, and get listings for each retainer
    let retainers = db.get_all_owned_retainers_and_character(user.id).await?;
    let listings_iter = retainers
        .into_iter()
        .map(|(character, retainers)| async move {
            // collect intermediate results with try_join_all to cancel early if there's an error
            let retainers_with_listings =
                try_join_all(retainers.into_iter().map(|(_owned, retainer)| async move {
                    let listings = db.get_retainer_listings(retainer.id).await;
                    listings.map(|(_retainer, listings)| {
                        (
                            Retainer::from(retainer),
                            listings
                                .into_iter()
                                .map(ActiveListing::from)
                                .collect::<Vec<_>>(),
                        )
                    })
                }))
                .await;
            retainers_with_listings.map(|r| (character.map(FfxivCharacter::from), r))
        });
    let listings = try_join_all(listings_iter).await?;
    let retainers = UserRetainerListings {
        retainers: listings,
    };
    Ok(Json(retainers))
}

pub(crate) async fn retainer_search(
    State(db): State<UltrosDb>,
    Path(retainer_name): Path<String>,
) -> Result<Json<Vec<Retainer>>, ApiError> {
    let retainers = db.search_retainers(&retainer_name).await?;
    Ok(Json(retainers))
}

pub(crate) async fn claim_retainer(
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<(), ApiError> {
    db.register_retainer(id, user.id, user.name).await?;
    Ok(())
}

pub(crate) async fn unclaim_retainer(
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<(), ApiError> {
    db.remove_owned_retainer(user.id, id).await?;
    Ok(())
}

pub(crate) async fn reorder_retainer(
    user: AuthDiscordUser,
    State(db): State<UltrosDb>,
    Json(data): Json<Vec<OwnedRetainer>>,
) -> Result<Json<()>, ApiError> {
    for retainer in data {
        db.update_owned_retainer(user.id as i64, retainer.id, |mut existing_retainer| {
            existing_retainer.weight = ActiveValue::Set(retainer.weight);
            existing_retainer
        })
        .await?;
    }
    Ok(Json(()))
}
