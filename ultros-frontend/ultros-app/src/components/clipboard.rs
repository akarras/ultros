use super::tooltip::*;
use leptos::*;
use leptos_icons::*;

#[component]
pub fn Clipboard(clipboard_text: String) -> impl IntoView {
    let (copied, set_copied) = create_signal(false);
    let icon = create_memo(move |_| {
        if !copied() {
            Icon::from(BsIcon::BsClipboard2Fill)
        } else {
            Icon::from(BsIcon::BsClipboard2CheckFill)
        }
    });
    let clipboard_text_2 = clipboard_text.clone();
    view! {<div class="clipboard" on:click=move |_| {
        #[cfg(all(web_sys_unstable_apis, feature = "hydrate"))]
        {
            if let Some(window) = web_sys::window()
            {
                let navigator = window.navigator();
                if let Some(clipboard) = navigator.clipboard() {
                    let _ = clipboard.write_text(&clipboard_text);
                    set_copied(true);
                }
            }
        }
        #[cfg(any(not(web_sys_unstable_apis), not(feature = "hydrate")))]
        {
            set_copied(false);
        }
    }>
    <Tooltip tooltip_text=MaybeSignal::derive(move || {
        if !copied() {
            Oco::Owned(format!("Copy {clipboard_text_2} to clipboard").into())
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
