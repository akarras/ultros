use crate::global_state::clipboard_text::GlobalLastCopiedText;

use super::tooltip::*;
use leptos::*;
use leptos_icons::*;

#[component]
pub fn Clipboard(#[prop(into)] clipboard_text: MaybeSignal<String>) -> impl IntoView {
    let last_copied_text = use_context::<GlobalLastCopiedText>().unwrap();
    let clipboard_text = create_memo(move |_| clipboard_text());
    let copied = create_memo(move |_| {
        last_copied_text.0()
            .map(|t| clipboard_text() == t)
            .unwrap_or_default()
    });
    let icon = create_memo(move |_| {
        if !copied() {
            Icon::from(BsIcon::BsClipboard2Fill)
        } else {
            Icon::from(BsIcon::BsClipboard2CheckFill)
        }
    });
    view! {<div class="clipboard" on:click=move |_| {
        #[cfg(all(web_sys_unstable_apis, feature = "hydrate"))]
        {
            if let Some(window) = web_sys::window()
            {
                let navigator = window.navigator();
                if let Some(clipboard) = navigator.clipboard() {
                    let text = clipboard_text.get_untracked();
                    let _ = clipboard.write_text(&text);
                    last_copied_text.0.set(Some(text));
                }
            }
        }
    }>
    <Tooltip tooltip_text=MaybeSignal::derive(move || {
        if !copied() {
            Oco::Owned(format!("Copy {} to clipboard", clipboard_text()))
        }
        else {
            Oco::from("Text copied!")
        }
    }) >
        {move || {let icon = icon(); view!{<Icon icon/>}}}
    </Tooltip>
    </div>
    }
}
