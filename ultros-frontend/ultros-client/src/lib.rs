use leptos_meta::MetaContext;
use wasm_bindgen::prelude::wasm_bindgen;

use leptos::*;
use ultros_app::*;

#[wasm_bindgen]
pub fn hydrate() {
    _ = console_log::init_with_level(log::Level::Debug);
    console_error_panic_hook::set_once();

    log::info!("hydrate mode - hydrating");

    leptos::mount_to_body(move |cx| {
        provide_context(cx, MetaContext::default());
        view! { cx, <App/> }
    });
}
