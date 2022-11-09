use std::{str::FromStr, sync::Arc};

use axum::extract::{Query, State};
use lodestone::model::server::Server;
use maud::html;
use serde::Deserialize;

use crate::{
    web::{
        error::WebError,
        oauth::AuthDiscordUser,
        templates::{
            components::{header::Header, world_dropdown::WorldDropdown},
            page::{Page, RenderPage},
        }, character_verifier_service::{self, CharacterVerifierService},
    },
    world_cache::WorldCache,
};

#[derive(Deserialize)]
pub(crate) struct CharacterQueryParameters {
    name: Option<String>,
    world: Option<i32>,
}

pub(crate) async fn add_character(
    user: AuthDiscordUser,
    State(world_cache): State<Arc<WorldCache>>,
    Query(query): Query<CharacterQueryParameters>,
) -> Result<RenderPage<AddCharacter>, WebError> {
    let search_results = if let Some(name) = &query.name {
        let mut builder = lodestone::search::SearchBuilder::new().character(&name);
        if let Some(world) = query.world {
            let world =
                world_cache.lookup_selector(&crate::world_cache::AnySelector::World(world))?;
            let world_name = world.get_name();
            builder = builder.server(Server::from_str(world_name)?);
        }
        let client = reqwest::Client::new();
        Some(builder.send_async(&client).await?)
    } else {
        None
    };

    Ok(RenderPage(AddCharacter {
        search_results,
        world_cache,
        user: Some(user),
        query
    }))
}

pub(crate) async fn claim_character(State(verification_service): State<CharacterVerifierService>) {
    
}

pub(crate) struct AddCharacter {
    search_results: Option<Vec<lodestone::search::ProfileSearchResult>>,
    world_cache: Arc<WorldCache>,
    user: Option<AuthDiscordUser>,
    query: CharacterQueryParameters
}

impl Page for AddCharacter {
    fn get_name(&self) -> &str {
        "Add Character"
    }

    fn draw_body(&self) -> maud::Markup {
        html! {
            ((Header {
                user: self.user.as_ref()
              }))
            div class="container" {
                div class="main-content" {
                    div class="content-well" {
                        span class="content-title" {
                            div class="content-well" {
                                form {
                                    label for="name" {
                                        "character name"
                                    }
                                    input name="name" id="name" type="text" value={
                                        @if let Some(name) = &self.query.name {
                                            ((name))
                                        }
                                    } {}
                                    ((WorldDropdown { world_id: self.query.world, world_cache: &self.world_cache }))
                                    input type="submit" value="Search" {}
                                }
                            }
                            @if let Some(results) = &self.search_results {

                                div class="content-well" {
                                    @for character in results {
                                        div class="flex flex-column" {
                                            span { ((character.name)) }
                                            span { ((character.world)) }
                                        }
                                        a class="btn" href={"/character/claim/" ((character.user_id))} {
                                            "Claim"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
