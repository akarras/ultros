use std::sync::Arc;

use axum::extract::{Query, State};
use axum_extra::extract::{cookie::Cookie, CookieJar};
use maud::html;
use serde::Deserialize;
use ultros_db::{
    entity::{final_fantasy_character, owned_ffxiv_character},
    UltrosDb,
};

use crate::{
    web::{
        error::WebError,
        home_world_cookie::{self, HomeWorld},
        oauth::AuthDiscordUser,
        templates::{
            components::{header::Header, world_dropdown::WorldDropdown},
            page::{Page, RenderPage},
        },
    },
    world_cache::{AnySelector, WorldCache},
};

#[derive(Deserialize)]
pub(crate) struct ProfileQueryOptions {
    world: Option<i32>,
}

pub(crate) async fn profile(
    user: Option<AuthDiscordUser>,
    mut home_world: Option<HomeWorld>,
    mut cookie_jar: CookieJar,
    State(world_cache): State<Arc<WorldCache>>,
    State(db): State<UltrosDb>,
    Query(ProfileQueryOptions { world }): Query<ProfileQueryOptions>,
) -> Result<(CookieJar, RenderPage<Profile>), WebError> {
    if let Some(world) = world {
        let mut cookie = Cookie::new(home_world_cookie::HOME_WORLD_COOKIE, world.to_string());
        cookie.make_permanent();
        cookie_jar = cookie_jar.add(cookie);
        home_world = Some(HomeWorld { home_world: world })
    }

    let characters = if let Some(discord_user) = &user {
        db.get_all_characters_for_discord_user(discord_user.id as i64)
            .await?
    } else {
        vec![]
    };
    Ok((
        cookie_jar,
        RenderPage(Profile {
            user,
            home_world,
            world_cache,
            characters,
        }),
    ))
}

pub(crate) struct Profile {
    user: Option<AuthDiscordUser>,
    home_world: Option<HomeWorld>,
    world_cache: Arc<WorldCache>,
    characters: Vec<(
        owned_ffxiv_character::Model,
        Option<final_fantasy_character::Model>,
    )>,
}

impl Page for Profile {
    fn get_name(&'_ self) -> &'_ str {
        "Profile"
    }

    fn draw_body(&self) -> maud::Markup {
        let Self {
            user,
            home_world,
            world_cache,
            characters,
        } = self;
        html! {
            ((Header { user: user.as_ref() }))
            div class="container" {
                div class="main-content" {
                    h1 {
                        "Profile"
                    }
                    label {
                        "Home word:"
                    }
                    div class="content-well" {
                        span class="content-title" {
                            "Characters"
                        }
                        a class="btn" href="/characters/add" {
                            "Add"
                        }
                        @for (owned, character) in characters {
                            div class="content-well" {
                                @if let Some(character) = character {
                                    span class="content-title" {
                                        ((character.first_name)) ((character.last_name))
                                    }
                                    span class="content-title" {
                                        @if let Ok(world) = world_cache.lookup_selector(&AnySelector::World(character.id)) {
                                            ((world.get_name()))
                                        }
                                    }
                                    a class="btn btn-secondary" href={ "/character/refresh/" ((character.id)) } {
                                        span class="fa fa-refresh" {}
                                    }
                                    a classs="btn btn-danger" {
                                        span class="fa fa-trash" href={ "/character/unclaim/" ((owned.ffxiv_character_id))} {}
                                    }
                                }
                            }
                        }
                    }
                    div class="content-well" {
                        form action="/profile" {
                            span class="content-well" {
                                "Home world"
                            }
                            ((WorldDropdown { world_id: home_world.map(|h| h.home_world), world_cache }))
                            input type="submit" value="Update";
                        }
                    }
                }
            }
        }
    }
}
