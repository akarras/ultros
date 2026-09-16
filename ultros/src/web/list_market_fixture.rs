//! Real database market rows for the opt-in Lists acceptance gate. This module
//! is compiled only with `test-auth`; it never replaces the listing read path.
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, post},
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, QueryFilter,
    QueryOrder, Set, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use ultros_db::{
    UltrosDb,
    entity::{active_listing, datacenter, retainer, retainer_city, world},
};

use super::{error::ApiError, oauth::AuthDiscordUser, state::WebState};

const ITEMS: [i32; 2] = [5056, 5057];

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Scenario {
    Baseline,
    MultiItem,
    Partial,
    Empty,
    Changed,
    Disappeared,
}

#[derive(Deserialize)]
struct Request {
    token: String,
    region_id: i32,
    scenario: Scenario,
}

#[derive(Serialize)]
struct Manifest {
    token: String,
    region_id: i32,
    isolated: bool,
    worlds: Vec<world::Model>,
    item_ids: [i32; 2],
    item_names: Vec<String>,
    listings: Vec<active_listing::Model>,
}

fn fixture_name(owner: u64, token: &str) -> Result<String, &'static str> {
    if token.len() != 32 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Fixture token must be 32 hexadecimal characters");
    }
    Ok(format!("qa1478_{owner}_{token}"))
}

async fn seed(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Json(request): Json<Request>,
) -> Result<Json<Manifest>, ApiError> {
    if !crate::test_market_isolation::enabled() {
        return Err(ApiError::BadRequest(
            "Market fixtures require ULTROS_TEST_MARKET_ISOLATION=true at startup",
        ));
    }
    let name = fixture_name(user.id, &request.token).map_err(ApiError::BadRequest)?;
    let item_names = ITEMS
        .iter()
        .map(|id| {
            xiv_gen_db::data()
                .items
                .get(&xiv_gen::ItemId(*id))
                .map(|item| item.name.to_string())
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(ApiError::BadRequest(
            "Fixture item is absent from the game catalog",
        ))?;
    let txn = db.get_connection().begin().await?;
    // Serialize fixture setup for the region; a second test cannot pass the
    // empty-stock check while the first setup is still uncommitted.
    txn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT pg_advisory_xact_lock($1, $2)",
        [1478.into(), request.region_id.into()],
    ))
    .await?;
    let dcs = datacenter::Entity::find()
        .filter(datacenter::Column::RegionId.eq(request.region_id))
        .order_by_asc(datacenter::Column::Id)
        .all(&txn)
        .await?;
    let worlds = world::Entity::find()
        .filter(world::Column::DatacenterId.is_in(dcs.iter().map(|dc| dc.id)))
        .order_by_asc(world::Column::Id)
        .all(&txn)
        .await?;
    // Keep the two-stack world first even before home-first travel ordering
    // ships; acceptance must not depend on a future planner behavior.
    let first = worlds
        .iter()
        .find_map(|candidate| {
            let group = worlds
                .iter()
                .filter(|world| world.datacenter_id == candidate.datacenter_id)
                .collect::<Vec<_>>();
            (group.len() >= 2).then_some(group)
        })
        .ok_or(ApiError::BadRequest(
            "Fixture region needs two worlds in one datacenter",
        ))?;
    let other = worlds
        .iter()
        .find(|world| world.datacenter_id != first[0].datacenter_id && world.id > first[0].id)
        .ok_or(ApiError::BadRequest(
            "Fixture region needs a later-ID world in another datacenter",
        ))?;
    let chosen = vec![first[0].clone(), first[1].clone(), other.clone()];
    let existing = retainer::Entity::find()
        .filter(retainer::Column::Name.eq(&name))
        .all(&txn)
        .await?;
    let stock = active_listing::Entity::find()
        .filter(active_listing::Column::WorldId.is_in(worlds.iter().map(|world| world.id)))
        .filter(active_listing::Column::ItemId.is_in(ITEMS))
        .all(&txn)
        .await?;
    if stock.iter().any(|listing| {
        !existing
            .iter()
            .any(|retainer| listing.retainer_id == retainer.id)
    }) {
        return Err(ApiError::BadRequest(
            "Fixture items already have market data in this region; use an isolated empty test database",
        ));
    }
    let retainers = if existing.is_empty() {
        if matches!(request.scenario, Scenario::Changed | Scenario::Disappeared) {
            return Err(ApiError::BadRequest(
                "Create the baseline fixture before changing it",
            ));
        }
        let city = retainer_city::Entity::find()
            .order_by_asc(retainer_city::Column::Id)
            .one(&txn)
            .await?
            .ok_or(ApiError::BadRequest("Retainer cities are not initialized"))?;
        let mut retainers = Vec::new();
        for world in &chosen {
            retainers.push(
                retainer::ActiveModel {
                    name: Set(name.clone()),
                    world_id: Set(world.id),
                    retainer_city_id: Set(city.id),
                    ..Default::default()
                }
                .insert(&txn)
                .await?,
            );
        }
        retainers
    } else {
        if existing.len() != chosen.len()
            || chosen.iter().any(|world| {
                !existing
                    .iter()
                    .any(|retainer| retainer.world_id == world.id)
            })
        {
            return Err(ApiError::BadRequest(
                "Fixture token belongs to a different region",
            ));
        }
        existing
    };
    let retainer_ids: Vec<_> = retainers.iter().map(|retainer| retainer.id).collect();
    if matches!(request.scenario, Scenario::Changed | Scenario::Disappeared) {
        let row = active_listing::Entity::find()
            .filter(active_listing::Column::RetainerId.is_in(retainer_ids.clone()))
            .order_by_asc(active_listing::Column::Id)
            .one(&txn)
            .await?
            .ok_or(ApiError::BadRequest(
                "The fixture has no listing left to change",
            ))?;
        if request.scenario == Scenario::Changed {
            let mut model: active_listing::ActiveModel = row.into();
            model.price_per_unit = Set(30);
            model.update(&txn).await?;
        } else {
            active_listing::Entity::delete_by_id(row.id)
                .exec(&txn)
                .await?;
        }
    } else {
        active_listing::Entity::delete_many()
            .filter(active_listing::Column::RetainerId.is_in(retainer_ids.clone()))
            .exec(&txn)
            .await?;
        let mut offers = match request.scenario {
            Scenario::Baseline | Scenario::MultiItem => vec![
                (0, 0, false, 2, 10),
                (0, 0, true, 2, 20),
                (0, 1, false, 3, 12),
                (0, 2, true, 4, 25),
            ],
            Scenario::Partial => vec![(0, 0, false, 2, 10)],
            _ => vec![],
        };
        if request.scenario == Scenario::MultiItem {
            offers.push((1, 0, false, 5, 7));
        }
        for (index, (item_index, world_index, hq, quantity, price)) in
            offers.into_iter().enumerate()
        {
            let retainer = retainers
                .iter()
                .find(|retainer| retainer.world_id == chosen[world_index].id)
                .ok_or(ApiError::BadRequest("Fixture retainer world is missing"))?;
            active_listing::ActiveModel {
                world_id: Set(chosen[world_index].id),
                item_id: Set(ITEMS[item_index]),
                retainer_id: Set(retainer.id),
                price_per_unit: Set(price),
                quantity: Set(quantity),
                hq: Set(hq),
                timestamp: Set(chrono::Utc::now().naive_utc()),
                listing_id: Set(Some(format!("qa1478-{}-{index}", request.token))),
                materia: Set(None),
                stain_id: Set(None),
                creator_name: Set(None),
                is_crafted: Set(false),
                on_mannequin: Set(false),
                ..Default::default()
            }
            .insert(&txn)
            .await?;
        }
    }
    let listings = active_listing::Entity::find()
        .filter(active_listing::Column::RetainerId.is_in(retainer_ids))
        .order_by_asc(active_listing::Column::Id)
        .all(&txn)
        .await?;
    txn.commit().await?;
    Ok(Json(Manifest {
        token: request.token,
        region_id: request.region_id,
        isolated: crate::test_market_isolation::enabled(),
        worlds: chosen,
        item_ids: ITEMS,
        item_names,
        listings,
    }))
}

async fn cleanup(
    State(db): State<UltrosDb>,
    user: AuthDiscordUser,
    Path(token): Path<String>,
) -> Result<Json<bool>, ApiError> {
    let name = fixture_name(user.id, &token).map_err(ApiError::BadRequest)?;
    let txn = db.get_connection().begin().await?;
    for retainer in retainer::Entity::find()
        .filter(retainer::Column::Name.eq(name))
        .all(&txn)
        .await?
    {
        // Never clear a whole item/world: a real ingest may have arrived while
        // the fixture ran. Only this caller's synthetic retainer is ours.
        active_listing::Entity::delete_many()
            .filter(active_listing::Column::RetainerId.eq(retainer.id))
            .exec(&txn)
            .await?;
        retainer::Entity::delete_by_id(retainer.id)
            .exec(&txn)
            .await?;
    }
    txn.commit().await?;
    Ok(Json(true))
}

pub(super) fn routes() -> Router<WebState> {
    Router::new()
        .route("/test/list/market-fixture", post(seed))
        .route("/test/list/market-fixture/{token}", delete(cleanup))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_namespace_is_owner_bound_and_rejects_wildcards() {
        let token = "0123456789abcdef0123456789abcdef";
        assert_ne!(
            fixture_name(1, token).unwrap(),
            fixture_name(2, token).unwrap()
        );
        for invalid in [
            "",
            "%",
            "0123456789abcdef0123456789abcde_",
            "0123456789abcdef0123456789abcdef0",
        ] {
            assert!(fixture_name(1, invalid).is_err());
        }
    }
}
