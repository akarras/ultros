use crate::api::{create_list, delete_list, edit_list, get_lists};
use crate::components::{loading::*, tooltip::*, world_name::*, world_picker::*};
use leptos::*;
use ultros_api_types::list::{CreateList, List};

#[component]
pub fn EditLists(cx: Scope) -> impl IntoView {
    let delete_list = create_action(cx, move |id: &i32| delete_list(cx, *id));
    let edit_list = create_action(cx, move |list: &List| edit_list(cx, list.clone()));
    let create_list = create_action(cx, move |list: &CreateList| create_list(cx, list.clone()));
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
    view! {cx,
    <div class="flex-row">
        <span class="content-title">"Edit Lists"</span>
        <Tooltip tooltip_text="Create list".to_string()>
            <button class="btn" on:click=move |_| set_creating(!creating())><i class="fa-solid fa-plus"></i></button>
        </Tooltip>
    </div>
    {move || creating().then(|| {
        let (new_list, set_new_list) = create_signal(cx, "".to_string());
        let (wdr_filter, set_wdr_filter) = create_signal(cx, None); // this should be the user's homeworld by default

        view!{cx,
            <div class="content-well flex-column">
                <label for="list-name">"List name:"</label>
                <input id="list-name" prop:value=new_list on:input=move |input| set_new_list(event_target_value(&input)) />
                <label>"Market world: " {move || wdr_filter().map(|world| view!{cx, <WorldName id=world/>})}</label>
                <WorldPicker current_world=wdr_filter.into() set_current_world=set_wdr_filter.into() />
                <button prop:disabled=move || wdr_filter().is_none() class="btn" on:click=move |_| {
                if let Some(wdr_filter) = wdr_filter() {
                    set_creating(false);
                    let list = CreateList {name: new_list(), wdr_filter};
                    create_list.dispatch(list);
                    set_new_list("".to_string());
                }
            }><i class="fa-solid fa-floppy-disk"></i></button>
        </div>
    }
    })}
    <div class="content-well">
        <Suspense fallback=move || view!{cx, <Loading />}>
        {move || lists.read(cx).map(|lists| {
            match lists {
                Ok(lists) => view!{cx,

                    <h3>"Current lists"</h3>
                    <table>
                    <tr><td>"List Name"</td><td>"World"</td></tr>
                        <For each=move || lists.clone()
                            key=move |list| list.id
                            view=move |cx, list| {
                                let (is_edit, set_is_edit) = create_signal(cx, false);
                                let (list, _set_list) = create_signal(cx, list);
                                let (name, set_name) = create_signal(cx, list().name);
                                let (current_world, set_current_world) = create_signal(cx, Some(list().wdr_filter));
                                view!{cx, <tr>
                                    {move || if is_edit() {
                                        view!{cx, <td>
                                                <input prop:value=name on:input=move |input| set_name(event_target_value(&input))/>
                                            </td>
                                            <td>
                                                <WorldPicker current_world=current_world.into() set_current_world=set_current_world.into() />
                                            </td>
                                        }.into_view(cx)
                                    } else {
                                        view!{cx, <td><a href=format!("/list/{}", list().id)>{list().name}</a></td><td><WorldName id=list().wdr_filter/></td>}.into_view(cx)
                                    }}
                                    <td>
                                        {move || if is_edit() {
                                            view!{cx, <button class="btn" on:click=move |_| {
                                                let mut list = list();
                                                list.name = name();
                                                if let Some(world) = current_world() {
                                                    list.wdr_filter = world;
                                                }
                                                edit_list.dispatch(list);
                                            } >
                                                <i class="fa-solid fa-check"></i>
                                            </button>
                                            <button class="btn" on:click=move |_| delete_list.dispatch(list().id)>
                                                <i class="fa-solid fa-trash"></i>
                                            </button>
                                        }.into_view(cx)
                                        } else {
                                            view!{cx,  <button class="btn" on:click=move |_| set_is_edit(true)>
                                            <i class="fa-solid fa-pencil"></i>
                                        </button>}.into_view(cx)
                                        }}
                                    </td>
                                </tr>}
                            }
                        />
                    </table>}.into_view(cx),
                Err(e) => view!{cx, <div>{format!("Error getting listings\n{e}")}</div>}.into_view(cx)
            }
        })}
        </Suspense>
    </div>}
}
