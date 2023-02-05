use crate::api::{create_list, delete_list, edit_list, get_lists};
use crate::components::{lists_nav::*, loading::*, tooltip::*};
use crate::global_state::LocalWorldData;
use leptos::*;
use ultros_api_types::{
    list::{CreateList, List},
    world_helper::{AnyResult, AnySelector},
};

#[component]
fn EditWorldList(
    cx: Scope,
    current_world: ReadSignal<AnySelector>,
    set_current_world: WriteSignal<AnySelector>,
) -> impl IntoView {
    let local_worlds = use_context::<LocalWorldData>(cx)
        .expect("Local world data should always be present")
        .0;
    view! {cx,
        <select>
            <Suspense fallback=move || view!{cx, <Loading/>}>
                {local_worlds().map(move |worlds| match worlds {
                    Some(worlds) => {
                        let data = worlds.get_all();
                        data.regions.iter().flat_map(|region| {
                            region.datacenters.iter().flat_map(|datacenter| {
                                datacenter.worlds.iter().map(|world| {
                                    view!{cx, <option>{&world.name}</option>}
                                })
                            })
                        }).collect::<Vec<_>>().into_view(cx)
                    },
                    None => {
                        "No worlds".into_view(cx)
                    }
                })}
            </Suspense>
        </select>
    }
}

#[component]
fn FreshWorldList(
    cx: Scope,
    current_world: ReadSignal<Option<AnySelector>>,
    set_current_world: WriteSignal<Option<AnySelector>>,
) -> impl IntoView {
    let local_worlds = use_context::<LocalWorldData>(cx)
        .expect("Local world data should always be present")
        .0;
    view! {cx,
        <div>
            <Suspense fallback=move || view!{cx, <Loading/>}>
                {local_worlds().map(move |worlds| match worlds.clone() {
                    Some(worlds) => {
                        let current = current_world();
                        let current_value = current.map(|current| worlds.lookup_selector(current)).flatten();
                        let is_in = move |other : AnyResult| current_value.map(|value| value.is_in(&other)).unwrap_or_default();
                        let is_eq = move |other : AnyResult| current_value.map(|value| value == other).unwrap_or_default();
                        let data = worlds.get_all().clone();
                        view!{cx,
                        <div class="flex-column">
                            {data.regions.into_iter().map(|region| {
                                let region_selected = is_in(AnyResult::from(&region));
                                let is_region = is_eq(AnyResult::from(&region));
                                view!{cx, <div class="flex-row">
                                <div style="height: 40px; width: 100px" class="btn" class:active=is_region on:click=move |_| set_current_world(Some(AnySelector::Region(region.id)))>
                                    {region.name.to_string()}
                                </div>
                                <div class="flex-column">
                                {region_selected.then(||
                                {region.datacenters
                                    .into_iter()
                                    .map(|datacenter| {
                                    let dc_selected = is_in(AnyResult::from(&datacenter));
                                    let is_dc = is_eq(AnyResult::from(&datacenter));
                                    view!{cx, <div class="flex-row">
                                        <div class="btn" class:active=is_dc on:click=move |_| set_current_world(Some(AnySelector::Datacenter(datacenter.id)))>
                                            {datacenter.name.to_string()}
                                        </div>
                                        {dc_selected.then(|| datacenter.worlds.into_iter().map(|world| {
                                            let is_world = is_eq(AnyResult::from(&world));
                                            view!{cx,
                                                <div class="btn" class:active=is_world on:click=move |_| set_current_world(Some(AnySelector::World(world.id)))>
                                                    {world.name.to_string()}
                                                </div>
                                            }
                                        }).collect::<Vec<_>>())}
                                    </div>}
                                    }).collect::<Vec<_>>()})}
                                    </div>
                                </div>}
                            }).collect::<Vec<_>>()}
                        </div>
                        }.into_view(cx)
                    },
                    None => {
                        "No worlds".into_view(cx)
                    }
                })}
            </Suspense>
        </div>
    }
}

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
    let (new_list, set_new_list) = create_signal(cx, "".to_string());

    let (wdr_filter, set_wdr_filter) = create_signal(cx, None); // this should be the user's homeworld by default
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
                    <FreshWorldList current_world=wdr_filter set_current_world=set_wdr_filter />
                <button prop:disabled=move || wdr_filter().is_none() class="btn" on:click=move |_| {
                    if let Some(wdr_filter) = wdr_filter() {
                        set_creating(false);
                        let list = CreateList {name: new_list(), wdr_filter};
                        create_list.dispatch(list);
                        set_new_list("".to_string());
                    }
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
                                    let (current_world, set_current_world) = create_signal(cx, list().wdr_filter);
                                    view!{cx, <tr>
                                        <td>{move || if is_edit() {
                                            view!{cx, <input prop:value=name on:input=move |input| set_name(event_target_value(&input))/>
                                                <EditWorldList current_world set_current_world />
                                            }.into_view(cx)
                                        } else {
                                            view!{cx, {name()}}.into_view(cx)
                                        }}</td>
                                        <td>
                                            {move || if is_edit() {
                                                view!{cx, <button class="btn" on:click=move |_| {
                                                    let mut list = list();
                                                    list.name = name();
                                                    list.wdr_filter = current_world();
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
