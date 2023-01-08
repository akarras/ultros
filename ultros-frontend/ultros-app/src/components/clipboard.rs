use super::tooltip::*;
use leptos::*;
use std::rc::Rc;
#[cfg(web_sys_unstable_apis)]
use web_sys::{Clipboard, Navigator, Window};

#[component]
pub fn Clipboard(cx: Scope, clipboard_text: String) -> impl IntoView {
    let (tooltip, set_tooltip) = create_signal(cx, format!("Copy {clipboard_text} to clipboard"));
    view! {cx, <div class="clipboard" on:click=move |_| {
        #[cfg(web_sys_unstable_apis)]
        {
            if let Some(window) = web_sys::window()
            {
                let navigator = window.navigator();
                if let Some(clipboard) = navigator.clipboard() {
                    clipboard.write_text(&clipboard_text);
                    set_tooltip("Text copied!".to_string());
                }
            }
        }
        #[cfg(not(web_sys_unstable_apis))]
        {
            set_tooltip("Clipboard API unavailable".to_string())
        }
    }>
        <Tooltip tooltip_text=tooltip >
            <span class="fa-regular fa-clipboard clipboard"></span>
        </Tooltip>
    </div>
    }
}
