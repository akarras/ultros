//! Browser-side Web Push subscribe flow.
//!
//! Walks through the standard sequence: register the service worker, prompt
//! for notification permission, subscribe through PushManager, then POST the
//! resulting (endpoint, p256dh, auth, user_agent) to the server.
//!
//! Compiled only for the `hydrate` build (wasm). The SSR build has a stub
//! that returns an error string so the parent component doesn't need to
//! cfg-gate every call site.

use ultros_api_types::alert::Endpoint;

/// User-facing result of a push subscription attempt. Strings are intended to
/// be shown directly in a toast.
pub(crate) type SubscribeResult = Result<Endpoint, String>;

#[cfg(all(feature = "hydrate", target_arch = "wasm32"))]
pub(crate) async fn enable_browser_notifications() -> SubscribeResult {
    use crate::api::{create_push_subscription, get_vapid_public_key};
    use js_sys::{Reflect, Uint8Array};
    use ultros_api_types::alert::CreatePushSubscriptionRequest;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{
        Notification, PushEncryptionKeyName, PushManager, PushSubscription,
        PushSubscriptionOptionsInit, ServiceWorkerRegistration,
    };

    // 1. Capability check. `navigator.serviceWorker` is always present in
    // modern browsers we care about; `window.PushManager` is the right check
    // for browser-level support of the Push API.
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let navigator = window.navigator();
    let sw_container = navigator.service_worker();
    if !Reflect::has(&window, &JsValue::from_str("PushManager")).unwrap_or(false) {
        return Err("Browser doesn't support Web Push".into());
    }

    // 2. Permission. Browsers require this come from a user gesture; we're
    // inside an on:click handler so that's satisfied.
    let perm_promise = Notification::request_permission()
        .map_err(|_| "notification permission request failed".to_string())?;
    let perm_js = JsFuture::from(perm_promise)
        .await
        .map_err(|_| "notification permission request failed".to_string())?;
    if perm_js.as_string().as_deref() != Some("granted") {
        return Err("Notification permission denied".into());
    }

    // 3. Register the service worker (idempotent — repeat calls return the
    // existing registration).
    let reg_js = JsFuture::from(sw_container.register("/service-worker.js"))
        .await
        .map_err(|_| "service worker registration failed".to_string())?;
    let registration: ServiceWorkerRegistration = reg_js
        .dyn_into()
        .map_err(|_| "service worker registration returned wrong type".to_string())?;

    // 4. Fetch the server's VAPID public key and decode base64url → bytes.
    let vapid = get_vapid_public_key()
        .await
        .map_err(|e| format!("could not fetch VAPID key: {e}"))?;
    let key_bytes = base64url_decode(&vapid.key)
        .map_err(|e| format!("malformed VAPID key from server: {e}"))?;
    let app_server_key = Uint8Array::from(key_bytes.as_slice());

    // 5. Subscribe.
    let push: PushManager = registration
        .push_manager()
        .map_err(|_| "push manager unavailable".to_string())?;
    let opts = PushSubscriptionOptionsInit::new();
    opts.set_user_visible_only(true);
    let app_server_key_js: JsValue = app_server_key.into();
    opts.set_application_server_key(&app_server_key_js);
    let sub_promise = push
        .subscribe_with_options(&opts)
        .map_err(|_| "push subscribe call failed".to_string())?;
    let sub_js = JsFuture::from(sub_promise)
        .await
        .map_err(|e| format!("push subscribe rejected: {e:?}"))?;
    let subscription: PushSubscription = sub_js
        .dyn_into()
        .map_err(|_| "push subscribe returned wrong type".to_string())?;

    // 6. Extract endpoint + keys. `getKey` returns an ArrayBuffer; we wrap
    // it in a Uint8Array view and base64url-encode the bytes.
    let endpoint = subscription.endpoint();
    let p256dh = subscription_key_b64url(&subscription, PushEncryptionKeyName::P256dh)
        .ok_or_else(|| "subscription missing p256dh key".to_string())?;
    let auth = subscription_key_b64url(&subscription, PushEncryptionKeyName::Auth)
        .ok_or_else(|| "subscription missing auth key".to_string())?;
    let user_agent = navigator.user_agent().ok();

    // 7. Hand it to the server.
    let req = CreatePushSubscriptionRequest {
        endpoint,
        p256dh,
        auth,
        user_agent,
    };
    create_push_subscription(req)
        .await
        .map_err(|e| format!("server rejected subscription: {e}"))
}

#[cfg(not(all(feature = "hydrate", target_arch = "wasm32")))]
pub(crate) async fn enable_browser_notifications() -> SubscribeResult {
    Err("Web Push is only available in the browser".into())
}

#[cfg(all(feature = "hydrate", target_arch = "wasm32"))]
fn subscription_key_b64url(
    sub: &web_sys::PushSubscription,
    name: web_sys::PushEncryptionKeyName,
) -> Option<String> {
    use js_sys::Uint8Array;
    let key_js = sub.get_key(name).ok()??;
    let arr = Uint8Array::new(&key_js);
    let mut bytes = vec![0u8; arr.length() as usize];
    arr.copy_to(&mut bytes);
    Some(base64url_encode(&bytes))
}

#[cfg(all(feature = "hydrate", target_arch = "wasm32"))]
fn base64url_encode(bytes: &[u8]) -> String {
    // Standard base64 alphabet with - and _ instead of + and /, no padding.
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        out.push(CHARS[((n >> 18) & 0x3f) as usize] as char);
        out.push(CHARS[((n >> 12) & 0x3f) as usize] as char);
        out.push(CHARS[((n >> 6) & 0x3f) as usize] as char);
        out.push(CHARS[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        out.push(CHARS[((n >> 18) & 0x3f) as usize] as char);
        out.push(CHARS[((n >> 12) & 0x3f) as usize] as char);
    } else if rem == 2 {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        out.push(CHARS[((n >> 18) & 0x3f) as usize] as char);
        out.push(CHARS[((n >> 12) & 0x3f) as usize] as char);
        out.push(CHARS[((n >> 6) & 0x3f) as usize] as char);
    }
    out
}

#[cfg(all(feature = "hydrate", target_arch = "wasm32"))]
fn base64url_decode(s: &str) -> Result<Vec<u8>, &'static str> {
    let mut buf = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        let mapped = match c {
            '-' => '+',
            '_' => '/',
            other => other,
        };
        buf.push(mapped);
    }
    while !buf.len().is_multiple_of(4) {
        buf.push('=');
    }
    decode_standard_base64(&buf)
}

#[cfg(all(feature = "hydrate", target_arch = "wasm32"))]
fn decode_standard_base64(s: &str) -> Result<Vec<u8>, &'static str> {
    fn lookup(c: u8) -> Result<u8, &'static str> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err("invalid base64 char"),
        }
    }
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err("base64 length not multiple of 4");
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut i = 0;
    while i < bytes.len() {
        let chunk = &bytes[i..i + 4];
        let b0 = lookup(chunk[0])?;
        let b1 = lookup(chunk[1])?;
        let b2 = if chunk[2] == b'=' {
            0
        } else {
            lookup(chunk[2])?
        };
        let b3 = if chunk[3] == b'=' {
            0
        } else {
            lookup(chunk[3])?
        };
        out.push((b0 << 2) | (b1 >> 4));
        if chunk[2] != b'=' {
            out.push(((b1 & 0xf) << 4) | (b2 >> 2));
        }
        if chunk[3] != b'=' {
            out.push(((b2 & 0x3) << 6) | b3);
        }
        i += 4;
    }
    Ok(out)
}
