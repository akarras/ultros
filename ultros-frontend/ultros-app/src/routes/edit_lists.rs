use crate::api::{create_list, delete_list, edit_list, get_lists};
use crate::components::{lists_nav::*, loading::*, tooltip::*};
use leptos::*;
use ultros_api_types::list::List;

#[component]
pub fn EditLists(cx: Scope) -> impl IntoView {
    let delete_list = create_action(cx, move |id: &i32| delete_list(cx, *id));
    let edit_list = create_action(cx, move |list: &List| edit_list(cx, list.clone()));
    let create_list = create_action(cx, move |list: &List| create_list(cx, list.clone()));
    let lists = create_resource(
        cx,
        move || {
            (
                delete_list.version().get(),
                edit_list.version().get(),
                create_list.version().get(),
            )
        },
        move |_| get_lists(cx),
    );
    let (creating, set_creating) = create_signal(cx, false);
    let (new_list, set_new_list) = create_signal(cx, "".to_string());
    view! {cx, <div class="container">
        <ListsNav/>
        <div class="main-content">
            <div class="flex-row">
                <span class="content-title">"Edit Lists"</span>
                <Tooltip tooltip_text="Create list".to_string()>
                    <button class="btn" on:click=move |_| set_creating(!creating())><i class="fa-solid fa-plus"></i></button>
                </Tooltip>
            </div>
            {move || creating().then(|| {

                view!{cx, <input prop:value=new_list on:input=move |input| set_new_list(event_target_value(&input)) />
                <button class="btn" on:click=move |_| {
                    set_creating(false);
                    let list = List {name: new_list(), ..Default::default()};
                    create_list.dispatch(list);
                    set_new_list("".to_string());
                }><i class="fa-solid fa-floppy-disk"></i></button>}
            })}
            <Suspense fallback=move || view!{cx, <Loading />}>
            {move || lists().map(|lists| {
                match lists {
                    Some(lists) => view!{cx, <table>
                        <tr><td>"List Name"</td></tr>
                            <For each=move || lists.clone()
                                 key=move |list| list.id
                                 view=move |list| {
                                    let (is_edit, set_is_edit) = create_signal(cx, false);
                                    let (list, _set_list) = create_signal(cx, list);
                                    let (name, set_name) = create_signal(cx, list().name);
                                    view!{cx, <tr>
                                        <td>{move || if is_edit() {
                                            view!{cx, <input prop:value=name on:input=move |input| set_name(event_target_value(&input))/>}.into_view(cx)
                                        } else {
                                            view!{cx, {name()}}.into_view(cx)
                                        }}</td>
                                        <td>
                                            {move || if is_edit() {
                                                view!{cx, <button class="btn" on:click=move |_| {
                                                    let mut list = list();
                                                    list.name = name();
                                                    edit_list.dispatch(list);
                                                } >
                                                    <i class="fa-solid fa-check"></i>
                                                </button>}.into_view(cx)
                                            } else {
                                                view!{cx,  <button class="btn" on:click=move |_| set_is_edit(true)>
                                                <i class="fa-solid fa-pencil"></i>
                                            </button>}.into_view(cx)
                                            }}
                                            <button class="btn" on:click=move |_| delete_list.dispatch(list().id)>
                                                <i class="fa-solid fa-trash"></i>
                                            </button>
                                        </td>
                                    </tr>}
                                }
                            />
                        </table>}.into_view(cx),
                    None => view!{cx, <div>"Error getting lists"</div>}.into_view(cx)
                }
            })}
            </Suspense>
        </div>
    </div>}
}
