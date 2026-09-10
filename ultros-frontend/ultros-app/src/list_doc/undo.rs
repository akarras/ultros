//! Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y, Cmd on Apple (spec section 3.3). One
//! window listener per open page, ignored while an editable element has
//! focus or one of the page's modals is open.

use leptos::prelude::*;

use crate::list_doc::handle::ListDocHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoKey {
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct KeyContext {
    pub ctrl: bool,
    pub meta: bool,
    pub shift: bool,
    pub apple: bool,
    pub editable_target: bool,
    pub modal_open: bool,
}

pub fn classify_key(key: &str, ctx: KeyContext) -> Option<UndoKey> {
    if ctx.editable_target || ctx.modal_open {
        return None;
    }
    let modifier = if ctx.apple { ctx.meta } else { ctx.ctrl };
    if !modifier {
        return None;
    }
    match key.to_ascii_lowercase().as_str() {
        "z" if ctx.shift => Some(UndoKey::Redo),
        "z" => Some(UndoKey::Undo),
        "y" if !ctx.apple => Some(UndoKey::Redo),
        _ => None,
    }
}

/// Register the window listener. Hydrate only: the server never sees keys.
pub fn install(handle: ListDocHandle, modal_open: Signal<bool>) {
    #[cfg(feature = "hydrate")]
    {
        use leptos_use::{UseEventListenerOptions, use_event_listener_with_options, use_window};
        use wasm_bindgen::JsCast;

        let apple = crate::global_state::platform::use_platform_hotkeys().apple;
        let _ = use_event_listener_with_options(
            use_window(),
            leptos::ev::keydown,
            move |ev: web_sys::KeyboardEvent| {
                let editable_target = ev
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .map(|el| {
                        let tag = el.tag_name().to_ascii_uppercase();
                        tag == "INPUT"
                            || tag == "TEXTAREA"
                            || tag == "SELECT"
                            || el
                                .dyn_ref::<web_sys::HtmlElement>()
                                .is_some_and(|h| h.is_content_editable())
                    })
                    .unwrap_or(false);
                let ctx = KeyContext {
                    ctrl: ev.ctrl_key(),
                    meta: ev.meta_key(),
                    shift: ev.shift_key(),
                    apple: apple.get_untracked(),
                    editable_target,
                    modal_open: modal_open.get_untracked(),
                };
                match classify_key(&ev.key(), ctx) {
                    Some(UndoKey::Undo) => {
                        ev.prevent_default();
                        handle.undo();
                    }
                    Some(UndoKey::Redo) => {
                        ev.prevent_default();
                        handle.redo();
                    }
                    None => {}
                }
            },
            UseEventListenerOptions::default()
                .capture(false)
                .passive(false),
        );
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = (handle, modal_open);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl() -> KeyContext {
        KeyContext {
            ctrl: true,
            ..Default::default()
        }
    }

    #[test]
    fn windows_bindings() {
        assert_eq!(classify_key("z", ctrl()), Some(UndoKey::Undo));
        assert_eq!(
            classify_key(
                "Z",
                KeyContext {
                    shift: true,
                    ..ctrl()
                }
            ),
            Some(UndoKey::Redo)
        );
        assert_eq!(classify_key("y", ctrl()), Some(UndoKey::Redo));
        assert_eq!(classify_key("z", KeyContext::default()), None);
        assert_eq!(
            classify_key(
                "z",
                KeyContext {
                    meta: true,
                    ..Default::default()
                }
            ),
            None
        );
        assert_eq!(classify_key("a", ctrl()), None);
    }

    #[test]
    fn apple_uses_cmd_and_has_no_cmd_y() {
        let cmd = KeyContext {
            meta: true,
            apple: true,
            ..Default::default()
        };
        assert_eq!(classify_key("z", cmd), Some(UndoKey::Undo));
        assert_eq!(
            classify_key("z", KeyContext { shift: true, ..cmd }),
            Some(UndoKey::Redo)
        );
        assert_eq!(classify_key("y", cmd), None);
        assert_eq!(
            classify_key(
                "z",
                KeyContext {
                    ctrl: true,
                    apple: true,
                    ..Default::default()
                }
            ),
            None
        );
    }

    #[test]
    fn editable_targets_and_modals_swallow_the_keys() {
        assert_eq!(
            classify_key(
                "z",
                KeyContext {
                    editable_target: true,
                    ..ctrl()
                }
            ),
            None
        );
        assert_eq!(
            classify_key(
                "z",
                KeyContext {
                    modal_open: true,
                    ..ctrl()
                }
            ),
            None
        );
    }
}
