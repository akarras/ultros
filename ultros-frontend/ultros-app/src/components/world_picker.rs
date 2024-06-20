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
            let data = create_memo(move |_| {
                worlds
                    .iter()
                    .filter_map(|w| w.as_world())
                    .cloned()
                    .collect::<Vec<_>>()
            });
            view! {
                <Select
                    items=data.into()
                    as_label=move |w| w.name.clone()
                    choice=current_world
                    set_choice=set_current_world
                    children=move |_w, label| {
                        view! { <div>{label}</div> }
                    }
                />
            }
        }
        Err(e) => view! {
            <div>
                <span>"No worlds"</span>
                <span>{e.to_string()}</span>
            </div>
        }
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
            view! {
                <Select
                    items=data.into()
                    choice=choice
                    set_choice=set_choice
                    as_label=move |(d, _)| d.clone()
                    children=move |(_, s), view| {
                        view! {
                            <div class="flex flex-row gap-4">
                                <div>{view}</div>
                                <div>
                                    {match s {
                                        AnySelector::World(_) => "world",
                                        AnySelector::Region(_) => "region",
                                        AnySelector::Datacenter(_) => "datacenter",
                                    }}
                                </div>
                            </div>
                        }
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
        Err(e) => view! {
            <div>
                <span>"No worlds"</span>
                <span>{e.to_string()}</span>
            </div>
        }
        .into_view(),
    }
}
