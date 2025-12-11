use leptos::{ev, leptos_dom::helpers::window_event_listener, prelude::*};

#[component]
pub fn ReorderableList<T, V, N>(items: RwSignal<Vec<T>>, item_view: V) -> impl IntoView
where
    T: 'static + Clone + Send + Sync,
    V: Fn(T) -> N + 'static + Copy + Send + Sync,
    N: IntoView + 'static,
{
    let (dragging, set_dragging) = signal(None::<usize>);
    let (drop_target, set_drop_target) = signal(None::<usize>);

    window_event_listener(ev::pointerup, move |_| {
        if let (Some(dragging_index), Some(drop_index)) =
            (dragging.get_untracked(), drop_target.get_untracked())
        {
            if dragging_index != drop_index {
                items.update(|items| {
                    // check bounds to prevent panic
                    if dragging_index < items.len() {
                        let removed_item = items.remove(dragging_index);
                        items.insert(drop_index, removed_item);
                    }
                });
            }
        }
        set_dragging(None);
    });

    view! {
        <For
            each=move || items().into_iter().enumerate()
            key=move |(id, _)| *id
            children=move |(id, child)| {
                view! {
                    <div
                        on:pointerdown=move |e| {
                            e.prevent_default();
                            set_dragging(Some(id));
                        }
                        on:pointerenter=move |_| {
                            if dragging().is_some() {
                                set_drop_target(Some(id));
                            }
                        }
                        class:drop-hint=move || {
                            if let (Some(dragging_id), Some(drop_id)) = (dragging(), drop_target()) {
                                dragging_id != id && drop_id == id
                            } else {
                                false
                            }
                        }
                        class:drag-active=move || {
                            dragging().map(|drag| drag == id).unwrap_or_default()
                        }
                    >
                        {item_view(child)}
                    </div>
                }
            }
        ></For>
    }
    .into_any()
}
