use anyhow::{anyhow, Result};
use futures::{future::join, Future};
use gloo_net::http::Request;
use leptos::*;
use leptos_meta::provide_meta_context;
use rexie::{ObjectStore, Rexie, Store, Transaction, TransactionMode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use ultros_api_types::{world::WorldData, world_helper::WorldHelper};
use ultros_app::*;
use wasm_bindgen::prelude::wasm_bindgen;

#[derive(Serialize, Deserialize)]
struct Data {
    version: String,
    #[serde(with = "serde_bytes")]
    data: Vec<u8>,
}

async fn retry<F, Fut, O, E>(fut: F, max_retries: i32) -> Result<O, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<O, E>>,
{
    let mut last_error = None;
    for _attempt in 1..=max_retries {
        let future = fut();
        last_error = match future.await {
            Ok(value) => return Ok(value),
            Err(e) => Some(e),
        };
    }
    Err(last_error.unwrap())
}

async fn open_transaction(rexie: &Rexie) -> Result<(Transaction, Store)> {
    let transaction = rexie
        .transaction(&["game_data"], TransactionMode::ReadWrite)
        .map_err(|e| anyhow!("failed to open db {e}"))?;
    let game_data = transaction
        .store("game_data")
        .map_err(|e| anyhow!("failed to open store {e}"))?;
    Ok((transaction, game_data))
}

async fn try_populate_xiv_gen_data<'a>(rexie: &Rexie) -> anyhow::Result<()> {
    // load local storage data for the current game version, if we don't have it get it from the server, store it, and init db
    let version = xiv_gen::data_version();
    {
        let (transaction, game_data) = open_transaction(rexie).await?;
        if let Ok(value) = game_data.get(&version.into()).await {
            match serde_wasm_bindgen::from_value::<Data>(value) {
                Ok(value) => match xiv_gen_db::try_init(&value.data) {
                    Ok(()) => return Ok(()),
                    Err(e) => error!("Error initializing using data {e}"),
                },
                Err(e) => error!("Error converting indexdb to data {e}"),
            };

            error!("failed to deserialize data. removing {version}");
            game_data
                .delete(&version.into())
                .await
                .map_err(|_| anyhow!("error deleting?"))?;
            transaction
                .done()
                .await
                .map_err(|e| anyhow!("error closing first transaction {e}"))?;
        }
    }
    let response = Request::get(&["/static/data/", version, ".bincode"].concat())
        .send()
        .await?
        .binary()
        .await?;
    let data = serde_wasm_bindgen::to_value(&Data {
        version: version.to_string(),
        data: response.clone(),
    })
    .map_err(|e| anyhow!("error serializing data {e}"))?;
    let (transaction, game_data) = open_transaction(rexie).await?;
    // allow the app to run if we can init
    xiv_gen_db::try_init(&response)?;
    // soft fail if we can't store
    for (key, _) in game_data
        .get_all(None, None, None, None)
        .await
        .map_err(|e| anyhow!("error getting data {e}"))?
    {
        game_data
            .delete(&key)
            .await
            .map_err(|e| anyhow!("error deleting {e}"))?;
    }
    if let Err(e) = game_data
        .add(&data, None)
        .await
        .map_err(|e| anyhow!("Error adding game data {e}"))
    {
        error!("Failed to store data {e}");
    }
    if let Err(e) = transaction
        .done()
        .await
        .map_err(|_| anyhow!("error waiting for tranasction to finish"))
    {
        error!("failed to finish transaction {e}");
    }
    Ok(())
}

async fn try_build_db() -> Result<Rexie> {
    Rexie::builder("ultros")
        .version(1)
        .add_object_store(ObjectStore::new("game_data").key_path("version"))
        .build()
        .await
        .map_err(|e| anyhow!("failed to build db {e}"))
}

async fn populate_xiv_gen_data<'a>() {
    let rexie = try_build_db().await.unwrap();
    retry(|| try_populate_xiv_gen_data(&rexie), 3)
        .await
        .unwrap();
}

async fn get_world_data() -> Arc<WorldHelper> {
    let json: WorldData = gloo_net::http::Request::get("/api/v1/world_data")
        .send()
        .await
        .map_err(|e| {
            log!("{e}");
            e
        })
        .unwrap()
        .json()
        .await
        .unwrap();
    Arc::new(WorldHelper::from(json))
}

#[wasm_bindgen]
pub fn hydrate() {
    _ = console_log::init_with_level(log::Level::Debug);
    console_error_panic_hook::set_once();
    // check that we have the right client version data

    log::info!("hydrate mode - hydrating");
    leptos::spawn_local(async move {
        let (_, worlds) = join(populate_xiv_gen_data(), get_world_data()).await;
        leptos::mount_to_body(move || {
            let worlds = worlds.clone();
            let worlds = Ok(worlds);
            provide_meta_context();
            view! { <App worlds/> }
        });
    });
}
