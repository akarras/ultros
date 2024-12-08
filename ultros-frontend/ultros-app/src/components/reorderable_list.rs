use leptos::prelude::*;

#[component]
pub fn ReorderableList<T, V, N>(items: RwSignal<Vec<T>>, item_view: V) -> impl IntoView
where
    T: 'static + Clone + Send + Sync,
    V: Fn(T) -> N + 'static + Copy + Send + Sync,
    N: IntoView + 'static,
{
    let (dragging, set_dragging) = signal(None);
    view! {
        <For each=move || items().into_iter().enumerate()
            key=move |(id, _)| *id
                children=move |(id, child)| {
                    let (hovered, set_hovered) = signal(false);
                    view! {
                        <div
                            draggable="true"
                            on:drop=move |e| {
                                log::info!("Drop");
                                e.prevent_default();
                                let drop_index = id;
                                if let Some(dragging) = dragging() {
                                    items
                                        .update(|items| {
                                            let removed_item = items.remove(dragging);
                                            items.insert(drop_index, removed_item);
                                        });
                                    set_dragging(None);
                                } else {
                                    log::warn!("no item dragging?");
                                }
                            }

                            on:dragend=move |_| set_dragging(None)
                            on:dragstart=move |_| set_dragging(Some(id))
                            on:dragover=move |e| e.prevent_default()
                            on:dragenter=move |_| { set_hovered(true) }
                            on:dragleave=move |_| { set_hovered(false) }
                            class:drop-hint=hovered
                            class:drag-active=move || {
                                dragging().map(|drag| drag == id).unwrap_or_default()
                            }
                        >

                            // if this is the drag object, leave the view the same, otherwise swap it out.
                            {item_view(child)}
                        </div>
                    }
                }>

        </For>
    }
}
