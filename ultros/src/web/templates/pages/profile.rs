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

    let (challenges, characters) = if let Some(discord_user) = &user {
        let (challenges, characters) = futures::future::join(
            db.get_pending_character_challenges_for_discord_user(discord_user.id as i64),
            db.get_all_characters_for_discord_user(discord_user.id as i64),
        )
        .await;
        (challenges?, characters?)
    } else {
        (vec![], vec![])
    };
    Ok((
        cookie_jar,
        RenderPage(Profile {
            user,
            home_world,
            world_cache,
            challenges,
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
    challenges: Vec<(
        ultros_db::entity::ffxiv_character_verification::Model,
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
            challenges,
        } = self;
        html! {
            ((Header { user: user.as_ref() }))
            div class="container" {
                div class="main-content flex flex-column" {
                    h1 {
                        "Profile"
                    }
                    div class="flex flex-wrap" {
                        @if !challenges.is_empty() {
                            div class="content-well" {
                                @for (challenge, character) in &self.challenges {
                                    @if let Some(character) = character {
                                        span {
                                            ((character.first_name))" "((character.last_name))
                                        }
                                    }
                                    a href={"/characters/verify/" ((challenge.id)) } {
                                        "Verify"
                                    }
                                }
                            }
                        }
                        div class="content-well flex flex-column" {
                            div class="flex flex-row" {
                                span class="content-title" {
                                    "Characters"
                                }
                                a class="btn" href="/characters/add" {
                                    "Add"
                                }
                            }
                            @for (owned, character) in characters {
                                @if let Some(character) = character {
                                    div class="flex-row" {
                                        span class="content-title" {
                                            ((character.first_name))" "((character.last_name))
                                        }
                                        span class="content-title" {
                                            @if let Ok(world) = world_cache.lookup_selector(&AnySelector::World(character.id)) {
                                                ((world.get_name()))
                                            }
                                        }
                                        a class="btn btn-secondary" href={ "/characters/refresh/" ((character.id)) } {
                                            span class="fa fa-refresh" {}
                                        }
                                        a class="btn btn-secondary" {
                                            span class="fa fa-trash" href={ "/characters/unclaim/" ((owned.ffxiv_character_id))} {}
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
                                br {}
                                ((WorldDropdown { world_id: home_world.map(|h| h.home_world), world_cache }))
                                input type="submit" value="Update";
                            }
                        }
                    }
                }
            }
        }
    }
}
