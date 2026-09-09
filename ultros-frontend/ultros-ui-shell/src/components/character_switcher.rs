//! Home-world switcher driven by the user's claimed characters.
//!
//! Every character already carries its home world (the Lodestone tells us when
//! the character is claimed), and most sidebar tools are world-scoped through
//! the `HOME_WORLD` cookie. So switching characters in game gets a one-click
//! equivalent here: pick the character you're playing and every world-aware
//! route follows.
//!
//! Rendered inside the signed-in branch of the account drop-up, which is what
//! keeps the character request from firing for anonymous visitors.

use crate::api::get_characters;
use crate::components::world_name::WorldName;
use crate::global_state::LocalWorldData;
use crate::global_state::home_world::use_home_world;
use crate::i18n::{t, use_i18n};
use leptos::prelude::*;
use ultros_api_types::world_helper::AnySelector;

#[component]
pub fn CharacterSwitcher() -> impl IntoView {
    let i18n = use_i18n();
    let (homeworld, set_homeworld) = use_home_world();
    let characters = Resource::new(|| (), |_| get_characters());

    view! {
        <Suspense fallback=|| ()>
            {move || {
                // A logged-out or failed request is indistinguishable from
                // "no characters" here — both mean there is nothing to switch
                // between, and the account menu is not the place to surface a
                // fetch error.
                let characters = characters.get().and_then(|c| c.ok()).unwrap_or_default();
                (!characters.is_empty())
                    .then(|| {
                        view! {
                            <div class="side-nav-section-header">{t!(i18n, characters)}</div>
                            <div class="menu-accordion">
                                {characters
                                    .into_iter()
                                    .map(|character| {
                                        let world_id = character.world_id;
                                        let selected = move || {
                                            homeworld.get().is_some_and(|w| w.id == world_id)
                                        };
                                        view! {
                                            <button
                                                // The whole class string is rebuilt rather than
                                                // toggling one name, so deselecting actually drops
                                                // the highlight.
                                                class=move || {
                                                    if selected() {
                                                        "menu-item menu-item-selected"
                                                    } else {
                                                        "menu-item"
                                                    }
                                                }
                                                aria-pressed=move || selected().to_string()
                                                on:click=move |_| {
                                                    let world = use_context::<LocalWorldData>()
                                                        .and_then(|w| w.0.ok())
                                                        .and_then(|worlds| {
                                                            worlds
                                                                .lookup_selector(AnySelector::World(world_id))
                                                                .and_then(|w| w.as_world().cloned())
                                                        });
                                                    if world.is_some() {
                                                        set_homeworld(world);
                                                    }
                                                }
                                            >
                                                <span class="truncate">
                                                    {character.first_name} " " {character.last_name}
                                                </span>
                                                <span class="menu-item-trailing">
                                                    <WorldName id=AnySelector::World(world_id) />
                                                </span>
                                            </button>
                                        }
                                    })
                                    .collect::<Vec<_>>()}
                            </div>
                            <div class="menu-divider"></div>
                        }
                    })
            }}
        </Suspense>
    }
    .into_any()
}
