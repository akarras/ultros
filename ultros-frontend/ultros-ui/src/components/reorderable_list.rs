use crate::i18n::{t, t_string, use_i18n};
use leptos::prelude::*;
use std::hash::Hash;
use web_sys::wasm_bindgen::JsCast;

/// Stable keys preserve row state and keyboard focus when an item moves.
#[component]
pub fn ReorderableList<T, V, N, K>(
    items: RwSignal<Vec<T>>,
    item_view: V,
    key_fn: impl Fn(&T) -> K + Copy + Send + Sync + 'static,
    item_label: impl Fn(&T) -> String + Copy + Send + Sync + 'static,
) -> impl IntoView
where
    T: 'static + Clone + Send + Sync,
    K: 'static + Clone + Eq + Hash + Send + Sync,
    V: Fn(T) -> N + 'static + Copy + Send + Sync,
    N: IntoView + 'static,
{
    let i18n = use_i18n();
    let dragging = RwSignal::new(None::<usize>);
    let over = RwSignal::new(None::<usize>);
    let announcement = RwSignal::new(String::new());
    let move_item = move |from: usize, to: usize| {
        #[cfg(feature = "hydrate")]
        let active = document().active_element();
        items.update(|rows| {
            if from == to || from >= rows.len() || to >= rows.len() {
                return;
            }
            let row = rows.remove(from);
            let name = item_label(&row);
            rows.insert(to, row);
            announcement.set(format!(
                "{}: {} ({}/{})",
                name,
                t_string!(i18n, a11y_order_updated),
                to + 1,
                rows.len()
            ));
        });
        // Moving an existing keyed DOM subtree can still blur its focused
        // button. Restore it after the move, unless focus already went to
        // another control in the meantime.
        #[cfg(feature = "hydrate")]
        request_animation_frame(move || {
            if document()
                .active_element()
                .is_none_or(|el| el.tag_name() == "BODY")
                && let Some(active) = active
                    .filter(|el| el.is_connected())
                    .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
            {
                let _ = active.focus();
            }
        });
    };
    view! {
        <div class="reorderable-list flex flex-col gap-2"
            on:pointermove=move |e| {
                if dragging.get_untracked().is_some() {
                    let index = document().element_from_point(e.client_x() as f32, e.client_y() as f32)
                        .and_then(|el| el.closest("[data-reorder-index]").ok().flatten())
                        .and_then(|el| el.get_attribute("data-reorder-index"))
                        .and_then(|index| index.parse::<usize>().ok());
                    over.set(index);
                }
            }
            on:pointerup=move |_| {
                if let (Some(from), Some(to)) = (dragging.get_untracked(), over.get_untracked()) { move_item(from, to); }
                dragging.set(None); over.set(None);
            }
            on:pointercancel=move |_| { dragging.set(None); over.set(None); }
        >
            <p class="sr-only" role="status" aria-atomic="true">{move || announcement.get()}</p>
            <For each=move || items.get() key=key_fn children=move |child| {
                let key = key_fn(&child);
                let index = Memo::new(move |_| items.with(|rows| rows.iter().position(|row| key_fn(row) == key).unwrap_or(0)));
                let name = StoredValue::new(item_label(&child));
                view! {
                    <div data-reorder-index=move || index.get()
                        class:drop-hint=move || over.get() == Some(index.get()) && dragging.get().is_some()
                    >
                        <div class="reorder-actions">
                            <button type="button" class="btn-secondary" aria-label=move || format!("{}: {}", name.get_value(), t_string!(i18n, a11y_move_up))
                                aria-disabled=move || (index.get() == 0).to_string()
                                on:click=move |_| { let from = index.get_untracked(); move_item(from, from.saturating_sub(1)); }
                            >{t!(i18n, a11y_move_up)}</button>
                            <button type="button" class="btn-secondary" aria-label=move || format!("{}: {}", name.get_value(), t_string!(i18n, a11y_move_down))
                                aria-disabled=move || (index.get() + 1 >= items.with(Vec::len)).to_string()
                                on:click=move |_| { let from = index.get_untracked(); move_item(from, from + 1); }
                            >{t!(i18n, a11y_move_down)}</button>
                            <span class="px-3 py-2 cursor-grab" aria-hidden="true" style="touch-action: none; user-select: none;"
                                on:pointerdown=move |e| {
                                    if e.button() == 0 {
                                        dragging.set(Some(index.get_untracked())); over.set(Some(index.get_untracked()));
                                        if let Some(target) = e.target().and_then(|el| el.dyn_into::<web_sys::Element>().ok()) { let _ = target.set_pointer_capture(e.pointer_id()); }
                                    }
                                }
                            >"⠿"</span>
                        </div>
                        {item_view(child)}
                    </div>
                }
            }/>
        </div>
    }
}
