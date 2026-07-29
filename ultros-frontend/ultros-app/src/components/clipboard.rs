use crate::global_state::{clipboard_text::GlobalLastCopiedText, toasts::use_toast};

use super::tooltip::*;
use crate::components::icon::Icon;
use icondata as i;
use leptos::prelude::*;

#[component]
pub fn Clipboard(#[prop(into)] clipboard_text: Signal<String>) -> impl IntoView {
    let last_copied_text = use_context::<GlobalLastCopiedText>().unwrap();
    let toasts = use_toast();
    // ⚡ Bolt Optimization: Removed `Memo::new` for cheap operations
    // `clipboard_text` is just a signal get, `copied` is simple string equality,
    // and `icon` is a cheap branch. Creating reactive `Memo` nodes for these
    // O(1) derivations carries overhead that exceeds the cost of recomputing them.
    let get_clipboard_text = move || clipboard_text();
    let copied = move || {
        last_copied_text.0()
            .map(|t| get_clipboard_text() == t)
            .unwrap_or_default()
    };
    let icon = Signal::derive(move || {
        if !copied() {
            i::BsClipboard2Fill
        } else {
            i::BsClipboard2CheckFill
        }
    });

    let tooltip_text = Signal::derive(move || {
        if !copied() {
            format!("Copy '{}' to clipboard", get_clipboard_text())
        } else {
            "Text copied!".to_string()
        }
    });

    view! {
        <button
            type="button"
            class="clipboard cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--brand-ring)] rounded"
            aria-label=move || {
                if !copied() {
                    format!("Copy {} to clipboard", get_clipboard_text())
                } else {
                    format!("Copied {} to clipboard", get_clipboard_text())
                }
            }
            on:click=move |e| {
                e.prevent_default();
                #[cfg(all(feature = "hydrate"))]
                {
                    use leptos::task::spawn_local;
                    use wasm_bindgen_futures::JsFuture;
                    if let Some(window) = web_sys::window() {
                        let navigator = window.navigator();
                        let clipboard = navigator.clipboard();
                        let text = clipboard_text.get_untracked();
                        // `write_text` returns a Promise that REJECTS when the browser
                        // blocks the write (e.g. Firefox revokes transient user
                        // activation when an ad iframe holds focus, or the document is
                        // not focused). Dropping that Promise leaks the rejection as an
                        // unhandled promise rejection, which our error reporter flags as
                        // an error (GlitchTip #5767). Await it so the rejection is
                        // consumed — a blocked copy is best-effort and unrecoverable.
                        let promise = clipboard.write_text(&text);
                        spawn_local(async move {
                            if JsFuture::from(promise).await.is_err() {
                                leptos::logging::warn!(
                                    "clipboard write_text was blocked by the browser"
                                );
                            }
                        });
                        last_copied_text.0.set(Some(text));
                        if let Some(toasts) = toasts {
                            toasts.success("Copied to clipboard!");
                        }
                    }
                }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = toasts;
    }
            }
        >
            <Tooltip tooltip_text=tooltip_text>
                <Icon icon aria_hidden=true />
            </Tooltip>
        </button>
    }
    .into_any()
}
