use std::sync::Arc;

use axum::extract::State;
use maud::html;
use ultros_db::{
    entity::{final_fantasy_character, owned_ffxiv_character},
    retainers::FullRetainersList,
    UltrosDb,
};

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
    characters: Vec<(
        owned_ffxiv_character::Model,
        Option<final_fantasy_character::Model>,
    )>,
    world_cache: Arc<WorldCache>,
}

pub(crate) async fn edit_retainer(
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
    user: AuthDiscordUser,
) -> Result<RenderPage<EditRetainers>, WebError> {
    let characters = db
        .get_all_characters_for_discord_user(user.id as i64)
        .await?;
    let retainers = db.get_all_owned_retainers_and_character(user.id).await?;
    Ok(RenderPage(EditRetainers {
        user: Some(user),
        retainers,
        characters,
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
                    a href="/retainers/add" class="btn-secondary" {
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
                                    tr {
                                        th {
                                            "retainer name"
                                        }
                                        th {
                                            "world"
                                        }
                                        th {
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
                                                div class="tooltip" {
                                                    "order: " ((owned_data.weight.unwrap_or_default()))
                                                    div class="tooltip-text" {
                                                        "Retainers will be sorted by this #."
                                                    }
                                                }
                                                div class="tooltip" {
                                                    a class="btn" href={"/retainers/upsort/" ((owned_data.id))} {
                                                        i class="fa fa-arrow-up" {
                                                        }
                                                    }
                                                    div class="tooltip-text" {
                                                        "Increase retainer sort order"
                                                    }
                                                }
                                                div class="tooltip" {
                                                    a class="btn" href={"/retainers/downsort/" ((owned_data.id))} {
                                                        i class="fa fa-arrow-down" {
                                                        }
                                                    }
                                                    div class="tooltip-text" {
                                                        "Decrease retainer sort order"
                                                    }
                                                }
                                                div class="tooltip" {
                                                    a class="btn" href={"/retainers/remove/" ((owned_data.id))} {
                                                        i class="fa fa-trash" {}
                                                    }
                                                    div class="tooltip-text" {
                                                        "Remove retainer"
                                                    }
                                                }
                                                @if character.is_none() && !self.characters.is_empty() {
                                                    div class="dropdown" {
                                                        span class="btn" {"Add Character"}
                                                        div class="dropdown-content" {
                                                            @for (_, c) in &self.characters {
                                                                @if let Some(c) = c {
                                                                    a href={"/retainers/character/add/"((owned_data.id))"/"((c.id))} {
                                                                        ((c.first_name))" "((c.last_name))
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                } @else {
                                                    div class="tooltip" {
                                                        a class="btn" href={"/retainers/character/remove/" ((owned_data.id)) } {
                                                            i class="fa fa-person" {}
                                                        }
                                                        div class="tooltip-text" {
                                                            "Remove character"
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
    }
}
