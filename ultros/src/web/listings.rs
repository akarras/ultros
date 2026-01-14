use anyhow::Error;
use axum::Json;
use axum::extract::{Path, State};
use axum::response::Redirect;
use futures::future::{try_join, try_join_all};
use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tracing::debug;
use ultros_api_types::websocket::ListingEventData;
use ultros_api_types::{ActiveListing, CurrentlyShownItem, Retainer};
use ultros_db::{UltrosDb, world_cache::WorldCache};
use universalis::{ItemId, ListingView, UniversalisClient, WorldId};

use crate::event::{EventSenders, EventType};
use crate::web::error::ApiError;
use crate::web::error::WebError;

#[tracing::instrument(skip(db, world_cache))]
pub(crate) async fn world_item_listings(
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    Path((world, item_id)): Path<(String, i32)>,
) -> Result<axum::Json<CurrentlyShownItem>, WebError> {
    let selected_value = world_cache.lookup_value_by_name(&world)?;
    let worlds = world_cache
        .get_all_worlds_in(&selected_value)
        .ok_or_else(|| Error::msg("Unable to get worlds"))?;
    let db_clone = db.clone();
    let world_iter = worlds.iter().copied();
    let (listings, sales) = try_join(
        db_clone.get_all_listings_in_worlds_with_retainers(&worlds, ItemId(item_id)),
        db.get_sale_history_from_multiple_worlds(world_iter, item_id, 200),
    )
    .await
    .inspect_err(|e| tracing::error!(error = ?e, "Error getting listings"))?;
    let currently_shown = CurrentlyShownItem {
        listings: listings
            .into_iter()
            .flat_map(|(l, r)| r.map(|r| (l.into(), r.into())))
            .collect(),
        sales: sales.into_iter().map(|s| s.into()).collect(),
    };
    Ok(axum::Json(currently_shown))
}

pub(crate) async fn refresh_world_item_listings(
    State(db): State<UltrosDb>,
    State(senders): State<EventSenders>,
    Path((world, item_id)): Path<(String, i32)>,
    State(world_cache): State<Arc<WorldCache>>,
) -> Result<Redirect, WebError> {
    let lookup = world_cache.lookup_value_by_name(&world)?;
    let all_worlds = world_cache
        .get_all_worlds_in(&lookup)
        .ok_or_else(|| anyhow::Error::msg("Unable to get worlds"))?;
    let world_clone = world.clone();
    let future = tokio::spawn(async move {
        let client = UniversalisClient::new("ultros");
        let current_data = client
            .marketboard_current_data(&world_clone, &[item_id])
            .await?;
        // we can potentially get listings from multiple worlds from this call so we should group listings by world
        let listings = match current_data {
            universalis::MarketView::SingleView(v) => v.listings,
            universalis::MarketView::MultiView(_) => {
                return Result::<_, anyhow::Error>::Err(anyhow::Error::msg(
                    "multiple listings returned?",
                ));
            }
        };

        // now ensure we insert all worlds into the map to account for empty worlds
        let listings_by_world: HashMap<u16, Vec<ListingView>> =
            all_worlds.into_iter().map(|w| (w as u16, vec![])).collect();
        let first_key = if listings_by_world.len() == 1 {
            listings_by_world.keys().next().copied()
        } else {
            None
        };
        let listings_by_world = listings
            .into_iter()
            .flat_map(|l| {
                if let Some(key) = first_key {
                    Some((key, l))
                } else {
                    l.world_id.map(|w| (w, l))
                }
            })
            .fold(listings_by_world, |mut m, (w, l)| {
                m.entry(w).or_default().push(l);
                m
            });
        debug!("manually refreshed worlds: {listings_by_world:?}");
        for (world_id, listings) in listings_by_world {
            let (added, removed) = db
                .update_listings(listings, ItemId(item_id), WorldId(world_id as i32))
                .await?;
            senders
                .listings
                .send(EventType::Add(Arc::new(ListingEventData {
                    item_id,
                    world_id: world_id.into(),
                    listings: added,
                })))?;
            senders
                .listings
                .send(EventType::Remove(Arc::new(ListingEventData {
                    item_id,
                    world_id: world_id.into(),
                    listings: removed,
                })))?;
        }
        Ok(())
    });
    let _ = timeout(Duration::from_secs(1), future).await?;
    Ok(Redirect::to(&format!("/item/{world}/{item_id}")))
}

/// Does a bulk lookup of item listings. Will not preserve order.
pub(crate) async fn bulk_item_listings(
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    Path((world, item_ids)): Path<(String, String)>,
) -> Result<Json<HashMap<i32, Vec<(ActiveListing, Option<Retainer>)>>>, ApiError> {
    let world_lookup = world_cache.lookup_value_by_name(&world)?;
    // borrow our worlds list & db now so it can be shared into the lookup futures
    let worlds = &world_cache
        .get_all_worlds_in(&world_lookup)
        .ok_or(anyhow::anyhow!("Invalid world"))?;
    let db = &db;
    // get item ids
    let item_ids: HashSet<i32> = item_ids.split(',').map(|id| id.parse()).try_collect()?;
    // now perform lookups for all the listings for each world/item pair
    let listings = try_join_all(item_ids.into_iter().map(|item| async move {
        db.get_all_listings_in_worlds_with_retainers(worlds, ItemId(item))
            .await
            // map the result to have the item id at the front.
            .map(|res| (item, res))
    }))
    .await?;
    // now convert the database models to API types.
    let listings = listings
        .into_iter()
        .map(|(id, l)| {
            (
                id,
                l.into_iter()
                    .map(|(listing, retainer)| {
                        (ActiveListing::from(listing), retainer.map(Retainer::from))
                    })
                    .collect(),
            )
        })
        .collect();
    Ok(Json(listings))
}

pub(crate) async fn listings_redirect(Path((world, id)): Path<(String, i32)>) -> Redirect {
    Redirect::permanent(&format!("/item/{world}/{id}"))
}
