use std::sync::Arc;

use axum::extract::State;
use maud::html;
use ultros_db::{retainers::FullRetainersList, UltrosDb};

use crate::{
    web::{
        error::WebError,
        oauth::AuthDiscordUser,
        templates::{
            components::header::Header,
            page::{Page, RenderPage},
        },
    },
    world_cache::{AnySelector, WorldCache},
};

pub(crate) struct EditRetainers {
    user: Option<AuthDiscordUser>,
    retainers: FullRetainersList,
    world_cache: Arc<WorldCache>,
}

pub(crate) async fn edit_retainer(
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    user: AuthDiscordUser,
) -> Result<RenderPage<EditRetainers>, WebError> {
    let retainers = db.get_all_owned_retainers_and_character(user.id).await?;
    Ok(RenderPage(EditRetainers {
        user: Some(user),
        retainers,
        world_cache,
    }))
}

impl Page for EditRetainers {
    fn get_name(&'_ self) -> &'_ str {
        "Edit retainers"
    }

    fn draw_body(&self) -> maud::Markup {
        html! {
            ((Header { user: self.user.as_ref() }))
            div class="container" {
                div class="content-nav nav" {
                    a href="/retainers/Add" class="btn-secondary" {
                        i class="fa-solid fa-pen-to-square" {}
                        "Add"
                    }
                    a class="btn-secondary listings" href="/retainers/listings" {
                        i class="fa-solid fa-sack-dollar" {}
                        "Listings"
                    }
                    a class="btn-secondary undercuts" href="/retainers/undercuts" {
                        i class="fa-solid fa-exclamation" {}
                        "Undercuts"
                    };
                }
                div class="main-content" {
                    @for (character, retainers) in &self.retainers {
                        div class="content-well" {
                            span class="content-title" {
                                ((character.as_ref().map(|c| format!("{} {}", c.first_name, c.last_name))).unwrap_or_else(|| "No Character".to_string()))
                                table {
                                    th {
                                        td {
                                            "Retainer Name"
                                        }
                                        td {
                                            "world"
                                        }
                                        td {
                                            "retainer city"
                                        }
                                        td {
                                            "sort order"
                                        }
                                        td {
                                            ""
                                        }
                                    }
                                    @for (owned_data, retainer) in retainers {
                                        tr {
                                            td {
                                                ((retainer.name))
                                            }
                                            td {
                                                ((self.world_cache.lookup_selector(&AnySelector::World(retainer.world_id)).as_ref().map(|world| world.get_name()).unwrap_or_default()))
                                            }
                                            td {
                                                ((retainer.retainer_city_id))
                                            }
                                            td {
                                                ((owned_data.weight.map(|w| w.to_string()).unwrap_or_default()))
                                            }
                                            td {
                                                a class="btn align-right" href={"/retainers/remove/" ((owned_data.id))} {
                                                    "Remove"
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
