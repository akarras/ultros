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
                    if let Some(window) = web_sys::window() {
                        let navigator = window.navigator();
                        let clipboard = navigator.clipboard();
                        let text = clipboard_text.get_untracked();
                        let _ = clipboard.write_text(&text);
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
