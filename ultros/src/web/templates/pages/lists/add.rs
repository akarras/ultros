use std::sync::Arc;

use axum::extract::State;
use maud::html;
use ultros_db::{world_cache::{WorldCache}, UltrosDb};

use crate::web::{oauth::AuthDiscordUser, templates::{page::{Page, RenderPage}, components::{header::Header, world_dropdown::WorldDropdown}}};

pub(crate) async fn add_list(user: AuthDiscordUser, State(world_cache): State<Arc<WorldCache>>) -> RenderPage<AddListPage> {
    RenderPage(AddListPage { user, world_cache })
}


pub(crate) struct AddListPage {
    user: AuthDiscordUser,
    world_cache: Arc<WorldCache>
}

impl Page for AddListPage {
    fn get_name(&'_ self) -> String {
        "Add".to_string()
    }

    fn draw_body(&self) -> maud::Markup {
        html! {
            ((Header {
                user: Some(&self.user)
            }))
            div class="container" {
                div class="main-content" {
                    form {
                        label for="list" {"List Name:"}
                        input type="text" id="list" name="list"{}
                        label {"World type:"}
                        ((WorldDropdown{ world_id: None, world_cache: &self.world_cache }))
                        input type="submit" value="create list"{}
                    }
                }
            }
        }
    }
}

