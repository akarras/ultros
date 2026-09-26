//! Native modal surface shared by search and task dialogs.
use leptos::{html, prelude::*, reactive::wrappers::write::SignalSetter};

fn contain_tab(event: web_sys::KeyboardEvent, panel: NodeRef<html::Dialog>) {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        if event.key() != "Tab" || event.default_prevented() {
            return;
        }
        let Some(panel) = panel.get_untracked() else {
            return;
        };
        let Ok(nodes) = panel.query_selector_all(
            "a[href],button,input,select,textarea,[tabindex],[contenteditable=true]",
        ) else {
            return;
        };
        let controls = (0..nodes.length())
            .filter_map(|index| nodes.item(index)?.dyn_into::<web_sys::HtmlElement>().ok())
            .filter(|element| {
                element.tab_index() >= 0
                    && !element
                        .matches(":disabled, [inert], [inert] *")
                        .unwrap_or(true)
                    && (element.offset_width() > 0 || element.offset_height() > 0)
                    && window()
                        .get_computed_style(element)
                        .ok()
                        .flatten()
                        .and_then(|style| style.get_property_value("visibility").ok())
                        .is_some_and(|visibility| visibility != "hidden")
            })
            .collect::<Vec<_>>();
        let active = document().active_element();
        let index = active.as_ref().and_then(|active| {
            controls
                .iter()
                .position(|control| control.is_same_node(Some(active)))
        });
        // Native modal inertness protects the background. Wrap the boundaries
        // explicitly too, so Tab does not send focus to the browser chrome.
        let target = match (index, event.shift_key()) {
            (None | Some(0), true) => controls.last(),
            (None, false) => controls.first(),
            (Some(index), false) if index + 1 == controls.len() => controls.first(),
            _ => return,
        };
        event.prevent_default();
        if let Some(target) = target {
            let _ = target.focus();
        } else {
            let _ = panel.focus();
        }
    }
    #[cfg(not(feature = "hydrate"))]
    let _ = (event, panel);
}

#[component]
pub fn DialogSurface(
    children: Children,
    #[prop(into)] set_visible: SignalSetter<bool>,
    #[prop(into)] class: String,
    #[prop(optional_no_strip)] aria_label: Option<Signal<String>>,
) -> impl IntoView {
    let panel = NodeRef::<html::Dialog>::new();
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        let opener = StoredValue::new_local(document().active_element());
        let mounted = StoredValue::new_local(None::<web_sys::HtmlDialogElement>);
        panel.on_load(move |element| {
            // Existing task dialogs supply a visible heading. Use it when the
            // caller has not supplied an explicit localized name.
            if aria_label.is_none()
                && let Ok(Some(heading)) = element.query_selector("h1,h2,h3,[data-dialog-title]")
            {
                let _ = element
                    .set_attribute("aria-label", &heading.text_content().unwrap_or_default());
            }
            let _ = element.show_modal();
            mounted.set_value(Some(element));
        });
        on_cleanup(move || {
            if let Some(element) = mounted.get_value() {
                element.close();
            }
            if let Some(opener) = opener.get_value().filter(|el| el.is_connected())
                && let Ok(opener) = opener.dyn_into::<web_sys::HtmlElement>()
                && opener.closest("[inert]").ok().flatten().is_none()
            {
                let _ = opener.focus();
            }
        });
    }
    view! {
        <dialog role="dialog" node_ref=panel class=class aria-label=move || aria_label.map(|label| label.get())
            aria-modal="true" data-ultros-modal=""
            on:keydown=move |event| contain_tab(event, panel)
            on:cancel=move |event: web_sys::Event| { event.prevent_default(); set_visible(false); }
        >{children()}</dialog>
    }
}
