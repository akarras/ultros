#![recursion_limit = "256"]
use any_spawner::Executor;
use anyhow::{Result, anyhow};
use futures::{Future, future::join};
use gloo_net::http::Request;
use leptos::leptos_dom::helpers::set_timeout;
use leptos::{prelude::*, task::spawn_local};
use log::{Level, error, info};
use rexie::{ObjectStore, Rexie, Store, Transaction, TransactionMode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use ultros_api_types::{
    bootstrap::Bootstrap, user::UserData, world::WorldData, world_helper::WorldHelper,
};
use ultros_app::*;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::{JsCast, JsValue};

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

fn get_i18n_lang() -> String {
    #[allow(unused_mut)]
    let mut default_lang = "en".to_string();
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let window = leptos::prelude::window();
        if let Some(document) = window.document() {
            if let Some(html_doc) = document.dyn_ref::<web_sys::HtmlDocument>() {
                if let Ok(cookie) = html_doc.cookie() {
                    for part in cookie.split(';') {
                        let part = part.trim();
                        if let Some(stripped) = part.strip_prefix("i18n_pref_locale=") {
                            default_lang = stripped.to_string();
                        }
                    }
                }
            }
        }
    }
    match default_lang.as_str() {
        "en" | "ja" | "de" | "fr" | "cn" | "ko" | "tc" => default_lang,
        _ => "en".to_string(),
    }
}

async fn init_data() -> anyhow::Result<Vec<u8>> {
    let version = xiv_gen::data_version();
    let lang = get_i18n_lang();
    let response = Request::get(&format!("/static/data/{}/{}.rkyv", version, lang))
        .send()
        .await?
        .binary()
        .await?;
    xiv_gen_db::try_init(&response)?;
    Ok(response)
}

async fn try_populate_xiv_gen_data_internal(rexie: &Rexie) -> anyhow::Result<()> {
    // load local storage data for the current game version, if we don't have it get it from the server, store it, and init db
    let version = format!("{}-{}", xiv_gen::data_version(), get_i18n_lang());
    {
        let (transaction, game_data) = open_transaction(rexie).await?;
        #[allow(clippy::collapsible_if)]
        if let Ok(Some(value)) = game_data.get(version.clone().into()).await {
            if !value.is_null() && !value.is_undefined() {
                match serde_wasm_bindgen::from_value::<Data>(value) {
                    Ok(value) => match xiv_gen_db::try_init(&value.data) {
                        Ok(()) => return Ok(()),
                        Err(e) => error!("Error initializing using data {e}"),
                    },
                    Err(e) => error!("Error converting indexdb to data {e}"),
                };

                error!("failed to deserialize data. removing {version}");
                game_data
                    .delete(version.clone().into())
                    .await
                    .map_err(|_| anyhow!("error deleting?"))?;
                transaction
                    .done()
                    .await
                    .map_err(|e| anyhow!("error closing first transaction {e}"))?;
            }
        }
    }
    let response = init_data().await?;
    let data = serde_wasm_bindgen::to_value(&Data {
        version: version.to_string(),
        data: response.clone(),
    })
    .map_err(|e| anyhow!("error serializing data {e}"))?;
    let (transaction, game_data) = open_transaction(rexie).await?;
    // allow the app to run if we can init
    // soft fail if we can't store
    game_data
        .clear()
        .await
        .map_err(|e| anyhow!("error clearing store {e}"))?;
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

pub async fn try_populate_xiv_gen_data() -> anyhow::Result<()> {
    if let Ok(rexie) = try_build_db().await {
        if let Err(_e) = retry(|| try_populate_xiv_gen_data_internal(&rexie), 3).await {
            let _ = init_data().await?;
        }
    } else {
        let _ = init_data().await?;
    }
    // Need to trigger a reactive update here if data() changed
    // In practice try_init already updates the atomic XIV_DATA state
    // We should trigger a UI update.
    Ok(())
}

async fn populate_xiv_gen_data() -> anyhow::Result<()> {
    try_populate_xiv_gen_data().await
}

async fn fetch_world_data_once() -> Result<Arc<WorldHelper>, anyhow::Error> {
    let json: WorldData = Request::get("/api/v1/world_data")
        .send()
        .await
        .map_err(|e| anyhow!("failed to fetch world data: {e}"))?
        .json()
        .await
        .map_err(|e| anyhow!("failed to parse world data: {e}"))?;
    Ok(Arc::new(WorldHelper::from(json)))
}

async fn get_world_data() -> Result<Arc<WorldHelper>, anyhow::Error> {
    retry(fetch_world_data_once, 3).await
}

async fn fetch_region_once() -> Result<String, anyhow::Error> {
    Request::get("/api/v1/detectregion")
        .send()
        .await
        .map_err(|e| anyhow!("failed to fetch region: {e}"))?
        .text()
        .await
        .map_err(|e| anyhow!("failed to read region response: {e}"))
}

async fn get_region() -> String {
    match retry(fetch_region_once, 3).await {
        Ok(text) => text,
        Err(e) => {
            error!("region detection failed after retries: {e}");
            String::new()
        }
    }
}

/// Best-effort fetch of the current user when the SSR bootstrap is missing.
///
/// Returns `Some(user)` if logged in, `None` if the server says we're not
/// authenticated (401 / etc.). Any other failure also collapses to `None` —
/// the hydration view tree just renders as logged-out, which matches what
/// the SSR side would have rendered for an unauthenticated request.
async fn fetch_current_user_fallback() -> Option<UserData> {
    let response = match Request::get("/api/v1/current_user").send().await {
        Ok(r) => r,
        Err(e) => {
            error!("current_user fetch failed: {e}");
            return None;
        }
    };
    if !response.ok() {
        return None;
    }
    match response.json::<UserData>().await {
        Ok(user) => Some(user),
        Err(e) => {
            error!("current_user parse failed: {e}");
            None
        }
    }
}

fn set_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        console_error_panic_hook::hook(panic_info);
        report_rust_panic(panic_info);
    }));
}

fn report_rust_panic(panic_info: &std::panic::PanicHookInfo<'_>) {
    let message = panic_info
        .payload()
        .downcast_ref::<&str>()
        .copied()
        .map(String::from)
        .or_else(|| panic_info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "Rust WASM panic".to_string());
    let location = panic_info
        .location()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Defer the JS call so it runs after the current task pops off the
    // wasm-bindgen-futures executor. If a panic fires mid-poll and we call
    // the reporter synchronously, any executor re-entry from the JS side
    // (a promise callback, another spawned future being woken) hits the
    // still-borrowed task-queue RefCell and triggers a secondary
    // `RefCell already borrowed` panic — see GlitchTip issues 909/881/915.
    set_timeout(
        move || {
            let global = js_sys::global();
            let Ok(reporter) =
                js_sys::Reflect::get(&global, &JsValue::from_str("__ultrosReportRustPanic"))
            else {
                return;
            };
            let Some(reporter) = reporter.dyn_ref::<js_sys::Function>() else {
                return;
            };
            let _ = reporter.call2(
                &JsValue::NULL,
                &JsValue::from_str(&message),
                &JsValue::from_str(&location),
            );
        },
        std::time::Duration::from_millis(0),
    );
}

/// Read the bootstrap blob the SSR handler injects as
/// `window.__ULTROS_BOOTSTRAP__`. Returns `None` if the script wasn't there or
/// failed to decode — callers should fall back to the legacy fetch path so
/// the client stays robust to old / mismatched HTML.
fn read_bootstrap() -> Option<Bootstrap> {
    use wasm_bindgen::{JsCast, JsValue};
    let window = leptos::prelude::window();
    let window_value: &JsValue = window.unchecked_ref();
    let value =
        js_sys::Reflect::get(window_value, &JsValue::from_str("__ULTROS_BOOTSTRAP__")).ok()?;
    if value.is_undefined() || value.is_null() {
        return None;
    }
    match serde_wasm_bindgen::from_value::<Bootstrap>(value) {
        Ok(b) => Some(b),
        Err(e) => {
            error!("Failed to decode __ULTROS_BOOTSTRAP__: {e}");
            None
        }
    }
}

fn dispatch_boot_event(name: &str) {
    if let Some(window) = web_sys::window()
        && let Ok(event) = web_sys::Event::new(name)
    {
        let _ = window.dispatch_event(&event);
    }
}

#[wasm_bindgen]
pub fn hydrate() {
    set_panic_hook();
    // tracing_wasm::set_as_global_default();
    console_log::init_with_level(Level::Info).unwrap();
    // check that we have the right client version data
    let _ = Executor::init_wasm_bindgen();
    log::info!("hydrate mode - hydrating");
    dispatch_boot_event("ultros:wasm-loaded");
    spawn_local(async move {
        info!("fetching..");
        // Use the SSR-injected bootstrap when available; only fall back to
        // network requests if it's missing (e.g. stale cached HTML).
        let bootstrap = read_bootstrap();
        let (xiv_data, worlds, region, current_user) = if let Some(b) = bootstrap {
            let xiv_data = populate_xiv_gen_data().await;
            (
                xiv_data,
                Ok(Arc::new(WorldHelper::from(b.world_data))),
                b.region,
                Some(b.current_user),
            )
        } else {
            info!(
                "bootstrap missing — falling back to HTTP for world_data + region + current_user"
            );
            // Fetch current_user alongside the other fallbacks so we can
            // provide BootstrapUser context before hydration runs. Otherwise
            // the SSR DOM (rendered with the server's view of auth state)
            // and the client view tree (auth state still loading) diverge
            // and tachys hydration panics at hydration.rs:163.
            let (xiv_data, ((worlds, region), current_user)) = join(
                populate_xiv_gen_data(),
                join(
                    join(get_world_data(), get_region()),
                    fetch_current_user_fallback(),
                ),
            )
            .await;
            (xiv_data, worlds, region, Some(current_user))
        };

        // The entire client view tree reads game data through
        // `xiv_gen_db::data()` (via `tracked_data()`, used across ~37 route and
        // component files). If the `.rkyv` data archive failed to populate —
        // an ad blocker or corporate proxy dropping the binary fetch, a flaky
        // network, or a crawler like Baiduspider that won't fetch it — then
        // `data()` panics with "XIV data not initialized" the instant we
        // hydrate (xiv-gen-db/src/lib.rs), taking down the page and firing a
        // WASM panic to GlitchTip (issue #6765). The SSR HTML was rendered
        // server-side with the embedded data, so it already shows the correct
        // page; hydrating with no client data could only panic — or, worse,
        // silently diverge into a hydration mismatch. Leave the static SSR
        // content in place instead. Navigation still works as full page loads,
        // each re-rendered server-side with real data.
        if let Err(e) = xiv_data {
            error!(
                "XIV game data failed to load; leaving server-rendered content un-hydrated: {e}"
            );
            // Resolve the boot-progress indicator, which waits on this event
            // and would otherwise show a "taking longer than expected" error
            // once its watchdog fires — the SSR page is the final state here.
            dispatch_boot_event("ultros:hydrated");
            return;
        }

        // The SSR response can end early — a stalled render, a dropped
        // connection, a proxy cutting the stream — and the browser still
        // reports `readyState === "complete"` for whatever it managed to
        // parse. The bootstrap `HydrationScripts` emits is a deferred module
        // script in `<head>`, so it fires on that truncated document just the
        // same, and `hydrate_body` then walks a DOM missing nearly everything
        // it expects: tachys hits `failed_to_cast_element` and panics at
        // `hydration.rs:163`, which cascades into `RefCell already borrowed`
        // from the wasm-bindgen-futures executor. That is GlitchTip #6831 —
        // measured on prod, every panicking load hydrated with 2 body children
        // where a healthy load has 9-12, and serving a deliberately truncated
        // copy of the same page reproduced it 4/4 against an intact-page
        // control of 0/4.
        //
        // `shell()` renders `SSR_END_SENTINEL_ID` as the last child of
        // `<body>`, so its absence means the document we were handed is
        // incomplete. There is nothing coherent to hydrate against; keep the
        // partial server-rendered markup rather than panicking on it.
        if document().get_element_by_id(SSR_END_SENTINEL_ID).is_none() {
            error!(
                "SSR document truncated (missing #{SSR_END_SENTINEL_ID}); \
                 skipping hydration to avoid a tachys hydration panic"
            );
            // Deliberately *not* dispatching "ultros:hydrated": this page is
            // genuinely broken, and letting the boot-progress watchdog surface
            // its existing "taking longer than expected — reload" affordance
            // gives the reader a way out. A truncated response is transient,
            // so a reload almost always succeeds.
            return;
        }

        info!("hydrating body");
        let world_data = match worlds {
            Ok(worlds) => LocalWorldData(Ok(worlds)),
            Err(e) => {
                error!("failed to load world data: {e}");
                LocalWorldData::failed(e.to_string())
            }
        };
        hydrate_body(move || {
            let world_data = world_data.clone();
            let region = region.clone();
            let current_user = current_user.clone();
            provide_context(GuessedRegion(region));
            provide_context(world_data);
            if let Some(current_user) = current_user {
                provide_context(BootstrapUser(current_user));
            }
            view! { <App /> }
        });
        dispatch_boot_event("ultros:hydrated");
    });
}
