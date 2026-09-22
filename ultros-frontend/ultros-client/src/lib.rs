#![recursion_limit = "256"]
use any_spawner::Executor;
use anyhow::{Result, anyhow};
use futures::{Future, future::join};
use gloo_net::http::Request;
use leptos::leptos_dom::helpers::set_timeout;
use leptos::{prelude::*, task::spawn_local};
use log::{Level, error, info};
use std::sync::Arc;
use ultros_api_types::{
    bootstrap::Bootstrap, user::UserData, world::WorldData, world_helper::WorldHelper,
};
use ultros_app::*;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::{JsCast, JsValue};

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

fn get_i18n_lang() -> String {
    #[allow(unused_mut)]
    let mut default_lang = "en".to_string();
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let window = leptos::prelude::window();
        if let Some(document) = window.document() {
            // SSR resolves explicit ?lang before cookies and browser language.
            // Load that exact game-data pack before hydration walks the DOM.
            if let Some(lang) = document
                .document_element()
                .and_then(|html| html.get_attribute("lang"))
                .filter(|lang| {
                    matches!(
                        lang.as_str(),
                        "en" | "ja" | "de" | "fr" | "cn" | "ko" | "tc"
                    )
                })
            {
                return lang;
            }
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

pub async fn try_populate_xiv_gen_data() -> anyhow::Result<()> {
    retry(
        || async {
            let response = Request::get(&xiv_gen_db::startup_url(&get_i18n_lang()))
                .send()
                .await?;
            if !response.ok() {
                return Err(anyhow!("game data request failed: {}", response.status()));
            }
            xiv_gen_db::try_init(&response.binary().await?)
        },
        3,
    )
    .await
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

/// `js_sys::Error` exposes no `stack` getter; it is a plain (non-standard
/// but universal) property, so read it reflectively.
fn error_stack(error: &js_sys::Error) -> String {
    js_sys::Reflect::get(error, &JsValue::from_str("stack"))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

fn set_panic_hook() {
    // V8 keeps only 10 frames by default. A Rust panic spends that many on
    // its own machinery (`begin_panic_handler`, `rust_panic_with_hook`, this
    // hook, `console_error_panic_hook`, the `Error()` import...) before the
    // panicking site appears, so the captured stack would never reach it.
    js_sys::Error::set_stack_trace_limit(&JsValue::from_f64(50.0));
    std::panic::set_hook(Box::new(|panic_info| {
        console_error_panic_hook::hook(panic_info);
        report_rust_panic(panic_info);
    }));
}

fn report_rust_panic(panic_info: &std::panic::PanicHookInfo<'_>) {
    // Capture the stack NOW, on the panicking call stack. The reporter call
    // below is deferred to a timer, and a stack taken there is the timer
    // trampoline (`__wbg_call -> closure -> reporter`), not the panic site —
    // which is what every GlitchTip RustWasmPanic event carried until this
    // capture was added. The browser lists wasm frames as
    // `ultros.wasm:wasm-function[N]:0x...`; the Sentry `beforeSend` hook
    // resolves `N` to a Rust function name from `/pkg/<hash>/ultros.symbols`.
    let stack = error_stack(&js_sys::Error::new(""));
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
            let _ = reporter.call3(
                &JsValue::NULL,
                &JsValue::from_str(&message),
                &JsValue::from_str(&location),
                &JsValue::from_str(&stack),
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

// The service worker supplies only a generated anonymous shell on guest-list
// routes. There is no SSR tree in that document, so it must mount rather than
// hydrate. Keep the ordinary SSR truncation guard intact everywhere else.
fn is_offline_guest_shell() -> bool {
    js_sys::Reflect::get(
        &js_sys::global(),
        &JsValue::from_str("__ULTROS_OFFLINE_GUEST__"),
    )
    .ok()
    .and_then(|value| value.as_bool())
    .unwrap_or(false)
}

#[wasm_bindgen(module = "/../../ultros/static/guest-offline.mjs")]
extern "C" {
    fn prepare_guest_offline(catalog_url: &str, lang: &str);
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
        let offline_guest = is_offline_guest_shell();
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
        if !offline_guest && document().get_element_by_id(SSR_END_SENTINEL_ID).is_none() {
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
        let app = move || {
            let world_data = world_data.clone();
            let region = region.clone();
            let current_user = current_user.clone();
            provide_context(GuessedRegion(region));
            provide_context(LoadedGameDataLocale(get_i18n_lang()));
            provide_context(world_data);
            if let Some(current_user) = current_user {
                provide_context(BootstrapUser(current_user));
            }
            view! { <App /> }
        };
        if offline_guest {
            if let Some(status) = document().get_element_by_id("offline-boot-status") {
                status.remove();
            }
            leptos::mount::mount_to_body(app);
        } else {
            hydrate_body(app);
        }
        dispatch_boot_event("ultros:hydrated");
        let lang = get_i18n_lang();
        prepare_guest_offline(&xiv_gen_db::startup_url(&lang), &lang);
    });
}
