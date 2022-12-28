use std::sync::Arc;

use axum::extract::{State, Query};
use maud::html;
use serde::Deserialize;
use ultros_db::{world_cache::WorldCache, UltrosDb};

use crate::web::{
    oauth::AuthDiscordUser,
    templates::{
        components::{header::Header, world_dropdown::WorldDropdown},
        page::{Page, RenderPage},
    }, error::WebError,
};

#[derive(Deserialize)]
pub(crate) struct AddListQueryItems {
    list: String,
    world: i32
}

pub(crate) async fn add_list(
    auth_user: AuthDiscordUser,
    State(world_cache): State<Arc<WorldCache>>,
    query: Option<Query<AddListQueryItems>>,
    State(db): State<UltrosDb>
) -> Result<RenderPage<AddListPage>, WebError> {
    if let Some(Query(AddListQueryItems{ list, world })) = &query {
        let user = db.get_or_create_discord_user(auth_user.id, auth_user.name.clone()).await?;
        db.create_list(user, list.clone(), ultros_db::world_cache::AnySelector::World(*world)).await?;
    }
    Ok(RenderPage(AddListPage { user: auth_user, world_cache, success: query.map(|Query(q)| q.list) }))
}

pub(crate) struct AddListPage {
    success: Option<String>,
    user: AuthDiscordUser,
    world_cache: Arc<WorldCache>,
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
                    @if let Some(name) = &self.success {
                        div class="content-well" {
                            "successfully created list " ((name))
                        }
                    }
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
