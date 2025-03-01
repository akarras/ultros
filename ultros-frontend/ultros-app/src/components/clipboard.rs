use crate::global_state::clipboard_text::GlobalLastCopiedText;

use super::tooltip::*;
use icondata as i;
use leptos::prelude::*;
use leptos_icons::*;

#[component]
pub fn Clipboard(#[prop(into)] clipboard_text: Signal<String>) -> impl IntoView {
    let last_copied_text = use_context::<GlobalLastCopiedText>().unwrap();
    let clipboard_text = Memo::new(move |_| clipboard_text());
    let copied = Memo::new(move |_| {
        last_copied_text.0()
            .map(|t| clipboard_text() == t)
            .unwrap_or_default()
    });
    let icon = Memo::new(move |_| {
        if !copied() {
            i::BsClipboard2Fill
        } else {
            i::BsClipboard2CheckFill
        }
    });
    view! {
        <div
            class="clipboard cursor-pointer"
            on:click=move |_| {
                #[cfg(all(feature = "hydrate"))]
                {
                    if let Some(window) = web_sys::window() {
                        let navigator = window.navigator();
                        let clipboard = navigator.clipboard();
                        let text = clipboard_text.get_untracked();
                        let _ = clipboard.write_text(&text);
                        last_copied_text.0.set(Some(text));
                    }
                }
            }
        >

            <Tooltip tooltip_text=Signal::derive(move || {
                if !copied() {
                    format!("Copy '{}' to clipboard", clipboard_text())
                } else {
                    "Text copied!".to_string()
                }
            })>
                <Icon icon/>
            </Tooltip>
        </div>
    }
    .into_any()
}
