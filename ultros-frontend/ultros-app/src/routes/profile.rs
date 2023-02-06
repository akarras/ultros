use leptos::*;

use crate::api::{claim_character, get_characters, search_characters};
use crate::components::loading::*;

#[component]
fn AddCharacterMenu(cx: Scope, claim_character: Action<i32, Option<()>>) -> impl IntoView {
    let (is_open, set_is_open) = create_signal(cx, false);
    let (character_search, set_character_search) = create_signal(cx, "".to_string());
    let search_action = create_action(cx, move |search: &String| {
        search_characters(cx, search.to_string())
    });

    view! {cx,
        <button class="btn" on:click=move |_| set_is_open(!is_open())><i class="fa-solid fa-plus"></i></button>
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
                        Some(characters) => view!{cx, <div>
                                <span class="content-title">"Search Results"</span>
                                {characters.is_empty().then(|| {
                                    "No search results found"
                                })}
                                {characters.into_iter().map(|character| view!{cx,
                                    <div>
                                        {character.first_name} {character.last_name}
                                        <button class="btn" on:click=move |_| claim_character.dispatch(character.id)>"Claim"</button>
                                    </div>
                                }).collect::<Vec<_>>()}
                            </div>}.into_view(cx),
                        None => view!{cx, "Failed to load characters"}.into_view(cx)
                    })}
                </div>
        })}
    }
}

#[component]
pub fn Profile(cx: Scope) -> impl IntoView {
    let characters = create_resource(cx, move || {}, move |_| get_characters(cx));
    let claim_character = create_action(cx, move |id: &i32| claim_character(cx, *id));

    view! { cx, <div class="container">
        <div class="main-content">
            <span class="content-title">"Profile"</span>
            <div class="content-well">
                <span class="content-title">
                    "Characters"
                </span>
                <AddCharacterMenu claim_character/>
                <Suspense fallback=move || view!{cx, <Loading/>}>
                    {move || characters().map(|characters| {
                        match characters {
                            Some(characters) => {
                                if characters.is_empty() {
                                    view!{cx, "No characters. Add a character to get started."}.into_view(cx)
                                } else {
                                    view!{cx, <div>
                                        {characters.into_iter().map(|character| {
                                            view!{cx,
                                                <div>
                                                    {character.first_name}" "{character.last_name}
                                                    <button class="btn"><span class="fa-solid fa-trash"></span></button>
                                                </div>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>}.into_view(cx)
                                }
                            },
                            None => {
                                view!{cx, "unable to get characters"}.into_view(cx)
                            }
                        }
                    })}
                </Suspense>
            </div>
        </div>
    </div>}
}
