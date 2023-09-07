use leptos::*;
use leptos_icons::*;
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
fn AddCharacterMenu(claim_character: Action<i32, AppResult<(i32, String)>>) -> impl IntoView {
    let (is_open, set_is_open) = create_signal(false);
    let (character_search, set_character_search) = create_signal("".to_string());
    let search_action = create_action(move |search: &String| search_characters(search.to_string()));

    view! {
        <button class="btn" on:click=move |_| set_is_open(!is_open())><Icon icon=Icon::from(BiIcon::BiPlusRegular)/></button>
        {move || claim_character.value()().map(|result| {
            view!{<div class="content-well">{
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
            view!{<div class="flex-column">
                    <label for="character-name">"Character:"</label>
                    <div class="flex-row">
                        <input prop:value=character_search id="character-name" on:input=move |input| set_character_search(event_target_value(&input)) />
                        <button class="btn" on:click=move |_| search_action.dispatch(character_search())>
                            <Icon icon=Icon::from(AiIcon::AiSearchOutlined) />
                        </button>
                    </div>
                    {search_action.pending()().then(|| view!{<Loading/>})}
                    {search_action.value()().map(|value| match value {
                        Ok(characters) => view!{<div>
                                <span class="content-title">"Search Results"</span>
                                {characters.is_empty().then_some({
                                    "No search results found"
                                })}
                                {characters.into_iter().map(|character| view!{
                                    <div class="flex flex-row">
                                        <span style="width: 250px">{character.first_name}" "{character.last_name}</span>
                                        <span style="width: 150px"><WorldName id=AnySelector::World(character.world_id)/></span>
                                        <button class="btn" on:click=move |_| { set_is_open(false); claim_character.dispatch(character.id); }>"Claim"</button>
                                    </div>
                                }).collect::<Vec<_>>()}
                            </div>}.into_view(),
                        Err(e) => format!("Failed to load characters {e}").into_view()
                    })}
                </div>
        })}
    }
}

#[component]
pub fn Settings() -> impl IntoView {
    let (homeworld, set_homeworld) = get_homeworld();
    let (price_region, set_price_region) = get_price_zone();
    let price_region = result_to_selector_read(price_region);
    let set_price_region = selector_to_setter_signal(set_price_region);
    view! {
    <div class="main-content">
        <span class="content-title">"Settings"</span>
        <div class="content-well">
            <label>"home world:"</label>
            <Suspense fallback=move || view!{<Loading/>}>
                <WorldOnlyPicker current_world=homeworld set_current_world=set_homeworld  />
            </Suspense>
                <label>"Default price selector"</label>
            <Suspense fallback=move || view!{<Loading />}>
                <WorldPicker current_world=price_region set_current_world=set_price_region />
            </Suspense>
        </div>
    </div>}
}

#[component]
pub fn Profile() -> impl IntoView {
    let claim_character = create_action(move |id: &i32| claim_character(*id));
    let unclaim_character = create_action(move |id: &i32| unclaim_character(*id));
    let check_verification = create_action(move |id: &i32| check_character_verification(*id));
    let characters = create_resource(
        move || {
            (
                unclaim_character.version()(),
                check_verification.version()(),
            )
        },
        move |_| get_characters(),
    );
    let pending_verifications = create_resource(
        move || (check_verification.version()(), claim_character.version()()),
        move |_| get_character_verifications(),
    );
    let (homeworld, set_homeworld) = get_homeworld();
    let (price_region, set_price_region) = get_price_zone();
    let price_region = result_to_selector_read(price_region);
    let set_price_region = selector_to_setter_signal(set_price_region);
    view! {
    <div class="main-content">
        <span class="content-title">"Settings"</span>
        <div class="content-well">
            <label>"home world:"</label>
            <Suspense fallback=move || view!{<Loading/>}>
                <WorldOnlyPicker current_world=homeworld set_current_world=set_homeworld  />
            </Suspense>
                <label>"Default price selector"</label>
            <Suspense fallback=move || view!{<Loading />}>
                <WorldPicker current_world=price_region set_current_world=set_price_region />
            </Suspense>
        </div>
        <div class="content-well">
            <span class="content-title">
                "Characters"
            </span>
            <AddCharacterMenu claim_character/>
            <Suspense fallback=move || view!{<Loading/>}>
                {move || pending_verifications.get().map(|verifications| {
                    match verifications {
                        Ok(verifications) => {
                            view!{<div>
                                {verifications.into_iter().map(|verification| {
                                    view!{<div class="flex-row">
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
                            </div>}.into_view()
                        },
                        Err(e) => view!{"Unable to fetch verifications"<br/>{e.to_string()}}.into_view()
                    }
                })}
            </Suspense>
            <Suspense fallback=move || view!{<Loading/>}>
                {move || characters.get().map(|characters| {
                    match characters {
                        Ok(characters) => {
                            if characters.is_empty() {
                                view!{"No characters. Add a character to get started."}.into_view()
                            } else {
                                view!{<div>
                                    {characters.into_iter().map(|character| {
                                        view!{
                                            <div class="flex flex-row">
                                                <span style="width: 250px;">{character.first_name}" "{character.last_name}</span>
                                                <span style="width: 150px"><WorldName id=AnySelector::World(character.world_id)/></span>
                                                <button class="btn" on:click=move |_| unclaim_character.dispatch(character.id)><Icon icon=Icon::from(BiIcon::BiTrashSolid) /></button>
                                            </div>
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>}.into_view()
                            }
                        },
                        Err(e) => {
                            view!{"unable to get characters"<br/>{e.to_string()}}.into_view()
                        }
                    }
                })}
            </Suspense>
        </div>
    </div>}
}
