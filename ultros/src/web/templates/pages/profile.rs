use std::sync::Arc;

use axum::extract::{Query, State};
use axum_extra::extract::{cookie::Cookie, CookieJar};
use maud::html;
use serde::Deserialize;

use crate::{
    web::{
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
    Query(ProfileQueryOptions { world }): Query<ProfileQueryOptions>,
) -> (CookieJar, RenderPage<Profile>) {
    if let Some(world) = world {
        let mut cookie = Cookie::new(home_world_cookie::HOME_WORLD_COOKIE, world.to_string());
        cookie.make_permanent();
        cookie_jar = cookie_jar.add(cookie);
        home_world = Some(HomeWorld { home_world: world })
    }
    (
        cookie_jar,
        RenderPage(Profile {
            user,
            home_world,
            world_cache,
        }),
    )
}

pub(crate) struct Profile {
    user: Option<AuthDiscordUser>,
    home_world: Option<HomeWorld>,
    world_cache: Arc<WorldCache>,
}

impl Page for Profile {
    fn get_name<'a>(&'a self) -> &'a str {
        "Profile"
    }

    fn draw_body(&self) -> maud::Markup {
        let Self {
            user,
            home_world,
            world_cache,
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
                    form action="/profile" {
                        ((WorldDropdown { world_id: home_world.map(|h| h.home_world), world_cache }))
                        input type="submit" value="Update";
                    }
                }
            }
        }
    }
}
