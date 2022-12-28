use std::sync::Arc;

use axum::{extract::{Path, State}, response::Redirect};
use maud::html;
use ultros_db::{
    entity::{active_listing, list, list_item, retainer},
    world_cache::{WorldCache, AnySelector},
    UltrosDb,
};
use xiv_gen::ItemId;

use crate::web::{
    error::WebError,
    oauth::AuthDiscordUser,
    templates::{
        components::{header::Header, item_icon::{ItemIcon, IconSize}},
        page::{Page, RenderPage},
    },
};

pub(crate) async fn list_details(
    user: AuthDiscordUser,
    Path(id): Path<i32>,
    State(db): State<UltrosDb>,
    State(world_cache): State<Arc<WorldCache>>,
) -> Result<RenderPage<ListView>, WebError> {
    let list = db.get_list(id, user.id as i64).await?;
    let mut list_items = db
        .get_listings_for_list(user.id as i64, id, &world_cache)
        .await?;
    list_items
        .iter_mut()
        .for_each(|(_, listings)| listings.sort_by_key(|(l, _)| l.price_per_unit));
    Ok(RenderPage(ListView {
        user,
        list,
        list_items,
        world_cache,
    }))
}

pub(crate) async fn delete_item(user: AuthDiscordUser, State(db): State<UltrosDb>, Path(id): Path<i32>) -> Result<Redirect, WebError> {
    let item = db.remove_item_from_list(user.id as i64, id).await?;
    Ok(Redirect::to(&format!("/list/{}", item.list_id)))
}

pub(crate) struct ListView {
    user: AuthDiscordUser,
    list: list::Model,
    list_items: Vec<(
        list_item::Model,
        Vec<(active_listing::Model, Option<retainer::Model>)>,
    )>,
    world_cache: Arc<WorldCache>
}

impl Page for ListView {
    fn get_name(&self) -> String {
        self.list.name.clone()
    }

    fn draw_body(&self) -> maud::Markup {
        let items = &xiv_gen_db::decompress_data().items;
        html! {
            ((Header {
                user: Some(&self.user)
            }))
            div class="container" {
                div class="content-nav nav" {
                    a class="btn-secondary" href={"/list/" ((self.list.id)) "/item/add"} {
                        span class="fa-solid fa-plus" {

                        }
                        "Add items"
                    }
                }
                div class="main-content" {
                    span class="content-title" {
                        ((self.list.name))
                    }
                    table {
                        thead {
                            th {
                                "Item Name"
                            }
                            th {
                                "Price Per Item"
                            }
                            th {
                                "World"
                            }
                            th {}
                        }
                        @for (item, listings) in &self.list_items {
                            tr {
                                @if let Some((listing, _retainer)) = listings.first() {
                                    @if let Ok(world) = self.world_cache.lookup_selector(&AnySelector::World(listing.world_id)) {
                                        td {
                                            a href={"/listings/"((world.get_name()))"/"((item.item_id))} {
                                                ((ItemIcon { item_id: item.item_id, icon_size: IconSize::Small }))
                                                ((items.get(&ItemId(item.item_id)).map(|item| item.name.as_str()).unwrap_or_default()))
                                            }
                                        }
                                        td {
                                            ((listing.price_per_unit))
                                        }
                                        td {
                                            ((world.get_name()))
                                        }
                                        td {
                                            a class="btn" href={"/list/edit/item/delete/"((item.id))} {
                                                div class="tooltip" {
                                                    span class="fa-solid fa-trash" {
                                                        
                                                    }
                                                    span class="tooltip-text" {"Delete this item from the list"}
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
