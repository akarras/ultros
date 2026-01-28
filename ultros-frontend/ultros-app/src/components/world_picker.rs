use leptos::{
    either::Either,
    prelude::*,
    reactive::wrappers::write::{IntoSignalSetter, SignalSetter},
};

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
            let data = Memo::new(move |_| {
                worlds
                    .iter()
                    .filter_map(|w| w.as_world())
                    .cloned()
                    .collect::<Vec<_>>()
            });
            let left = view! {
                <div class="relative">
                    <Select
                        id="world-only-picker-listbox"
                        items=data.into()
                        as_label=move |w| w.name.clone()
                        choice=current_world
                        set_choice=set_current_world
                        children=move |_w, label| {
                            view! {
                                <div class="flex items-center px-4 py-2 rounded-lg transition-colors hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)]">
                                    {label}
                                </div>
                            }
                        }
                    />
                </div>
            };
            Either::Left(left)
        }
        Err(e) => Either::Right(view! {
            <div class="relative">
                <div class="text-red-400 p-2 rounded-lg bg-red-950/50 border border-red-800/30">
                    <span>"No worlds: "</span>
                    <span>{e.to_string()}</span>
                </div>
            </div>
        }),
    }
}

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
            let data = Memo::new(move |_| {
                worlds
                    .iter()
                    .map(|l| (l.get_name().to_string(), AnySelector::from(&l)))
                    .collect::<Vec<_>>()
            });
            let choice = Memo::new(move |_| {
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
            Either::Left(view! {
                <div class="relative">
                    <Select
                        id="world-picker-listbox"
                        items=data.into()
                        choice=choice
                        set_choice=set_choice
                        as_label=move |(d, _)| d.clone()
                        children=move |(_, s), view| {
                            view! {
                                <div class="flex justify-between px-4 py-2
                                hover:bg-[color:color-mix(in_srgb,var(--brand-ring)_12%,transparent)] rounded-lg transition-colors
                                items-end gap-2">
                                    <div>{view}</div>
                                    <div class="text-sm text-[color:var(--color-text-muted)]">
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
                </div>
            })
        }
        Err(e) => Either::Right(view! {
            <div class="relative z-[150]">
                <div class="text-red-400 p-2 rounded-lg bg-red-950/50 border border-red-800/30">
                    <span>"No worlds: "</span>
                    <span>{e.to_string()}</span>
                </div>
            </div>
        }),
    }
    .into_any()
}
