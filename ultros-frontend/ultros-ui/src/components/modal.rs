use crate::components::icon::Icon;
use crate::i18n::{t_string, use_i18n};
use icondata as i;
use leptos::{portal::Portal, prelude::*, reactive::wrappers::write::SignalSetter};

#[cfg(feature = "hydrate")]
fn modal_keyboard(panel: NodeRef<leptos::html::Div>, set_visible: SignalSetter<bool>) {
    use wasm_bindgen::JsCast;

    fn topmost() -> Option<web_sys::Element> {
        let panels = document().query_selector_all("[data-ultros-modal]").ok()?;
        panels
            .item(panels.length().checked_sub(1)?)?
            .dyn_into()
            .ok()
    }

    fn focusable(panel: &web_sys::Element) -> Vec<web_sys::HtmlElement> {
        let Ok(nodes) = panel.query_selector_all(
            "a[href],button,input,select,textarea,[tabindex],[contenteditable=true]",
        ) else {
            return Vec::new();
        };
        (0..nodes.length())
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
            .collect()
    }

    // Keep the existing portal layout: custom Select menus also use body
    // portals, which a native dialog's inert subtree would make unusable.
    let opener = StoredValue::new_local(document().active_element());
    let mounted = StoredValue::new_local(None::<web_sys::HtmlDivElement>);
    panel.on_load(move |element| {
        mounted.set_value(Some(element.clone()));
        if !topmost().is_some_and(|top| top.is_same_node(Some(&element))) {
            return;
        }
        if let Some(first) = focusable(&element).first() {
            let _ = first.focus();
        } else {
            let _ = element.focus();
        }
    });

    let _ = leptos_use::use_event_listener(window(), leptos::ev::keydown, move |event| {
        let Some(Some(element)) = panel.try_get_untracked() else {
            return;
        };
        if event.default_prevented()
            || !topmost().is_some_and(|top| top.is_same_node(Some(&element)))
        {
            return;
        }
        if event.key() == "Escape" {
            event.prevent_default();
            event.stop_immediate_propagation();
            set_visible(false);
        } else if event.key() == "Tab" {
            let controls = focusable(&element);
            let active = document().active_element();
            let index = active.as_ref().and_then(|active| {
                controls
                    .iter()
                    .position(|control| control.is_same_node(Some(active)))
            });
            // Only take over the boundaries; native tab order handles all
            // interior controls, including controls inserted asynchronously.
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
                let _ = element.focus();
            }
        }
    });

    on_cleanup(move || {
        let Some(element) = mounted.get_value() else {
            return;
        };
        let opener = opener.get_value().filter(|opener| opener.is_connected());
        // Closing an underlying modal must not move focus out of a newer one.
        if topmost().is_some_and(|top| {
            !top.is_same_node(Some(&element))
                && !opener
                    .as_ref()
                    .is_some_and(|opener| top.contains(Some(opener)))
        }) {
            return;
        }
        if let Some(opener) = opener
            && let Ok(opener) = opener.dyn_into::<web_sys::HtmlElement>()
        {
            let _ = opener.focus();
        }
    });
}

#[component]
pub fn Modal<T>(
    children: TypedChildrenFn<T>,
    #[prop(into)] set_visible: SignalSetter<bool>,
    #[prop(optional, into)] max_width: Option<String>,
    #[prop(optional, into)] aria_label: Option<Signal<String>>,
) -> impl IntoView
where
    T: Render + RenderHtml + Send + 'static,
{
    let i18n = use_i18n();
    let panel = NodeRef::<leptos::html::Div>::new();
    #[cfg(feature = "hydrate")]
    modal_keyboard(panel, set_visible);
    let children = children.into_inner();
    let max_width = max_width.unwrap_or_else(|| "max-w-2xl w-[95%] sm:w-[500px]".to_string());
    view! {
        <Portal>
            <div
                // `overflow-y-auto` + the panel's `m-auto` (rather than
                // items-center) keep a modal taller than the viewport fully
                // reachable: auto margins center a short panel, and a tall one
                // scrolls with the overlay from its top edge — flex centering
                // would push the top above the viewport with no way to reach
                // it (the list settings drawer's Delete button was unclickable
                // at 900px-tall windows).
                class="fixed inset-0 z-[90] bg-black/60 flex justify-center overflow-y-auto p-3 sm:p-6
                transition-opacity duration-300 ease-in-out
                animate-fade-in"
                on:click=move |_| set_visible(false)
            >
                <div
                    node_ref=panel
                    data-ultros-modal=""
                    tabindex="-1"
                    class=format!("flex flex-col min-w-0 m-auto h-fit {max_width}
                    panel rounded-2xl shadow-xl
                    backdrop-blur-md
                    p-4 sm:p-6 z-50
                    animate-slide-in")
                    role="dialog"
                    aria-modal="true"
                    aria-label=move || aria_label.map(|label| label.get())
                    on:click=move |e| {
                        e.stop_propagation();
                    }
                >
                    <div class="flex justify-end mb-2">
                        <button
                            class="p-2 rounded-lg hover:bg-[color:color-mix(in_srgb,_var(--brand-ring)_20%,_transparent)]
                            text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]
                            transition-colors duration-200
                            focus:outline-none focus:ring-2 focus:ring-[color:var(--brand-ring)]"
                            on:click=move |_| set_visible(false)
                            aria-label=t_string!(i18n, modal_aria_close)
                        >
                            <Icon icon=i::CgClose width="1.5em" height="1.5em" />
                        </button>
                    </div>

                    <div class="relative">{children().into_view()}</div>
                </div>
            </div>
        </Portal>
    }
    .into_any()
}
