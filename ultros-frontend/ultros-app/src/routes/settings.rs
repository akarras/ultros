use crate::api::{
    check_character_verification, claim_character, delete_user, get_character_verifications,
    get_characters, search_characters, unclaim_character,
};
use crate::components::meta::{MetaDescription, MetaTitle};
use crate::components::{ad::*, loading::*, toggle::Toggle, world_name::*, world_picker::*};
use crate::error::AppResult;
use crate::global_state::cookies::Cookies;
use crate::global_state::home_world::{
    get_price_zone, result_to_selector_read, selector_to_setter_signal, use_home_world,
};
use leptos::either::{Either, EitherOf3};
use leptos::prelude::*;
use leptos::reactive::wrappers::write::IntoSignalSetter;
use leptos::task::spawn_local;
use leptos_icons::Icon;

use icondata as i;
use log::info;
use ultros_api_types::world_helper::AnySelector;

#[component]
fn AddCharacterMenu(claim_character: Action<i32, AppResult<(i32, String)>>) -> impl IntoView {
    let (is_open, set_is_open) = signal(false);
    let (character_search, set_character_search) = signal("".to_string());
    let search_action = Action::new(move |search: &String| search_characters(search.to_string()));

    view! {
        <button
            class="px-4 py-2 rounded-lg bg-violet-900/30 hover:bg-violet-800/40
                   border border-white/10 hover:border-yellow-200/30
                   transition-all duration-300 text-amber-200 hover:text-amber-100
                   flex items-center gap-2"
            on:click=move |_| set_is_open(!is_open())
        >
            <Icon icon=i::BiPlusRegular/>
            "Add Character"
        </button>

        {move || {
            claim_character
                .value()()
                .map(|result| {
                    view! {
                        <div class="mt-4 p-4 rounded-xl bg-violet-900/20 border border-white/10 backdrop-blur-sm">
                            {match result {
                                Ok((_id, value)) => Either::Left(view! {
                                    <div class="text-green-400">
                                        "Successfully started claim. Add "
                                        <span class="font-medium">{value}</span>
                                        " to your lodestone profile"
                                    </div>
                                }),
                                Err(e) => Either::Right(view! {
                                    <div class="text-red-400">
                                        "Error adding character to your profile: "
                                        {e.to_string()}
                                    </div>
                                }),
                            }}
                        </div>
                    }
                })
        }}

        {move || {
            is_open()
                .then(|| {
                    view! {
                        <div class="mt-4 p-6 rounded-xl bg-violet-900/20 border border-white/10 backdrop-blur-sm">
                            <h3 class="text-xl font-bold text-amber-200 mb-4">"Search Character"</h3>
                            <div class="flex gap-2">
                                <input
                                    class="flex-grow p-2 rounded-lg bg-violet-950/50 border border-white/10
                                           focus:outline-none focus:border-yellow-200/30 transition-colors"
                                    placeholder="Enter character name..."
                                    prop:value=character_search
                                    on:input=move |input| set_character_search(event_target_value(&input))
                                />
                                <button
                                    class="px-4 py-2 rounded-lg bg-violet-900/30 hover:bg-violet-800/40
                                           border border-white/10 hover:border-yellow-200/30
                                           transition-all duration-300 text-amber-200 hover:text-amber-100"
                                    on:click=move |_| {let _ = search_action.dispatch(character_search());}
                                >
                                    <Icon icon=i::AiSearchOutlined/>
                                </button>
                            </div>

                            <div class="mt-4">
                                {search_action.pending()().then(|| view! { <Loading/> })}
                                {search_action
                                    .value()()
                                    .map(|value| match value {
                                        Ok(characters) => {
                                            Either::Left(view! {
                                                <div class="space-y-2">
                                                    <h4 class="text-lg font-medium text-amber-200">"Search Results"</h4>
                                                    {if characters.is_empty() {
                                                        Either::Left(view! {
                                                            <div class="text-gray-400 italic">
                                                                "No characters found"
                                                            </div>
                                                        })
                                                    } else {
                                                        Either::Right(view! {
                                                            <div class="space-y-2">
                                                                {characters
                                                                    .into_iter()
                                                                    .map(|character| {
                                                                        view! {
                                                                            <div class="flex items-center justify-between p-2 rounded-lg
                                                                                        bg-violet-950/30 border border-white/5">
                                                                                <div class="flex items-center gap-4">
                                                                                    <span class="text-amber-100">
                                                                                        {character.first_name} " " {character.last_name}
                                                                                    </span>
                                                                                    <span class="text-gray-400">
                                                                                        <WorldName id=AnySelector::World(character.world_id)/>
                                                                                    </span>
                                                                                </div>
                                                                                <button
                                                                                    class="px-3 py-1 rounded-lg bg-violet-800/30 hover:bg-violet-700/40
                                                                                           border border-white/10 hover:border-yellow-200/30
                                                                                           transition-all duration-300 text-amber-200 hover:text-amber-100"
                                                                                    on:click=move |_| {
                                                                                        set_is_open(false);
                                                                                        claim_character.dispatch(character.id);
                                                                                    }
                                                                                >
                                                                                    "Claim"
                                                                                </button>
                                                                            </div>
                                                                        }
                                                                    })
                                                                    .collect::<Vec<_>>()}
                                                            </div>
                                                        })
                                                    }}
                                                </div>
                                            })
                                        }
                                        Err(e) => Either::Right(view! {
                                            <div class="text-red-400">
                                                "Failed to load characters: "
                                                {e.to_string()}
                                            </div>
                                        })
                                    })}
                            </div>
                        </div>
                    }
                })
        }}
    }
}

#[component]
fn HomeWorldPicker() -> impl IntoView {
    let (homeworld, set_homeworld) = use_home_world();
    let (price_region, set_price_region) = get_price_zone();
    let price_region = result_to_selector_read(price_region);
    let set_price_region = selector_to_setter_signal(set_price_region);

    view! {
        <div class="p-6 rounded-xl bg-gradient-to-br from-violet-900/30 to-amber-500/20
                    border border-white/10 backdrop-blur-sm">
            <h3 class="text-2xl font-bold text-amber-200 mb-4">"World Settings"</h3>
            <div class="grid md:grid-cols-3 gap-6">
                <div class="space-y-2">
                    <label class="text-lg text-amber-100">"Home World"</label>
                    <WorldOnlyPicker current_world=homeworld set_current_world=set_homeworld/>
                    <p class="text-sm text-gray-400">
                        "The home world will default for the analyzer and several other pages"
                    </p>
                </div>

                <div class="space-y-2">
                    <label class="text-lg text-amber-100">"Default Price Zone"</label>
                    <WorldPicker current_world=price_region set_current_world=set_price_region/>
                    <p class="text-sm text-gray-400">
                        "What world/region to show prices by default for within "
                        <a href="/items" class="text-amber-200 hover:text-amber-100 transition-colors">
                            "items"
                        </a>
                        " pages"
                    </p>
                </div>
            </div>
        </div>
    }
}

#[component]
fn AdChoice() -> impl IntoView {
    let ad_choice = use_context::<Cookies>().unwrap();
    let (cookie, set_cookie) = ad_choice.use_cookie_typed::<_, bool>("HIDE_ADS");

    view! {
        <div class="p-6 rounded-xl bg-gradient-to-br from-violet-900/30 to-amber-500/20
                    border border-white/10 backdrop-blur-sm">
            <h3 class="text-2xl font-bold text-amber-200 mb-4">"Ad Settings"</h3>
            <div class="grid md:grid-cols-3 gap-6">
                <div class="col-span-2 space-y-2">
                    <p class="text-gray-300">
                        "If you do not wish to see ads, this toggle will remove them entirely from the site.
                         Use of adblockers is fine as well. If you wish to support the site, do consider
                         leaving this disabled."
                    </p>
                    <p class="text-sm text-gray-400 italic">
                        "This is mostly an experiment for the time being."
                    </p>
                </div>

                <div>
                    <Toggle
                        checked=Signal::derive(move || {
                            let cookie = cookie();
                            info!("{cookie:?}");
                            cookie.unwrap_or_default()
                        })
                        set_checked={ (move |checked: bool| set_cookie(checked.then(|| true))).into_signal_setter() }
                        checked_label="Ads Disabled"
                        unchecked_label="Ads Enabled"
                    />
                </div>
            </div>

            <div class="mt-6">
                <Ad/>
            </div>
        </div>
    }
}

#[component]
fn DeleteUser() -> impl IntoView {
    let (confirmed, set_confirmed) = signal(false);

    view! {
        <div class="p-6 rounded-xl bg-red-900/20 border border-red-800/30 backdrop-blur-sm">
            <h3 class="text-2xl font-bold text-red-400 mb-4">"Delete Account"</h3>
            <p class="text-gray-300 mb-4">
                "DANGER: If you wish to delete your account and all information associated with it,
                 confirm with the toggle and then press the delete button"
            </p>

            <div class="space-y-4">
                <Toggle
                    checked=confirmed
                    set_checked=set_confirmed
                    checked_label="Yes, delete my account"
                    unchecked_label=""
                />

                <button
                    class=move || {
                        if confirmed() {
                            "px-4 py-2 rounded-lg bg-red-800 hover:bg-red-700
                             text-white transition-colors duration-200"
                        } else {
                            "px-4 py-2 rounded-lg bg-gray-700 text-gray-400 cursor-not-allowed"
                        }
                    }
                    on:click=move |_| {
                        if confirmed.get_untracked() {
                            spawn_local(async move { if let Ok(()) = delete_user().await {} });
                        }
                    }
                >
                    "Delete my account"
                </button>
            </div>
        </div>
    }
}

#[component]
pub fn Settings() -> impl IntoView {
    view! {
        <div class="main-content p-6">
            <MetaTitle title="Ultros settings page"/>
            <MetaDescription text="Manage settings such as homeworld or other for Ultros"/>

            <div class="container mx-auto max-w-7xl space-y-6">
                <h1 class="text-3xl font-bold text-amber-200">"Settings"</h1>
                <HomeWorldPicker/>
                <AdChoice/>
            </div>
        </div>
    }
}
#[component]
pub fn Profile() -> impl IntoView {
    let claim_character = Action::new(move |id: &i32| claim_character(*id));
    let unclaim_character = Action::new(move |id: &i32| unclaim_character(*id));
    let check_verification = Action::new(move |id: &i32| check_character_verification(*id));

    let characters = Resource::new(
        move || {
            (
                unclaim_character.version()(),
                check_verification.version()(),
            )
        },
        move |_| get_characters(),
    );
    let pending_verifications = Resource::new(
        move || (check_verification.version()(), claim_character.version()()),
        move |_| get_character_verifications(),
    );

    view! {
        <div class="main-content p-6">
            <div class="container mx-auto max-w-7xl space-y-6">
                <div class="flex items-center justify-between">
                    <h1 class="text-3xl font-bold text-amber-200">"Profile Settings"</h1>
                </div>

                <HomeWorldPicker/>
                <AdChoice/>

                // Characters Section
                <div class="p-6 rounded-xl bg-gradient-to-br from-violet-900/30 to-amber-500/20
                            border border-white/10 backdrop-blur-sm">
                    <div class="flex items-center justify-between mb-6">
                        <h2 class="text-2xl font-bold text-amber-200">"Characters"</h2>
                        <AddCharacterMenu claim_character/>
                    </div>

                    // Pending Verifications
                    <Suspense
                        fallback=move || {
                            view! {
                                <div class="flex items-center justify-center p-8">
                                    <Loading/>
                                </div>
                            }.into_any()
                        }
                    >
                    {move || pending_verifications.get().map(|verifications| match verifications {
                        Ok(verifications) if !verifications.is_empty() => EitherOf3::A(view! {
                            <div class="mb-6 space-y-4">
                                <h3 class="text-xl font-semibold text-amber-100">
                                    "Pending Verifications"
                                </h3>
                                <div class="space-y-3">
                                    {verifications
                                        .into_iter()
                                        .map(|_verification| {
                                            view! {
                                                <div class="p-4 rounded-lg bg-violet-950/30 border border-white/5 space-y-2">
                                                    // ... rest of the verification view ...
                                                </div>
                                            }
                                        })
                                        .collect::<Vec<_>>().into_any()}
                                </div>
                            </div>
                        }),
                        Ok(_) => EitherOf3::B(view! {
                            <div></div>
                        }),
                        Err(e) => EitherOf3::C(view! {
                            <div class="p-4 rounded-lg bg-red-900/20 border border-red-800/30 text-red-400">
                                "Unable to fetch verifications: "
                                {e.to_string()}
                            </div>
                        })
                    })}
                    </Suspense>

                    // Character List
                    <Suspense
                        fallback=move || {
                            view! {
                                <div class="flex items-center justify-center p-8">
                                    <Loading/>
                                </div>
                            }
                        }
                    >
                        {move || characters.get().map(|characters| match characters {
                            Ok(characters) if characters.is_empty() => {
                                EitherOf3::A(view! {
                                    <div class="text-center p-8 text-gray-400">
                                        "No characters added yet. Add a character to get started."
                                    </div>
                                })
                            }
                            Ok(characters) => {
                                EitherOf3::B(view! {
                                    <div class="space-y-3">
                                        {characters
                                            .into_iter()
                                            .map(|character| {
                                                view! {
                                                    <div class="flex items-center justify-between p-4
                                                                rounded-lg bg-violet-950/30 border border-white/5
                                                                group hover:border-white/10 transition-colors">
                                                        <div class="flex items-center gap-4">
                                                            <span class="text-amber-100">
                                                                {character.first_name}
                                                                " "
                                                                {character.last_name}
                                                            </span>
                                                            <span class="text-gray-400">
                                                                <WorldName id=AnySelector::World(character.world_id)/>
                                                            </span>
                                                        </div>
                                                        <button
                                                            class="p-2 rounded-lg bg-red-900/0 hover:bg-red-900/30
                                                                   border border-transparent hover:border-red-800/30
                                                                   text-gray-400 hover:text-red-400
                                                                   opacity-0 group-hover:opacity-100
                                                                   transition-all duration-200"
                                                            on:click=move |_| { let _ = unclaim_character.dispatch(character.id); }
                                                        >
                                                            <Icon icon=i::BiTrashSolid/>
                                                        </button>
                                                    </div>
                                                }
                                            })
                                            .collect::<Vec<_>>()}
                                    </div>
                                })
                            }
                            Err(e) => EitherOf3::C(view! {
                                <div class="p-4 rounded-lg bg-red-900/20 border border-red-800/30 text-red-400">
                                    "Unable to fetch characters: "
                                    {e.to_string()}
                                </div>
                            }),
                        })}
                    </Suspense>
                </div>

                // Delete Account Section
                <DeleteUser/>
            </div>
        </div>
    }
}
