use leptos::prelude::*;
use web_sys::PointerEvent;
use web_sys::wasm_bindgen::JsCast;

#[component]
pub fn ReorderableList<T, V, N>(items: RwSignal<Vec<T>>, item_view: V) -> impl IntoView
where
    T: 'static + Clone + Send + Sync,
    V: Fn(T) -> N + 'static + Copy + Send + Sync,
    N: IntoView + 'static,
{
    let (dragging, set_dragging) = signal(None);
    let (over, set_over) = signal(None);

    let on_pointer_move = move |e: PointerEvent| {
        if dragging().is_some() {
            let x = e.client_x();
            let y = e.client_y();
            if let Some(index) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|doc| doc.element_from_point(x as f32, y as f32))
                .and_then(|el| el.closest("[data-index]").ok().flatten())
                .and_then(|el| el.get_attribute("data-index"))
                .and_then(|s| s.parse::<usize>().ok())
            {
                set_over(Some(index));
                return;
            }
            set_over(None);
        }
    };

    let reset = move |_| {
        set_dragging(None);
        set_over(None);
    };

    let on_pointer_up = move |e: PointerEvent| {
        if let (Some(start), Some(end)) = (dragging(), over()) {
            #[allow(clippy::collapsible_if)]
            if start != end {
                items.update(|items| {
                    let item = items.remove(start);
                    items.insert(end, item);
                });
            }
        }
        if let Some(target) = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        {
            let _ = target.release_pointer_capture(e.pointer_id());
        }
        reset(e);
    };

    view! {
        <div
            class="reorderable-list flex flex-col gap-2"
            on:pointermove=on_pointer_move
            on:pointerup=on_pointer_up
            on:pointercancel=reset
        >
            <For
                each=move || items().into_iter().enumerate()
                key=move |(id, _)| *id
                children=move |(id, child)| {
                    view! {
                        <div
                            data-index=id
                            on:pointerdown=move |e| {
                                if e.button() == 0 {
                                    set_dragging(Some(id));
                                    set_over(Some(id));
                                    if let Some(target) = e
                                        .target()
                                        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                                    {
                                        let _ = target.set_pointer_capture(e.pointer_id());
                                    }
                                }
                            }

                            class:drop-hint=move || {
                                over() == Some(id) && dragging().is_some() && dragging() != Some(id)
                            }
                            class:drag-active=move || dragging() == Some(id)
                            style="touch-action: none; user-select: none;"
                        >
                            {item_view(child)}
                        </div>
                    }
                }
            />
        </div>
    }
    .into_any()
}
