use leptos::*;

#[component]
pub fn ReorderableList<T, V, N>(items: RwSignal<Vec<T>>, item_view: V) -> impl IntoView
where
    T: 'static + Clone,
    V: Fn(T) -> N + 'static + Copy,
    N: IntoView,
{
    let (dragging, set_dragging) = create_signal(None);

    {
        move || {
            items()
                .into_iter()
                .enumerate()
                .map(|(id, child)| {
                    let (hovered, set_hovered) = create_signal(false);
                    view! {<div draggable="true" on:drop=move |e| {
                        log::info!("Drop");
                        e.prevent_default();
                        let drop_index = id;
                        if let Some(dragging) = dragging() {
                            items.update(|items| {
                                let removed_item = items.remove(dragging);
                                items.insert(drop_index, removed_item);
                            });
                            set_dragging(None);
                        } else {
                            log::warn!("no item dragging?");
                        }


                    } on:dragend=move |_|  set_dragging(None)
                    on:dragstart=move |_| set_dragging(Some(id))
                    on:dragover=move |e| e.prevent_default()
                    on:dragenter=move |_| {set_hovered(true)}
                    on:dragleave=move |_| {set_hovered(false)}
                    class:drop-hint=hovered
                    class:drag-active=move || dragging().map(|drag| drag == id).unwrap_or_default()
                    >
                        // if this is the drag object, leave the view the same, otherwise swap it out.
                        {item_view(child)}
                    </div>}
                })
                .collect::<Vec<_>>()
        }
    }
}
