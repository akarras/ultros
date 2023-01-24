use leptos::*;
use leptos_router::use_params_map;

use crate::api::get_list_items;
use crate::components::{item_icon::*, loading::*};

#[component]
pub fn ListView(cx: Scope) -> impl IntoView {
    let params = use_params_map(cx);
    let list_view = create_resource(
        cx,
        move || {
            params
                .with(|p| {
                    p.get("id")
                        .as_ref()
                        .map(|id| id.parse::<i32>().ok())
                        .flatten()
                })
                .unwrap_or_default()
        },
        move |id| get_list_items(cx, id),
    );
    view! {cx,
        <div class="container">
            <div class="main-content">
                <Suspense fallback=move || view!{cx, <Loading />}>
                {move || list_view().map(move |list| match list {
                    Some((list, items)) => view!{cx,
                        <div class="content-well">
                            <span class="content-title">{list.name}</span>
                            <For each=move || items.clone() key=|item| item.id view=move |item| view!{cx, <tr>
                                <td>
                                    <ItemIcon item_id=item.item_id icon_size=IconSize::Small/>

                                </td>
                                <td>
                                </td>
                            </tr>}
                            />
                        </div>},
                    None => view!{cx, <div>"Failed to get items"</div>}
                })
                }
                </Suspense>
            </div>
    </div>}
}
