use leptos::either::Either;
use leptos::prelude::*;
use ultros_api_types::user::OwnedRetainer;
use ultros_api_types::world_helper::AnySelector;
use ultros_api_types::Retainer;

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
        <div class="retainer-list flex-column w-full max-w-lg">
            <MetaTitle title="Edit Retainers" />
            <span class="content-title">"Retainers"</span>
            <Transition fallback=move || {
                view! { <div></div> }
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
                                                                    <div>{character.first_name} " " {character.last_name}</div>
                                                                },
                                                            )
                                                        } else {
                                                            Either::Right(view! { <div>"No character"</div> })
                                                        }}

                                                        <div class="flex-column">
                                                            <ReorderableList
                                                                items=retainers
                                                                item_view=move |
                                                                    (owned, retainer): (OwnedRetainer, Retainer)|
                                                                {
                                                                    let owned_id = owned.id;
                                                                    let retainer_name = retainer.name.to_string();
                                                                    let world_id = retainer.world_id;
                                                                    view! {
                                                                        <div class="flex-row">
                                                                            <div class="flex w-full md:w-[300px]">
                                                                                <span class="w-full md:w-[200px] truncate">{retainer_name}</span>
                                                                                <span>
                                                                                    <WorldName id=AnySelector::World(world_id) />
                                                                                </span>
                                                                            </div>
                                                                            <button
                                                                                class="btn"
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
                                            // I have no idea how I would have found that the #[server] macro takes params as a struct
                                            // without the compiler just spelling it out for me

                                            // I have no idea how I would have found that the #[server] macro takes params as a struct
                                            // without the compiler just spelling it out for me

                                            // I have no idea how I would have found that the #[server] macro takes params as a struct
                                            // without the compiler just spelling it out for me

                                            <div>"Retainers" <br /> {e.to_string()}</div>
                                        },
                                    )
                                }
                            }
                        })
                }}

            </Transition>
        </div>
        <div class="retainer-search">
            <span class="content-title">"Search:"</span>
            <input
                prop:value=retainer_search
                on:input=move |input| set_retainer_search(event_target_value(&input))
            />
            <div class="retainer-results">
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
                                                <div class="content-well flex-column">
                                                    <For
                                                        each=move || retainers.clone()
                                                        key=move |retainer| retainer.id
                                                        children=move |retainer| {
                                                            let world = AnySelector::World(retainer.world_id);
                                                            view! {
                                                                <div class="card flex-row">
                                                                    <div class="flex w-full md:w-[300px]">
                                                                        <span class="w-full md:w-[200px] truncate">{retainer.name}</span>
                                                                        <WorldName id=world />
                                                                    </div>
                                                                    <button
                                                                        class="btn"
                                                                        on:click=move |_| {
                                                                            let _ = claim.dispatch(retainer.id);
                                                                        }
                                                                    >
                                                                        {move || match is_retainer_owned(retainer.id) {
                                                                            true => "Claimed",
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
                                            view! { <div>{format!("No retainers found\n{e}")}</div> },
                                        )
                                    }
                                }
                            })
                    }}

                </Suspense>
            </div>
        </div>
    }.into_any()
}
