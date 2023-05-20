use leptos::*;
use ultros_api_types::world_helper::AnySelector;

use crate::api::{
    check_character_verification, claim_character, get_character_verifications, get_characters,
    search_characters, unclaim_character,
};
use crate::components::{loading::*, world_name::*, world_picker::*};
use crate::error::AppResult;
use crate::global_state::home_world::{
    get_homeworld, get_price_zone, result_to_selector_read, selector_to_setter_signal,
};

#[component]
fn AddCharacterMenu(
    cx: Scope,
    claim_character: Action<i32, AppResult<(i32, String)>>,
) -> impl IntoView {
    let (is_open, set_is_open) = create_signal(cx, false);
    let (character_search, set_character_search) = create_signal(cx, "".to_string());
    let search_action = create_action(cx, move |search: &String| {
        search_characters(cx, search.to_string())
    });

    view! {cx,
        <button class="btn" on:click=move |_| set_is_open(!is_open())><i class="fa-solid fa-plus"></i></button>
        {move || claim_character.value()().map(|result| {
            view!{cx, <div class="content-well">{
                match result {
                    Ok((_id, value)) => {
                        format!("Successfully started claim. Add {value} to your lodestone profile")
                    },
                    Err(e) => {
                        format!("Error adding character to your profile\n{e}")
                    }
                }
            }</div>}
        })}
        {move || is_open().then(||
            view!{cx, <div class="flex-column">
                    <label for="character-name">"Character:"</label>
                    <div class="flex-row">
                        <input prop:value=character_search id="character-name" on:input=move |input| set_character_search(event_target_value(&input)) />
                        <button class="btn" on:click=move |_| search_action.dispatch(character_search())>
                            <i class="fa-solid fa-magnifying-glass"></i>
                        </button>
                    </div>
                    {search_action.pending()().then(|| view!{cx, <Loading/>})}
                    {search_action.value()().map(|value| match value {
                        Ok(characters) => view!{cx, <div>
                                <span class="content-title">"Search Results"</span>
                                {characters.is_empty().then(|| {
                                    "No search results found"
                                })}
                                {characters.into_iter().map(|character| view!{cx,
                                    <div class="flex flex-row">
                                        <span style="width: 250px">{character.first_name}" "{character.last_name}</span>
                                        <span style="width: 150px"><WorldName id=AnySelector::World(character.world_id)/></span>
                                        <button class="btn" on:click=move |_| { set_is_open(false); claim_character.dispatch(character.id); }>"Claim"</button>
                                    </div>
                                }).collect::<Vec<_>>()}
                            </div>}.into_view(cx),
                        Err(e) => format!("Failed to load characters {e}").into_view(cx)
                    })}
                </div>
        })}
    }
}

#[component]
pub fn Profile(cx: Scope) -> impl IntoView {
    let claim_character = create_action(cx, move |id: &i32| claim_character(cx, *id));
    let unclaim_character = create_action(cx, move |id: &i32| unclaim_character(cx, *id));
    let check_verification =
        create_action(cx, move |id: &i32| check_character_verification(cx, *id));
    let characters = create_resource(
        cx,
        move || {
            (
                unclaim_character.version()(),
                check_verification.version()(),
            )
        },
        move |_| get_characters(cx),
    );
    let pending_verifications = create_resource(
        cx,
        move || (check_verification.version()(), claim_character.version()()),
        move |_| get_character_verifications(cx),
    );
    let (homeworld, set_homeworld) = get_homeworld(cx);
    let (price_region, set_price_region) = get_price_zone(cx);
    let price_region = result_to_selector_read(cx, price_region);
    let set_price_region = selector_to_setter_signal(cx, set_price_region);
    view! { cx,
        <div class="main-content">
            <span class="content-title">"Profile"</span>
            <div class="content-well">
                <label>"home world:"</label>
                <Suspense fallback=move || view!{cx, <Loading/>}>
                    <WorldOnlyPicker current_world=homeworld.into() set_current_world=set_homeworld.into()  />
                </Suspense>
                    <label>"Default price selector"</label>
                <Suspense fallback=move || view!{cx, <Loading />}>
                    <WorldPicker current_world=price_region.into() set_current_world=set_price_region.into() />
                </Suspense>
            </div>
            <div class="content-well">
                <span class="content-title">
                    "Characters"
                </span>
                <AddCharacterMenu claim_character/>
                <Suspense fallback=move || view!{cx, <Loading/>}>
                    {move || pending_verifications.read(cx).map(|verifications| {
                        match verifications {
                            Ok(verifications) => {
                                view!{cx, <div>
                                    {verifications.into_iter().map(|verification| {
                                        view!{cx, <div class="flex-row">
                                                <div class="flex-column">
                                                    {verification.character.first_name}" "{verification.character.last_name}
                                                    <br/>
                                                    "verification string:" {verification.verification_string}
                                                    <br/>
                                                    "Add the verification string to your lodestone profile and then click verify!"
                                                </div>
                                                <button class="btn" on:click=move |_| {
                                                    check_verification.dispatch(verification.id)
                                                }>"Verify"</button>
                                            </div>}
                                    }).collect::<Vec<_>>()}
                                </div>}.into_view(cx)
                            },
                            Err(e) => view!{cx, "Unable to fetch verifications"<br/>{e.to_string()}}.into_view(cx)
                        }
                    })}
                </Suspense>
                <Suspense fallback=move || view!{cx, <Loading/>}>
                    {move || characters.read(cx).map(|characters| {
                        match characters {
                            Ok(characters) => {
                                if characters.is_empty() {
                                    view!{cx, "No characters. Add a character to get started."}.into_view(cx)
                                } else {
                                    view!{cx, <div>
                                        {characters.into_iter().map(|character| {
                                            view!{cx,
                                                <div class="flex flex-row">
                                                    <span style="width: 250px;">{character.first_name}" "{character.last_name}</span>
                                                    <span style="width: 150px"><WorldName id=AnySelector::World(character.world_id)/></span>
                                                    <button class="btn" on:click=move |_| unclaim_character.dispatch(character.id)><span class="fa-solid fa-trash"></span></button>
                                                </div>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>}.into_view(cx)
                                }
                            },
                            Err(e) => {
                                view!{cx, "unable to get characters"<br/>{e.to_string()}}.into_view(cx)
                            }
                        }
                    })}
                </Suspense>
            </div>
        </div>}
}
