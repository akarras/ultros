use std::sync::Arc;

use axum::extract::State;
use maud::html;
use ultros_db::{entity::list, UltrosDb, world_cache::{WorldCache, AnySelector}};

use crate::web::{
    error::WebError,
    oauth::AuthDiscordUser,
    templates::{
        components::header::Header,
        page::{Page, RenderPage},
    },
};

pub(crate) async fn overview(
    user: AuthDiscordUser,
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
) -> Result<RenderPage<ListsOverview>, WebError> {
    let lists = db.get_lists_for_user(user.id as i64).await?;
    Ok(RenderPage(ListsOverview { user, lists, world_cache }))
}

pub(crate) struct ListsOverview {
    user: AuthDiscordUser,
    lists: Vec<list::Model>,
    world_cache: Arc<WorldCache>
}

impl Page for ListsOverview {
    fn get_name(&'_ self) -> String {
        "Lists".to_string()
    }

    fn draw_body(&self) -> maud::Markup {
        html! {
            ((Header {
                user: Some(&self.user)
            }))
            div class="container" {
                div class="content-nav nav" {
                    a class="btn-secondary" href="/list/add" {
                        span class="fa-solid fa-plus" {
                        }
                        "Create List"
                    }
                }
                div class="main-content" {
                    h2{ "Lists" }
                    @if self.lists.is_empty() {
                        "No lists. Get started by adding a list " a href="/list/add" {"add"}
                    }
                    table {
                        thead {
                            tr {
                                th {
                                    "List name"
                                }
                                th {
                                    "World name"
                                }
                                th {

                                }
                            }
                        }
                        tbody {
                            @for list in &self.lists {
                                tr {
                                    td{
                                        a href={"/list/" ((list.id))} {
                                            ((list.name))
                                        }
                                    }
                                    td {
                                        @if let Ok(selector) = AnySelector::try_from(list) {
                                            @if let Ok(result) = self.world_cache.lookup_selector(&selector) {
                                                ((result.get_name()))
                                            }
                                        }
                                    }
                                    td {
                                        div class="tooltip" {
                                            span class="tooltip-text" {
                                                "delete list"
                                            }
                                            a class="btn" href={"/list/" ((list.id)) "/delete"} {
                                                span class="fa-solid fa-trash" {
                                                    
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
    }
}
