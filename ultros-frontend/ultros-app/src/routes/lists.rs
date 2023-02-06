use leptos::*;

use crate::api::get_lists;
use crate::components::{lists_nav::*, loading::*, world_name::*};

#[component]
pub fn Lists(cx: Scope) -> impl IntoView {
    let lists = create_resource(cx, move || "lists", move |_| get_lists(cx));
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
                                    <a href=format!("/list/{}", list.id) style="font-size: 30px">
                                    {list.name}" - "
                                    <WorldName id=list.wdr_filter/>
                                    </a>
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
