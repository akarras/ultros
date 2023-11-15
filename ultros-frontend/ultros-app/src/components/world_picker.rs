use leptos::*;

use crate::{components::select::Select, global_state::LocalWorldData};
use ultros_api_types::{world::World, world_helper::AnySelector};

#[component]
pub fn WorldOnlyPicker(
    current_world: Signal<Option<World>>,
    set_current_world: SignalSetter<Option<World>>,
) -> impl IntoView {
    let local_worlds = use_context::<LocalWorldData>()
        .expect("Local world data should always be present")
        .0;
    match local_worlds {
        Ok(worlds) => {
            let data = worlds.get_inner_data().clone();
            view!{
                <select class="p-1" on:change=move |input| {
                    let id = event_target_value(&input);
                    // let (_world_type, id) = world_target.split_once(":").unwrap();
                    let id = id.parse().unwrap();
                    let world = worlds.lookup_selector(AnySelector::World(id)).and_then(|world| world.as_world()).cloned();
                    set_current_world(world);
                }>
            {data.regions.into_iter().map(|region| {
                view!{// <optgroup label=region.name>
                {region.datacenters.into_iter().map(|datacenter| {
                    view!{// <optgroup label=datacenter.name>
                    {datacenter.worlds.into_iter().map(|world| {
                        view!{<option value=world.id prop:selected=move || {
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

            </select>}.into_view()
        }
        Err(e) => view! {<div><span>"No worlds"</span>
        <span>{e.to_string()}</span></div>}
        .into_view(),
    }
}

/// Changes a world, but does not allow a null option.
#[component]
pub fn WorldPicker(
    current_world: Signal<Option<AnySelector>>,
    set_current_world: SignalSetter<Option<AnySelector>>,
) -> impl IntoView {
    let local_worlds = use_context::<LocalWorldData>()
        .expect("Local world data should always be present")
        .0;

    match local_worlds {
        Ok(worlds) => {
            let worlds_1 = worlds.clone();
            let data = create_memo(move |_| {
                worlds
                    .iter()
                    .map(|l| (l.get_name().to_string(), AnySelector::from(&l)))
                    .collect::<Vec<_>>()
            });
            let choice = create_memo(move |_| {
                current_world().and_then(|world| {
                    worlds_1
                        .lookup_selector(world)
                        .map(|r| (r.get_name().to_string(), world))
                })
            })
            .into();
            let set_choice = move |option: Option<(String, AnySelector)>| {
                set_current_world(option.map(|(_, s)| s));
            };
            let set_choice = set_choice.into_signal_setter();
            view! { <Select items=data.into()
                choice=choice
                set_choice=set_choice
                as_label=move |(d, _)| d.clone()
                children=move |(_, s), view| view!{
                    <div class="flex flex-row gap-4"><div>{view}</div><div>{match s{
                    AnySelector::World(_) => "world",
                    AnySelector::Region(_) => "region",
                    AnySelector::Datacenter(_) => "datacenter",
                }}</div>
                </div>
                }

                />
            }
            .into_view()

            // data.regions.into_iter().map(|r| {
            //     r.datacenters
            //         .into_iter()
            //         .map(|d| d.worlds.into_iter().map(|w| AnyResult::World(w)))
            // })
        }
        Err(e) => view! {<div><span>"No worlds"</span>
        <span>{e.to_string()}</span></div>}
        .into_view(),
    }
}
