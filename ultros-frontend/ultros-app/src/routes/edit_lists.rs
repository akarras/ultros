use crate::api::{create_list, delete_list, edit_list, get_lists};
use crate::components::{lists_nav::*, loading::*, tooltip::*, world_name::*};
use crate::global_state::LocalWorldData;
use leptos::*;
use ultros_api_types::{
    list::{CreateList, List},
    world_helper::{AnyResult, AnySelector},
};

/// Changes a world, but does not allow a null option.
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
        <select on:change=move |input| {
            let world_target = event_target_value(&input);
            // world target should be in the form of world_type:id
            let (world_type, id) = world_target.split_once(":").unwrap();
            let id = id.parse().unwrap();
            let selector = match world_type {
                "world" => AnySelector::World(id),
                "datacenter" => AnySelector::Datacenter(id),
                "region" => AnySelector::Region(id),
                _ => panic!("Input type was a correct format {world_target}")
            };
            set_current_world(selector)
        }>
            <Suspense fallback=move || view!{cx, <Loading/>}>
                {local_worlds.read(cx).map(move |worlds| match worlds {
                    Ok(worlds) => {
                        let data = worlds.get_all().clone();
                        let current = worlds.lookup_selector(current_world()).map(|name| name.get_name().to_string());
                        view!{cx, <option>{current}</option>
                        {data.regions.into_iter().map(|region| {
                            view!{cx, <option value=move || format!("region:{}", region.id)>{&region.name}</option>
                            {region.datacenters.into_iter().map(|datacenter| {
                                view!{cx, <option value=move || format!("datacenter:{}", datacenter.id)>{&datacenter.name}</option>
                                {datacenter.worlds.into_iter().map(|world| {
                                    view!{cx, <option value=move || {format!("world:{}", world.id)}>
                                        {&world.name}
                                        </option>}
                                }).collect::<Vec<_>>()}
                                }
                            }).collect::<Vec<_>>()}
                            }
                        }).collect::<Vec<_>>()}}.into_view(cx)
                    },
                    Err(e) => {
                        view!{cx, <div><span>"No worlds"</span>
                            <span>{e.to_string()}</span></div>}.into_view(cx)
                    }
                })}
            </Suspense>
        </select>
    }
}

/// World list selector that is able to select a world when there isn't a world set at all
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
                {local_worlds.read(cx).map(move |worlds| match worlds.clone() {
                    Ok(worlds) => {
                        let current = current_world();
                        let current_value = current.map(|current| worlds.lookup_selector(current)).flatten();
                        let is_in = move |other : AnyResult| current_value.map(|value| value.is_in(&other)).unwrap_or_default();
                        let is_eq = move |other : AnyResult| current_value.map(|value| value == other).unwrap_or_default();
                        let data = worlds.get_all().clone();
                        view!{cx,
                        <div class="flex-row">
                            {data.regions.into_iter().map(|region| {
                                let region_selected = is_in(AnyResult::from(&region));
                                let is_region = is_eq(AnyResult::from(&region));
                                view!{cx, <div class="flex-column">
                                <div style="height: 40px" class="btn" class:active=is_region on:click=move |_| set_current_world(Some(AnySelector::Region(region.id)))>
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
                    Err(e) => {
                        view!{cx, <div>"No worlds"<br/>{e.to_string()}</div>}.into_view(cx)
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
                let (new_list, set_new_list) = create_signal(cx, "".to_string());
                let (wdr_filter, set_wdr_filter) = create_signal(cx, None); // this should be the user's homeworld by default

                view!{cx,
                    <div class="content-well flex-column">
                        <label for="list-name">"List name:"</label>
                        <input id="list-name" prop:value=new_list on:input=move |input| set_new_list(event_target_value(&input)) />
                        <label>"Market world: " {move || wdr_filter().map(|world| view!{cx, <WorldName id=world/>})}</label>
                        <FreshWorldList current_world=wdr_filter set_current_world=set_wdr_filter />
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
                                        let (current_world, set_current_world) = create_signal(cx, list().wdr_filter);
                                        view!{cx, <tr>
                                            {move || if is_edit() {
                                                view!{cx, <td>
                                                        <input prop:value=name on:input=move |input| set_name(event_target_value(&input))/>
                                                    </td>
                                                    <td>
                                                        <EditWorldList current_world set_current_world />
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
                                                        list.wdr_filter = current_world();
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
            </div>
        </div>
    </div>}
}
