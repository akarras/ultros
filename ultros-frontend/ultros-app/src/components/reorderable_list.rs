use leptos::html::Div;
use leptos::prelude::*;
use leptos::ev::PointerEvent;
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

    let start_dragging = move |id: usize, e: PointerEvent| {
        if let Some(target) = e
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
        {
            e.prevent_default();
            target.set_pointer_capture(e.pointer_id());
        }
        set_dragging(Some(id));
        set_original_y(e.y());
        set_drag_y(e.y());
    };

    let stop_dragging = move |_e: PointerEvent| {
        if let (Some(drag_index), Some(drop_index)) = (dragging.get_untracked(), hovered_index.get_untracked())
            if drag_index != drop_index {
            items.update(|items| {
                let item = items.remove(drag_index);
                items.insert(drop_index, item);
            });
        }
        set_dragging(None);
        set_hovered_index(None);
    };

    let on_pointer_move = move |e: PointerEvent| {
        if dragging.get_untracked().is_some() {
            set_drag_y(e.y());
        }
    };

    view! {
        <div
            node_ref=list_ref
            on:pointermove=on_pointer_move
            on:pointerup=stop_dragging
            on:pointercancel=stop_dragging
        >
            <For
                each=move || items().into_iter().enumerate()
                key=move |(id, _)| *id
                children=move |(id, child)| {
                    let is_dragging = move || dragging() == Some(id);
                    let is_hovered = move || hovered_index() == Some(id);

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
                            on:pointerenter=move |_| if dragging().is_some() { set_hovered_index(Some(id)) }
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
