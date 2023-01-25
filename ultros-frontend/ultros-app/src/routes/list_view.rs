use std::cmp::Reverse;

use leptos::*;
use leptos_router::use_params_map;
use ultros_api_types::list::ListItem;
use xiv_gen::ItemId;

use crate::api::{add_item_to_list, delete_list_item, get_list_items};
use crate::components::{item_icon::*, item_icon::*, loading::*};

#[component]
pub fn ListView(cx: Scope) -> impl IntoView {
    let params = use_params_map(cx);
    let add_item = create_action(cx, move |list_item: &ListItem| {
        let item = list_item.clone();
        add_item_to_list(cx, item.list_id, item)
    });
    let delete_item = create_action(cx, move |list_item: &i32| delete_list_item(cx, *list_item));
    let list_view = create_resource(
        cx,
        move || {
            (
                add_item.version().get(),
                delete_item.version().get(),
                params
                    .with(|p| {
                        p.get("id")
                            .as_ref()
                            .map(|id| id.parse::<i32>().ok())
                            .flatten()
                    })
                    .unwrap_or_default(),
            )
        },
        move |(_, _, id)| get_list_items(cx, id),
    );
    let (item_menu, set_item_menu) = create_signal(cx, false);
    let game_items = &xiv_gen_db::decompress_data().items;
    view! {cx,
        <div class="container">
            <div class="main-content">
                <button class="btn" on:click=move |_| set_item_menu(!item_menu())><i class="fa-solid fa-plus"></i></button>
                {move || item_menu().then(|| {
                    let (search, set_search) = create_signal(cx, "".to_string());
                    let items = &xiv_gen_db::decompress_data().items;
                    let item_search = move || {
                        search.with(|s| {
                            let mut score = items
                                .into_iter()
                                .filter(|(_, i)| i.item_search_category.0 > 0)
                                .filter(|_| !s.is_empty())
                                .flat_map(|(id, i)| sublime_fuzzy::best_match(s, &i.name).map(|m| (id, i, m)))
                                .collect::<Vec<_>>();
                            score.sort_by_key(|(_, i, m)| (Reverse(m.score()), Reverse(i.level_item.0)));
                            score
                                .into_iter()
                                .filter(|(_, _, ma)| ma.score() > 0)
                                .map(|(id, item, ma)| (id, item, ma))
                                .take(10)
                                .collect::<Vec<_>>()
                        })
                    };
                    view!{cx, <div>
                            <input prop:value=search on:input=move |input| set_search(event_target_value(&input)) />
                            <div class="content-well flex-column">
                                {move || {
                                    let search = item_search()
                                        .into_iter()
                                        .map(move |(id, item, _)| view!{cx, <span>
                                                <ItemIcon item_id=id.0 icon_size=IconSize::Medium/>{&item.name}
                                                <button class="btn" on:click=move |_| {
                                                    let item = ListItem { item_id: id.0, list_id: params
                                                        .with(|p| {
                                                            p.get("id")
                                                                .as_ref()
                                                                .map(|id| id.parse::<i32>().ok())
                                                                .flatten()
                                                        })
                                                        .unwrap_or_default(), ..Default::default() };
                                                    add_item.dispatch(item);
                                                }><i class="fa-solid fa-plus"></i></button>
                                            </span>}).collect::<Vec<_>>();
                                    view!{cx, {search}}
                                }}
                            </div>
                        </div>}
                })}
                <Suspense fallback=move || view!{cx, <Loading />}>
                {move || list_view().map(move |list| match list {
                    Some((list, items)) => view!{cx,
                        <div class="content-well">
                            <span class="content-title">{list.name}</span>
                            <table>
                                <tr>
                                    <th>"Item"</th>
                                    <th>"Price"</th>
                                </tr>
                                <For each=move || items.clone() key=|item| item.id view=move |item| view!{cx, <tr>
                                    <td>
                                        <ItemIcon item_id=item.item_id icon_size=IconSize::Small/>
                                        {game_items.get(&ItemId(item.item_id)).map(|item| &item.name)}
                                    </td>
                                    <td>
                                        <button class="btn" on:click=move |_| {delete_item.dispatch(item.id)}>
                                            <i class="fa-solid fa-trash"></i>
                                        </button>
                                    </td>
                                </tr>}
                                />
                            </table>
                        </div>},
                    None => view!{cx, <div>"Failed to get items"</div>}
                })
                }
                </Suspense>
            </div>
    </div>}
}
