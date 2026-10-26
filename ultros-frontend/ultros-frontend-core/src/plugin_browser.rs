//! Browser-only consent transport. Do not instrument request fields or responses.
use leptos::prelude::*;
use ultros_api_types::plugin::*;

#[derive(Clone, Copy)]
struct PluginAvailability(Resource<Option<PluginWebsiteInfo>>);

/// Share one availability request across discovery links, only after Labs opt-in.
pub fn provide_availability() {
    let (labs, _) = crate::global_state::labs::use_dalamud_labs();
    provide_context(PluginAvailability(Resource::new_blocking(
        move || labs.get(),
        |enabled| async move { if enabled { info().await.ok() } else { None } },
    )));
}

pub fn use_setup_available() -> Signal<bool> {
    let PluginAvailability(info) = expect_context::<PluginAvailability>();
    let (labs, _) = crate::global_state::labs::use_dalamud_labs();
    Signal::derive(move || labs.get() && info.get().flatten().as_ref().is_some_and(setup_available))
}

pub fn setup_available(config: &PluginWebsiteInfo) -> bool {
    config.enabled
        && (config.localhost
            || [
                &config.latest_version,
                &config.repository_url,
                &config.download_url,
            ]
            .iter()
            .all(|value| value.as_ref().is_some_and(|value| !value.is_empty())))
}

pub async fn info() -> crate::error::AppResult<PluginWebsiteInfo> {
    crate::api::fetch_api("/api/v1/plugin/browser/info").await
}

pub fn document_ready() -> bool {
    #[cfg(feature = "ssr")]
    {
        true
    }
    #[cfg(not(feature = "ssr"))]
    {
        leptos::prelude::document()
            .get_element_by_id("ultros-plugin-document")
            .is_some()
    }
}

#[cfg(not(feature = "ssr"))]
async fn request<T: serde::de::DeserializeOwned + Send + 'static>(
    path: &'static str,
    fields: Option<Vec<(String, String)>>,
) -> Result<T, String> {
    if !document_ready() {
        return Err("Open the Plugin page again to start a connection.".into());
    }
    let (sender, receiver) = flume::bounded(1);
    leptos::task::spawn_local(async move {
        let result = async {
            let request=if let Some(fields)=fields {
                let body=fields.iter().map(|(k,v)| format!("{}={}",percent_encoding::utf8_percent_encode(k,percent_encoding::NON_ALPHANUMERIC),percent_encoding::utf8_percent_encode(v,percent_encoding::NON_ALPHANUMERIC))).collect::<Vec<_>>().join("&");
                gloo_net::http::Request::post(path).credentials(web_sys::RequestCredentials::SameOrigin)
                    .header("Content-Type","application/x-www-form-urlencoded").body(body).map_err(|_| "Could not prepare the request.".to_string())?
            } else { gloo_net::http::Request::get(path).credentials(web_sys::RequestCredentials::SameOrigin).build().map_err(|_| "Could not prepare the request.".to_string())? };
            let response=request.send().await.map_err(|_| "Could not reach Ultros. Check your connection and try again.".to_string())?;
            let status=response.status();
            let body=response.text().await.map_err(|_| "Could not read the response. Start again to retry.".to_string())?;
            if !(200..300).contains(&status) {
                let code=serde_json::from_str::<serde_json::Value>(&body).ok().and_then(|v| v.get("error").and_then(|v| v.as_str()).map(str::to_owned));
                return Err(match (status,code.as_deref()) {
                    (401,_)=>"Your sign-in has expired. Sign in again, then start a new connection.",
                    (429,_)=>"Too many attempts. Wait a few minutes before trying again.",
                    (_,Some("invalid_or_expired_code"))=>"That code is incorrect or has expired. Check the code in your game, or start a new connection there.",
                    (_,Some("invalid_or_expired_link"))=>"This connection link is invalid or has expired. Start a new connection in game, or enter its short code.",
                    (403,_)| (400,Some("expired_token"))=>"This connection request has expired or your account changed. Start again and compare the code in your game.",
                    (_,Some("invalid_approved_scopes"))=>"Choose at least one permission. Editing lists needs list reading; saving or uploading retainers needs retainer reading.",
                    _=>"The request could not be completed. Start again to retry.",
                }.into());
            }
            serde_json::from_str(&body).map_err(|_| "Could not read the response. Start again to retry.".to_string())
        }.await;
        let _ = sender.send(result);
    });
    receiver
        .recv_async()
        .await
        .map_err(|_| "The request was interrupted. Start again to retry.".to_string())?
}

#[cfg(feature = "ssr")]
async fn request<T>(
    _path: &'static str,
    _fields: Option<Vec<(String, String)>>,
) -> Result<T, String> {
    Err("Connection controls become available when the page has loaded in your browser.".into())
}

/// The head script strips the URL fragment and posts it before app startup.
/// Waiting here also ensures its HttpOnly cookie survives a sign-in redirect.
pub async fn prepare_link() -> Result<(), String> {
    #[cfg(not(feature = "ssr"))]
    {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::JsValue;
        if let Ok(capture) = js_sys::Reflect::get(
            leptos::prelude::window().as_ref(),
            &JsValue::from_str("__ULTROS_CAPTURE_PLUGIN_LINK__"),
        ) && let Some(capture) = capture.dyn_ref::<js_sys::Function>()
        {
            capture.call0(&JsValue::NULL).map_err(|_| {
                "Could not prepare this connection link. Open it again.".to_string()
            })?;
        }
        let pending = js_sys::Reflect::get(
            leptos::prelude::window().as_ref(),
            &JsValue::from_str("__ULTROS_PLUGIN_LINK_CAPTURE__"),
        )
        .map_err(|_| "Could not prepare this connection link. Open it again.".to_string())?;
        if !pending.is_undefined() {
            let result = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&pending))
                .await
                .map_err(|_| {
                    "Could not prepare this connection link. Open it again.".to_string()
                })?;
            if let Some(error) = result.as_string() {
                return Err(error);
            }
        }
    }
    Ok(())
}

pub async fn approval() -> Result<BrowserApproval, String> {
    prepare_link().await?;
    request("/api/v1/plugin/browser/approval", None).await
}
pub async fn devices() -> Result<BrowserDevices, String> {
    request("/api/v1/plugin/browser/devices", None).await
}
pub async fn select(csrf: String, code: String) -> Result<BrowserSelection, String> {
    request(
        "/api/v1/plugin/pairings/select",
        Some(vec![("csrf".into(), csrf), ("user_code".into(), code)]),
    )
    .await
}
pub async fn select_link(csrf: String) -> Result<BrowserSelection, String> {
    request(
        "/api/v1/plugin/browser/link/select",
        Some(vec![("csrf".into(), csrf)]),
    )
    .await
}
pub async fn approve(
    csrf: String,
    nonce: String,
    scopes: Vec<String>,
    deny: bool,
) -> Result<BrowserDecision, String> {
    let mut fields = vec![
        ("csrf".into(), csrf),
        ("nonce".into(), nonce),
        (
            "decision".into(),
            if deny { "deny" } else { "approve" }.into(),
        ),
    ];
    fields.extend(scopes.into_iter().map(|scope| ("scope".into(), scope)));
    request("/api/v1/plugin/pairings/approve", Some(fields)).await
}
pub async fn revoke(csrf: String, device: String) -> Result<bool, String> {
    request(
        "/api/v1/plugin/browser/devices/revoke",
        Some(vec![("csrf".into(), csrf), ("device".into(), device)]),
    )
    .await
}
