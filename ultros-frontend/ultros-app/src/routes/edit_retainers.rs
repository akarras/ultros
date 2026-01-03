use leptos::either::Either;
use leptos::prelude::*;
use ultros_api_types::Retainer;
use ultros_api_types::user::OwnedRetainer;
use ultros_api_types::world_helper::AnySelector;

use crate::api::{
    claim_retainer, get_retainers, search_retainers, unclaim_retainer, update_retainer_order,
};
use crate::components::{loading::*, meta::*, reorderable_list::*, world_name::*};

#[component]
pub fn EditRetainers() -> impl IntoView {
    // This page should let the user drag and drop retainers to reorder them
    // It should also support a search panel for retainers to the right that will allow the user to search for retainers

    let (retainer_search, set_retainer_search) = signal(String::new());

    let search_results = Resource::new(retainer_search, move |search| async move {
        search_retainers(search).await
    });

    let claim = Action::new(move |retainer_id| claim_retainer(*retainer_id));

    let remove_retainer = Action::new(move |owned_id| unclaim_retainer(*owned_id));
    let update_retainers =
        Action::new(move |owners: &Vec<OwnedRetainer>| update_retainer_order(owners.clone()));
    let retainers = Resource::new(
        move || {
            (
                claim.version().get(),
                remove_retainer.version().get(),
                // update_retainers.version().get(),
            )
        },
        move |key| {
            log::info!("getting retainers {key:?}");
            get_retainers()
        },
    );

    let is_retainer_owned = move |retainer_id: i32| {
        retainers
            .with(|retainer| {
                retainer.as_ref().map(|retainers| {
                    retainers.as_ref().ok().map(|r| {
                        r.retainers.iter().any(|(_character, retainers)| {
                            retainers
                                .iter()
                                .any(|(_, retainer)| retainer.id == retainer_id)
                        })
                    })
                })
            })
            .flatten()
            .unwrap_or_default()
    };

    view! {
        <div class="container mx-auto p-4 flex flex-col lg:flex-row gap-6 items-start justify-center">
            <MetaTitle title="Edit Retainers" />

            <div class="retainer-list panel p-6 flex flex-col w-full lg:w-1/2 gap-4">
                <h2 class="text-2xl font-bold mb-2">"Retainers"</h2>
                <Transition fallback=move || {
                    view! { <div class="loading loading-spinner loading-lg"></div> }
                }>
                    {move || {
                        retainers
                            .get()
                            .map(|retainers| {
                                match retainers {
                                    Ok(retainers) => {
                                        Either::Left(
                                            view! {
                                                {move || {
                                                    update_retainers
                                                        .value()
                                                        .with(|value| {
                                                            value.as_ref().map(|value| {
                                                                match value {
                                                                    Ok(_) => None,
                                                                    Err(e) => Some(format!("App error: {e:?}")),
                                                                }
                                                            })
                                                        })
                                                }}

                                                <For
                                                    each=move || retainers.retainers.clone()
                                                    key=move |(character, retainers)| (
                                                        character.as_ref().map(|c| c.id).unwrap_or_default(),
                                                        retainers.iter().map(|(o, _r)| o.id).collect::<Vec<_>>(),
                                                    )

                                                    children=move |(character, retainers)| {
                                                        let retainers = RwSignal::new(retainers);
                                                        Effect::new(move |_| {
                                                            let retainers = retainers();
                                                            let mut changed = false;
                                                            let retainers = retainers
                                                                .into_iter()
                                                                .enumerate()
                                                                .flat_map(|(i, (mut owned, _retainer))| {
                                                                    if let Some(weight) = &mut owned.weight {
                                                                        if *weight != i as i32 {
                                                                            changed = true;
                                                                            *weight = i as i32;
                                                                            return Some(owned);
                                                                        }
                                                                    } else {
                                                                        owned.weight = Some(i as i32);
                                                                        changed = true;
                                                                        return Some(owned);
                                                                    }
                                                                    None
                                                                })
                                                                .collect();
                                                            if changed {
                                                                log::info!("Updating retainer list");
                                                                update_retainers.dispatch(retainers);
                                                            }
                                                        });
                                                        view! {
                                                            // I have no idea how I would have found that the #[server] macro takes params as a struct
                                                            // without the compiler just spelling it out for me

                                                            {if let Some(character) = character {
                                                                Either::Left(
                                                                    view! {
                                                                        <h3 class="text-xl font-bold mt-4 mb-2">
                                                                            {character.first_name} " " {character.last_name}
                                                                        </h3>
                                                                    },
                                                                )
                                                            } else {
                                                                Either::Right(view! {
                                                                    <h3 class="text-xl font-bold mt-4 mb-2 text-gray-500">
                                                                        "Unassigned"
                                                                    </h3>
                                                                })
                                                            }}

                                                            <div class="flex flex-col gap-2">
                                                                <ReorderableList
                                                                    items=retainers
                                                                    item_view=move |
                                                                        (owned, retainer): (OwnedRetainer, Retainer)|
                                                                    {
                                                                        let owned_id = owned.id;
                                                                        let retainer_name = retainer.name.to_string();
                                                                        let world_id = retainer.world_id;
                                                                        view! {
                                                                            <div class="card bg-base-200 border border-base-300 p-3 rounded-xl flex flex-row items-center justify-between gap-4 mb-2 shadow-sm">
                                                                                <div class="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-4 overflow-hidden">
                                                                                    <span class="font-bold truncate text-lg">{retainer_name}</span>
                                                                                    <div class="opacity-80 text-sm">
                                                                                        <WorldName id=AnySelector::World(world_id) />
                                                                                    </div>
                                                                                </div>
                                                                                <button
                                                                                    class="btn btn-sm btn-error btn-outline"
                                                                                    on:click=move |_| {
                                                                                        let _ = remove_retainer.dispatch(owned_id);
                                                                                    }
                                                                                >
                                                                                    "Unclaim"
                                                                                </button>
                                                                            </div>
                                                                        }
                                                                    }
                                                                />

                                                            </div>
                                                        }
                                                    }
                                                />
                                            },
                                        )
                                    }
                                    Err(e) => {
                                        Either::Right(
                                            view! {
                                                <div class="alert alert-error">
                                                    <span>"Error loading retainers: " {e.to_string()}</span>
                                                </div>
                                            },
                                        )
                                    }
                                }
                            })
                    }}

                </Transition>
            </div>
            <div class="retainer-search panel p-6 flex flex-col w-full lg:w-1/2 gap-4">
                <h2 class="text-2xl font-bold mb-2">"Add Retainer"</h2>
                <input
                    class="input w-full bg-base-200"
                    prop:value=retainer_search
                    on:input=move |input| set_retainer_search(event_target_value(&input))
                    placeholder="Search for a retainer to add"
                />
                <div class="retainer-results flex flex-col gap-2">
                    <Suspense fallback=move || {
                        view! { <Loading /> }
                    }>
                        {move || {
                            search_results
                                .get()
                                .map(|retainers| {
                                    match retainers {
                                        Ok(retainers) => {
                                            Either::Left(
                                                view! {
                                                    <div class="flex flex-col gap-2">
                                                        <For
                                                            each=move || retainers.clone()
                                                            key=move |retainer| retainer.id
                                                            children=move |retainer| {
                                                                let world = AnySelector::World(retainer.world_id);
                                                                view! {
                                                                    <div class="card bg-base-200 border border-base-300 flex-row gap-2 p-3 items-center rounded-xl shadow-sm justify-between">
                                                                        <div class="flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-4 overflow-hidden">
                                                                            <span class="font-bold truncate">{retainer.name}</span>
                                                                            <div class="opacity-80 text-sm">
                                                                                <WorldName id=world />
                                                                            </div>
                                                                        </div>
                                                                        <button
                                                                            class:btn-disabled=move || is_retainer_owned(retainer.id)
                                                                            class="btn btn-primary btn-sm"
                                                                            on:click=move |_| {
                                                                                let _ = claim.dispatch(retainer.id);
                                                                            }
                                                                        >
                                                                            {move || match is_retainer_owned(retainer.id) {
                                                                                true => "Owned",
                                                                                false => "Claim",
                                                                            }}

                                                                        </button>
                                                                    </div>
                                                                }
                                                            }
                                                        />

                                                    </div>
                                                },
                                            )
                                        }
                                        Err(e) => {
                                            Either::Right(
                                                view! { <div class="text-center opacity-70 p-4">{format!("No retainers found\n{e}")}</div> },
                                            )
                                        }
                                    }
                                })
                        }}

                    </Suspense>
                </div>
            </div>
        </div>
    }.into_any()
}
