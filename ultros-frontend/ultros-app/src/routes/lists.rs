use leptos::*;

use crate::api::get_lists;
use crate::components::{lists_nav::*, loading::*, world_name::*};
use ultros_api_types::world_helper::AnySelector;

pub fn AddItem(cx: Scope) -> impl IntoView {}

#[component]
pub fn Lists(cx: Scope) -> impl IntoView {
    let lists = create_resource(cx, move || {}, move |_| get_lists(cx));
    view! {cx,
    <div class="container">
        <ListsNav />
        <div class="main-content flex-column">
            <span class="content-title">"Lists"</span>
            <Suspense fallback=move || view!{cx, <Loading/>}>
            {move || lists().map(move |lists| {
                match lists {
                    Some(lists) => {
                        view!{cx,
                        <div class="content-well">
                            <For each=move || lists.clone()
                            key=move |list| list.id
                            view=move |list| view!{cx, <div>
                                    {list.name}
                                    {if let Some(world_id) = list.world_id {
                                        view!{cx, <WorldName id=AnySelector::World(world_id)/>}.into_view(cx)
                                    } else {
                                        ().into_view(cx)
                                    }}
                                </div>}
                            />
                        </div>}.into_view(cx)
                    },
                    None => {
                        view!{cx, "No lists"}.into_view(cx)
                    }
                }})
            }
            </Suspense>
        </div>
    </div>
    }
}
