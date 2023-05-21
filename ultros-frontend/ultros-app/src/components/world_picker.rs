use leptos::*;

use crate::global_state::LocalWorldData;
use ultros_api_types::{world::World, world_helper::AnySelector};

#[component]
pub fn WorldOnlyPicker(
    cx: Scope,
    current_world: Signal<Option<World>>,
    set_current_world: SignalSetter<Option<World>>,
) -> impl IntoView {
    let local_worlds = use_context::<LocalWorldData>(cx)
        .expect("Local world data should always be present")
        .0;
    match local_worlds {
        Ok(worlds) => {
            let data = worlds.get_all().clone();
            view!{cx,
                <select on:change=move |input| {
                    let id = event_target_value(&input);
                    // let (_world_type, id) = world_target.split_once(":").unwrap();
                    let id = id.parse().unwrap();
                    let world = worlds.lookup_selector(AnySelector::World(id)).and_then(|world| world.as_world().map(|w| w.clone()));
                    set_current_world(world);
                }>
            {data.regions.into_iter().map(|region| {
                view!{cx, // <optgroup label=region.name>
                {region.datacenters.into_iter().map(|datacenter| {
                    view!{cx, // <optgroup label=datacenter.name>
                    {datacenter.worlds.into_iter().map(|world| {
                        view!{cx, <option value=world.id prop:selected=move || {
                            current_world.with(|w| w.as_ref().map(|w| w.id).unwrap_or_default() == world.id)
                        }>
                            {&world.name}
                            </option>}
                    }).collect::<Vec<_>>()}
                    // </optgroup>
                    }
                }).collect::<Vec<_>>()}
                // </optgroup>
                }
            }).collect::<Vec<_>>()}

            </select>}.into_view(cx)
        }
        Err(e) => view! {cx, <div><span>"No worlds"</span>
        <span>{e.to_string()}</span></div>}
        .into_view(cx),
    }
}

/// Changes a world, but does not allow a null option.
#[component]
pub fn WorldPicker(
    cx: Scope,
    current_world: Signal<Option<AnySelector>>,
    set_current_world: SignalSetter<Option<AnySelector>>,
) -> impl IntoView {
    let local_worlds = use_context::<LocalWorldData>(cx)
        .expect("Local world data should always be present")
        .0;
    match local_worlds {
        Ok(worlds) => {
            let data = worlds.get_all().clone();
            // TODO: include a current world default option in the picker
            view!{cx,
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
                    set_current_world(Some(selector))
                }>
                    {data.regions.into_iter().map(|region| {
                        view!{cx, <option value=move || format!("region:{}", region.id) prop:selected=move || current_world().map(|w| w == AnySelector::Region(region.id)).unwrap_or_default()>{&region.name}</option>
                        {region.datacenters.into_iter().map(|datacenter| {
                            view!{cx, <option value=move || format!("datacenter:{}", datacenter.id) prop:selected=move || current_world().map(|w| w == AnySelector::Datacenter(datacenter.id)).unwrap_or_default()>{&datacenter.name}</option>
                            {datacenter.worlds.into_iter().map(|world| {
                                view!{cx, <option value=move || {format!("world:{}", world.id)} prop:selected=move || current_world().map(|w| w == AnySelector::World(world.id)).unwrap_or_default()>
                                    {&world.name}
                                    </option>}
                            }).collect::<Vec<_>>()}
                            }
                        }).collect::<Vec<_>>()}
                        }
                    }).collect::<Vec<_>>()}
                    </select>
                    }.into_view(cx)
        }
        Err(e) => view! {cx, <div><span>"No worlds"</span>
        <span>{e.to_string()}</span></div>}
        .into_view(cx),
    }
}
