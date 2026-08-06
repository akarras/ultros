//! Shared plumbing for shareable invite links.
//!
//! Lists and groups both mint `/{thing}/invite/{code}` URLs, and both have to
//! copy them to the clipboard without leaking an unhandled promise rejection.
//! That handling lives here once rather than being re-derived per feature.

use crate::global_state::clipboard_text::GlobalLastCopiedText;
use crate::global_state::toasts::Toasts;
use leptos::prelude::Set;

/// Absolute URL for an invite when the origin is known, relative otherwise.
///
/// `base_path` is the route prefix without a trailing slash, e.g. `/list/invite`.
/// SSR has no `window`, so it renders the relative form and the browser resolves
/// it against the current origin.
pub(crate) fn invite_url(base_path: &str, invite_id: &str) -> String {
    #[cfg(feature = "hydrate")]
    {
        if let Some(window) = web_sys::window()
            && let Ok(origin) = window.location().origin()
        {
            return format!("{origin}{base_path}/{invite_id}");
        }
    }
    format!("{base_path}/{invite_id}")
}

/// Copy an invite link, echoing it into the global "last copied" slot and
/// toasting `toast_message` (already localized by the caller).
pub(crate) fn copy_invite_url(
    base_path: &str,
    invite_id: &str,
    last_copied: Option<GlobalLastCopiedText>,
    toasts: Option<Toasts>,
    toast_message: String,
) {
    let url = invite_url(base_path, invite_id);
    #[cfg(feature = "hydrate")]
    if let Some(window) = web_sys::window() {
        use leptos::task::spawn_local;
        use wasm_bindgen_futures::JsFuture;
        let clipboard = window.navigator().clipboard();
        // `write_text` returns a Promise that rejects when the browser blocks the
        // write. Dropping it leaks an unhandled promise rejection that our error
        // reporter flags as an error (see GlitchTip #5767). Await it so a blocked
        // best-effort copy is consumed instead of reported.
        let promise = clipboard.write_text(&url);
        spawn_local(async move {
            if JsFuture::from(promise).await.is_err() {
                leptos::logging::warn!("clipboard write_text was blocked by the browser");
            }
        });
    }
    if let Some(last_copied) = last_copied {
        last_copied.0.set(Some(url));
    }
    if let Some(toasts) = toasts {
        toasts.success(toast_message);
    }
}

/// `"3/10 uses"`, or `"3/∞ uses"` when the invite has no cap.
pub(crate) fn uses_label(uses: i32, max_uses: Option<i32>) -> String {
    format!(
        "{}/{} uses",
        uses,
        max_uses
            .map(|max_uses| max_uses.to_string())
            .unwrap_or_else(|| "∞".to_string())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_url_falls_back_to_a_relative_path_without_a_window() {
        // The hydrate feature is off under `cargo test`, so this exercises the
        // SSR branch.
        assert_eq!(
            invite_url("/list/invite", "test-id"),
            "/list/invite/test-id"
        );
        assert_eq!(
            invite_url("/group/invite", "test-id"),
            "/group/invite/test-id"
        );
    }

    #[test]
    fn uses_label_renders_an_infinity_cap_when_unlimited() {
        assert_eq!(uses_label(5, None), "5/∞ uses");
        assert_eq!(uses_label(3, Some(10)), "3/10 uses");
        assert_eq!(uses_label(0, Some(1)), "0/1 uses");
    }
}
