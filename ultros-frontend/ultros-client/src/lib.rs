use std::sync::Arc;

use leptos_meta::provide_meta_context;
use ultros_api_types::{world::WorldData, world_helper::WorldHelper};
use wasm_bindgen::prelude::wasm_bindgen;

use leptos::*;
use ultros_app::*;

#[wasm_bindgen]
pub fn hydrate() {
    _ = console_log::init_with_level(log::Level::Debug);
    console_error_panic_hook::set_once();

    log::info!("hydrate mode - hydrating");
    leptos::spawn_local(async move {
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
        let worlds = Ok(Arc::new(WorldHelper::from(json)));
        leptos::mount_to_body(move || {
            let worlds = worlds.clone();
            provide_meta_context();
            view! { <App worlds/> }
        });
    });
}
