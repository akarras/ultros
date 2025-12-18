use leptos::ev::{pointermove, pointerup, PointerEvent};
use leptos::html::Div;
use leptos::leptos_dom::helpers::{window_event_listener, WindowListenerHandle};
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;

#[component]
pub fn ReorderableList<T, V, N>(items: RwSignal<Vec<T>>, item_view: V) -> impl IntoView
where
    T: 'static + Clone + Send + Sync,
    V: Fn(T) -> N + 'static + Copy + Send + Sync,
    N: IntoView + 'static,
{
    let (dragging, set_dragging) = signal::<Option<usize>>(None);
    let (hovered_index, set_hovered_index) = signal::<Option<usize>>(None);
    let (drag_y, set_drag_y) = signal(0);
    let (original_y, set_original_y) = signal(0);
    let list_ref = NodeRef::<Div>::new();

    let move_handle = StoredValue::<Option<WindowListenerHandle>>::new(None);
    let up_handle = StoredValue::<Option<WindowListenerHandle>>::new(None);

    let on_pointer_move = Callback::new(move |e: PointerEvent| {
        if dragging.get_untracked().is_some() {
            set_drag_y(e.y());
        }
    });

    let stop_dragging = Callback::new(move |_e: PointerEvent| {
        if let (Some(drag_index), Some(drop_index)) =
            (dragging.get_untracked(), hovered_index.get_untracked())
            && drag_index != drop_index
        {
            items.update(|items| {
                let item = items.remove(drag_index);
                items.insert(drop_index, item);
            });
        }
        set_dragging(None);
        set_hovered_index(None);

        move_handle.with_value(|handle| {
            if let Some(handle) = handle.take() {
                handle.remove();
            }
        });
        up_handle.with_value(|handle| {
            if let Some(handle) = handle.take() {
                handle.remove();
            }
        });
    });

    let start_dragging = move |id: usize, e: PointerEvent| {
        if let Some(target) = e
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
        {
            e.prevent_default();
            if let Err(e) = target.set_pointer_capture(e.pointer_id()) {
                log::warn!("Failed to set pointer capture: {:?}", e);
            }
        }
        set_dragging(Some(id));
        set_original_y(e.y());
        set_drag_y(e.y());

        let move_cb = on_pointer_move;
        let up_cb = stop_dragging;
        move_handle.set_value(Some(window_event_listener(pointermove, move |e| {
            move_cb(e);
        })));
        up_handle.set_value(Some(window_event_listener(pointerup, move |e| {
            up_cb(e);
        })));
    };

    view! {
        <div node_ref=list_ref>
            <For
                each=move || items().into_iter().enumerate()
                key=move |(id, _)| *id
                children=move |(id, child)| {
                    let is_dragging = move || dragging() == Some(id);
                    let is_hovered = move || hovered_index() == Some(id) && Some(id) != dragging();

                    let transform = move || {
                        if is_dragging() {
                            format!("translateY({}px)", drag_y() - original_y())
                        } else {
                            "".to_string()
                        }
                    };

                    view! {
                        <div
                            on:pointerdown=move |e| start_dragging(id, e)
                            on:pointerenter=move |_| {
                                if dragging().is_some() {
                                    set_hovered_index(Some(id));
                                }
                            }
                            style:transform=transform
                            class="reorderable-item"
                            class:dragging=is_dragging
                            class:drop-hint=is_hovered
                        >
                            {item_view(child)}
                        </div>
                    }
                }
            />
        </div>
    }
}
