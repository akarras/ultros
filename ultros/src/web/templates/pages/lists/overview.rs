use axum::extract::State;
use maud::html;
use ultros_db::{entity::list, UltrosDb};

use crate::web::{oauth::AuthDiscordUser, templates::{page::{Page, RenderPage}, components::header::Header}, error::WebError};

pub(crate) async fn overview(user: AuthDiscordUser, State(db): State<UltrosDb>) -> Result<RenderPage<ListsOverview>, WebError> {
    let lists = db.get_lists_for_user(user.id as i64).await?;
    Ok(RenderPage(ListsOverview { user, lists }))
}

pub(crate) struct ListsOverview {
    user: AuthDiscordUser,
    lists: Vec<list::Model>
}

impl Page for ListsOverview {
    fn get_name(&'_ self) -> String {
        "Lists".to_string()
    }

    fn draw_body(&self) -> maud::Markup {
        html!{
            ((Header {
                user: Some(&self.user)
            }))
            div class="container" {
                div class="main-content" {
                    h2{ "Lists" }
                    @if self.lists.is_empty() {
                        "No lists. Get started by adding a list " a href="/list/add" {"add"}
                    }
                    ul {
                        @for list in &self.lists {
                            a href={"/list/" ((list.id))} {
                                ((list.name))
                            }
                        }
                    }
                }
            }
        }
    }
}