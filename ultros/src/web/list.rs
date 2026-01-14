use axum::Json;
use axum::extract::{Path, State};
use futures::StreamExt;
use futures::future::try_join_all;
use futures::stream::TryStreamExt;
use std::sync::Arc;
use ultros_api_types::ActiveListing;
use ultros_api_types::list::{CreateList, List, ListItem};
use ultros_db::common_type_conversions::ApiConversionError;
use ultros_db::world_cache::{AnySelector, WorldCache};
use ultros_db::{ActiveValue, UltrosDb};
use universalis::ItemId;

use crate::web::error::ApiError;
use crate::web::oauth::AuthDiscordUser;

pub(crate) async fn get_lists(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
) -> Result<Json<Vec<List>>, ApiError> {
    let lists = db
        .get_lists_for_user(user.id as i64)
        .await?
        .into_iter()
        .map(List::try_from)
        .collect::<Result<Vec<_>, ApiConversionError>>()?;
    Ok(Json(lists))
}

pub(crate) async fn get_list(
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<(List, Vec<ListItem>)>, ApiError> {
    let (list, list_items) = futures::future::try_join(
        db.get_list(id, user.id as i64),
        db.get_list_items(id, user.id as i64),
    )
    .await?;
    let list_items = list_items
        .into_iter()
        .map(ListItem::from)
        .collect::<Vec<_>>();
    let list = List::try_from(list)?;
    Ok(Json((list, list_items)))
}

pub(crate) async fn get_list_with_listings(
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<(List, Vec<(ListItem, Vec<ActiveListing>)>)>, ApiError> {
    let (list, list_items) = futures::future::try_join(
        db.get_list(id, user.id as i64),
        db.get_list_items(id, user.id as i64),
    )
    .await?;
    // tbd: probably don't need to send clients all listings, but for now keep it this way.
    let selector = AnySelector::try_from(&list)?;
    let world = world_cache.lookup_selector(&selector)?;
    let world_ids = world_cache
        .get_all_worlds_in(&world)
        .ok_or(anyhow::anyhow!("Bad world id"))?;
    // borrow these for use inside the closure
    let world_ids = &world_ids;
    let db = &db;
    let list_items = futures::stream::iter(list_items.into_iter().map(|list| async move {
        // get alll the listings that match our item list
        let listings = db
            .get_all_listings_in_worlds(world_ids, ItemId(list.item_id))
            .await;
        listings.map(|listings| {
            // return this as a tuple and bring the list that we moved vec
            (
                ListItem::from(list),
                // convert our new active listing to the API types
                listings.into_iter().map(ActiveListing::from).collect(),
            )
        })
    }))
    .buffered(2)
    .try_collect()
    .await?;

    Ok(Json((List::try_from(list)?, list_items)))
}

pub(crate) async fn delete_list(
    State(db): State<UltrosDb>,
    Path(list_id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<()>, ApiError> {
    db.delete_list(list_id, user.id as i64).await?;
    Ok(Json(()))
}

pub(crate) async fn create_list(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(list): Json<CreateList>,
) -> Result<Json<()>, ApiError> {
    let discord_user = db.get_or_create_discord_user(user.id, user.name).await?;
    db.create_list(discord_user, list.name, Some(list.wdr_filter.into()))
        .await?;
    Ok(Json(()))
}

pub(crate) async fn edit_list(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(list): Json<List>,
) -> Result<Json<()>, ApiError> {
    db.update_list(list.id, user.id as i64, |ulist| {
        ulist.datacenter_id = ActiveValue::Set(match list.wdr_filter {
            ultros_api_types::world_helper::AnySelector::Datacenter(dc) => Some(dc),
            _ => None,
        });
        ulist.region_id = ActiveValue::Set(match list.wdr_filter {
            ultros_api_types::world_helper::AnySelector::Region(region) => Some(region),
            _ => None,
        });
        ulist.world_id = ActiveValue::Set(match list.wdr_filter {
            ultros_api_types::world_helper::AnySelector::World(world) => Some(world),
            _ => None,
        });
        ulist.name = ActiveValue::Set(list.name);
    })
    .await?;
    Ok(Json(()))
}

pub(crate) async fn post_item_to_list(
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
    Json(item): Json<ListItem>,
) -> Result<Json<()>, ApiError> {
    let list = db.get_list(id, user.id as i64).await?;
    let ListItem {
        item_id,
        hq,
        quantity,
        acquired,
        ..
    } = item;
    db.add_item_to_list(&list, user.id as i64, item_id, hq, quantity, acquired)
        .await?;
    Ok(Json(()))
}

pub(crate) async fn post_items_to_list(
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
    Json(items): Json<Vec<ListItem>>,
) -> Result<Json<()>, ApiError> {
    let list = db.get_list(id, user.id as i64).await?;

    let _list = db
        .add_items_to_list(&list, user.id as i64, items.into_iter().map(|i| i.into()))
        .await?;
    Ok(Json(()))
}

pub(crate) async fn edit_list_item(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(item): Json<ListItem>,
) -> Result<Json<()>, ApiError> {
    let item = item.into();
    db.update_list_item(item, user.id as i64).await?;
    Ok(Json(()))
}

pub(crate) async fn delete_list_item(
    State(db): State<UltrosDb>,
    Path(id): Path<i32>,
    user: AuthDiscordUser,
) -> Result<Json<()>, ApiError> {
    db.remove_item_from_list(user.id as i64, id).await?;
    Ok(Json(()))
}

pub(crate) async fn delete_multiple_list_items(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(ids): Json<Vec<i32>>,
) -> Result<Json<()>, ApiError> {
    try_join_all(
        ids.into_iter()
            .map(|id| db.remove_item_from_list(user.id as i64, id)),
    )
    .await?;
    Ok(Json(()))
}
