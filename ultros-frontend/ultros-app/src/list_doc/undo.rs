//! Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y, Cmd on Apple (spec section 3.3). One
//! window listener per open page, ignored while an editable element has
//! focus or one of the page's modals is open.
//!
//! The listener is document-agnostic: account lists (`ListDocHandle`) and
//! device lists (`GuestListHandle`) both install it through
//! [`UndoBindings`], so the platform modifiers, the editable-target and
//! modal guards and the listener lifecycle are defined exactly once
//! (issue #1429).

use leptos::prelude::*;

/// What the keyboard shortcuts drive. `undo`/`redo` run on the document the
/// installing page currently has open; `modal_open` suppresses the shortcuts
/// while a modal or confirmation panel owns the keyboard.
#[derive(Clone, Copy)]
pub struct UndoBindings {
    pub undo: Callback<()>,
    pub redo: Callback<()>,
    pub modal_open: Signal<bool>,
}

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

/// Who owns Ctrl+Z when the keydown target is a form control (#1430).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Not a text editor, or a list editor whose live value equals the value
    /// the document holds: the shortcut is a document undo/redo.
    Document,
    /// An editor holding uncommitted text: the browser's own text undo keeps
    /// the keys and the document is untouched.
    Draft,
}

/// The draft-versus-committed rule. A list editor opts in by rendering
/// `data-committed` with the document's current value for that field; while
/// its live value matches, nothing is drafted and Ctrl+Z reaches the
/// document. Inputs without the attribute (free text elsewhere on the page),
/// textareas and contenteditable regions always keep native undo. Selects
/// and non-text inputs never hold a draft.
pub fn classify_target(
    tag: &str,
    input_type: Option<&str>,
    content_editable: bool,
    value: Option<&str>,
    committed: Option<&str>,
) -> Target {
    match tag.to_ascii_uppercase().as_str() {
        "SELECT" => Target::Document,
        "TEXTAREA" => Target::Draft,
        "INPUT" => match input_type.map(|kind| kind.to_ascii_lowercase()).as_deref() {
            Some(
                "checkbox" | "radio" | "button" | "submit" | "reset" | "range" | "color" | "file"
                | "image",
            ) => Target::Document,
            _ => match committed {
                Some(committed) if value.unwrap_or_default().trim() == committed.trim() => {
                    Target::Document
                }
                _ => Target::Draft,
            },
        },
        _ if content_editable => Target::Draft,
        _ => Target::Document,
    }
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

/// Register the window listener under the current reactive owner. Hydrate
/// only: the server never sees keys. The listener is removed when that owner
/// is disposed, so a page that re-installs per opened document (or a device
/// editor that is re-created per list) never accumulates listeners.
pub fn install(bindings: UndoBindings) {
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
                        let value = el
                            .dyn_ref::<web_sys::HtmlInputElement>()
                            .map(|input| input.value());
                        classify_target(
                            &el.tag_name(),
                            el.get_attribute("type").as_deref(),
                            el.dyn_ref::<web_sys::HtmlElement>()
                                .is_some_and(|h| h.is_content_editable()),
                            value.as_deref(),
                            el.get_attribute("data-committed").as_deref(),
                        ) == Target::Draft
                    })
                    .unwrap_or(false);
                let ctx = KeyContext {
                    ctrl: ev.ctrl_key(),
                    meta: ev.meta_key(),
                    shift: ev.shift_key(),
                    apple: apple.get_untracked(),
                    editable_target,
                    modal_open: bindings.modal_open.get_untracked(),
                };
                match classify_key(&ev.key(), ctx) {
                    Some(UndoKey::Undo) => {
                        ev.prevent_default();
                        bindings.undo.run(());
                    }
                    Some(UndoKey::Redo) => {
                        ev.prevent_default();
                        bindings.redo.run(());
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
        let _ = bindings;
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
    fn list_editors_hand_the_keys_over_once_clean() {
        let input = |kind: Option<&str>, value: &str, committed: Option<&str>| {
            classify_target("INPUT", kind, false, Some(value), committed)
        };
        // A cell whose live value is what the document holds.
        assert_eq!(input(Some("number"), "8", Some("8")), Target::Document);
        assert_eq!(input(Some("number"), " 8 ", Some("8")), Target::Document);
        // The same cell mid-edit.
        assert_eq!(input(Some("number"), "44", Some("8")), Target::Draft);
        // Catalog search: empty is clean, typed text is a draft.
        assert_eq!(input(Some("text"), "", Some("")), Target::Document);
        assert_eq!(input(Some("text"), "Bronze", Some("")), Target::Draft);
        assert_eq!(input(None, "", Some("")), Target::Document);
        // Inputs that never opted in keep native undo.
        assert_eq!(input(Some("text"), "", None), Target::Draft);
        assert_eq!(input(Some("text"), "name", None), Target::Draft);
        // Non-text controls never hold a draft.
        assert_eq!(input(Some("checkbox"), "on", None), Target::Document);
        assert_eq!(input(Some("Radio"), "on", None), Target::Document);
        assert_eq!(
            classify_target("select", None, false, Some("hq"), None),
            Target::Document
        );
        assert_eq!(
            classify_target("TEXTAREA", None, false, Some(""), Some("")),
            Target::Draft
        );
        assert_eq!(
            classify_target("DIV", None, true, None, None),
            Target::Draft
        );
        assert_eq!(
            classify_target("BUTTON", None, false, None, None),
            Target::Document
        );
        assert_eq!(
            classify_target("TD", None, false, None, None),
            Target::Document
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
