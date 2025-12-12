use std::collections::HashSet;

use leptos::prelude::*;
use ultros_api_types::world_helper::AnySelector;

use crate::global_state::{LocalWorldData, world_filter::WorldFilter};

#[component]
pub(crate) fn WorldFilterComponent() -> impl IntoView {
    let world_data = use_context::<LocalWorldData>().unwrap().0.unwrap();
    let filter = use_context::<RwSignal<WorldFilter>>().unwrap();
    let (show_modal, set_show_modal) = signal(false);
    let (regions, _) = signal(world_data.get_inner_data().regions.clone());
    let selected_worlds = RwSignal::new(HashSet::<AnySelector>::new());

    view! {
        <div>
            <button
                class="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded"
                on:click=move |_| set_show_modal(true)
            >
                "Filter Worlds"
            </button>
            <Show when=show_modal>
                <div class="fixed z-10 inset-0 overflow-y-auto">
                    <div class="flex items-end justify-center min-h-screen pt-4 px-4 pb-20 text-center sm:block sm:p-0">
                        <div class="fixed inset-0 transition-opacity" aria-hidden="true">
                            <div class="absolute inset-0 bg-gray-500 opacity-75"></div>
                        </div>
                        <div class="inline-block align-bottom bg-white rounded-lg text-left overflow-hidden shadow-xl transform transition-all sm:my-8 sm:align-middle sm:max-w-lg sm:w-full">
                            <div class="bg-white px-4 pt-5 pb-4 sm:p-6 sm:pb-4">
                                <h3 class="text-lg leading-6 font-medium text-gray-900">
                                    "Filter Worlds & Data Centers"
                                </h3>
                                <div class="mt-2">
                                    <p class="text-sm text-gray-500">
                                        "Select the worlds and data centers you want to exclude from the search."
                                    </p>
                                    <div class="mt-4">
                                        <For
                                            each=regions
                                            key=|region| region.id
                                            children=move |region| {
                                                let region_name = region.name.clone();
                                                view! {
                                                    <div>
                                                        <h4 class="font-bold">{region_name}</h4>
                                                        <For
                                                            each=move || region.datacenters.clone()
                                                            key=|dc| dc.id
                                                            children=move |dc| {
                                                                let dc_clone = dc.clone();
                                                                view! {
                                                                    <div class="ml-4">
                                                                        <label>
                                                                            <input
                                                                                type="checkbox"
                                                                                on:change=move |ev| {
                                                                                    let checked = event_target_checked(&ev);
                                                                                    let selector = AnySelector::Datacenter(dc_clone.id);
                                                                                    if checked {
                                                                                        selected_worlds.update(|s| { s.insert(selector); });
                                                                                    } else {
                                                                                        selected_worlds.update(|s| { s.remove(&selector); });
                                                                                    }
                                                                                }
                                                                            />
                                                                            {dc.name.clone()}
                                                                        </label>
                                                                        <For
                                                                            each=move || dc.worlds.clone()
                                                                            key=|world| world.id
                                                                            children=move |world| {
                                                                                let world_clone = world.clone();
                                                                                view! {
                                                                                    <div class="ml-8">
                                                                                        <label>
                                                                                            <input
                                                                                                type="checkbox"
                                                                                                on:change=move |ev| {
                                                                                                    let checked = event_target_checked(&ev);
                                                                                                    let selector = AnySelector::World(world_clone.id);
                                                                                                    if checked {
                                                                                                        selected_worlds.update(|s| { s.insert(selector); });
                                                                                                    } else {
                                                                                                        selected_worlds.update(|s| { s.remove(&selector); });
                                                                                                    }
                                                                                                }
                                                                                            />
                                                                                            {world.name.clone()}
                                                                                        </label>
                                                                                    </div>
                                                                                }
                                                                            }
                                                                        />
                                                                    </div>
                                                                }
                                                            }
                                                        />
                                                    </div>
                                                }
                                            }
                                        />
                                    </div>
                                </div>
                            </div>
                            <div class="bg-gray-50 px-4 py-3 sm:px-6 sm:flex sm:flex-row-reverse">
                                <button
                                    type="button"
                                    class="w-full inline-flex justify-center rounded-md border border-transparent shadow-sm px-4 py-2 bg-blue-600 text-base font-medium text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-blue-500 sm:ml-3 sm:w-auto sm:text-sm"
                                    on:click=move |_| {
                                        filter.update(|f| f.0 = selected_worlds.get_untracked());
                                        set_show_modal(false);
                                    }
                                >
                                    "Apply Filter"
                                </button>
                                <button
                                    type="button"
                                    class="mt-3 w-full inline-flex justify-center rounded-md border border-gray-300 shadow-sm px-4 py-2 bg-white text-base font-medium text-gray-700 hover:bg-gray-50 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 sm:mt-0 sm:ml-3 sm:w-auto sm:text-sm"
                                    on:click=move |_| set_show_modal(false)
                                >
                                    "Cancel"
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    }
}
