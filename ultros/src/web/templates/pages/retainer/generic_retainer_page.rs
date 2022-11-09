use maud::html;
use ultros_db::entity::active_listing;

use crate::{
    utils,
    web::{
        oauth::AuthDiscordUser,
        templates::{
            components::{
                header::Header,
                item_icon::{IconSize, ItemIcon},
            },
            page::Page,
        },
    },
};

pub(crate) struct GenericRetainerPage {
    pub(crate) retainer_name: String,
    pub(crate) retainer_id: i32,
    pub(crate) world_name: String,
    pub(crate) listings: Vec<active_listing::Model>,
    pub(crate) user: Option<AuthDiscordUser>,
}

impl Page for GenericRetainerPage {
    fn get_name(&'_ self) -> &'_ str {
        &self.retainer_name
    }

    fn draw_body(&self) -> maud::Markup {
        html! {
            ((Header { user: self.user.as_ref() }))

            div class="container" {
                div class="content-nav nav" {
                  a class="btn-secondary" href={"/retainers/add/" ((self.retainer_id)) } {
                    "Add Retainer"
                  }
                }
                div class="main-content" {
                    div class="flex-wrap" {
                        div class="flex-column content-well" {
                            h1 {
                                "Retainer details"
                            }
                            span class="content-title" {
                                "Name: "
                                ((self.retainer_name))
                            }
                            span class="content-title" {
                                "World: "
                                ((self.world_name))
                            }
                        }
                        div class="flex-column content-well" {
                            h2 {
                                "listings"
                            }
                            table {
                                tr {
                                    th {
                                        "item"
                                    }
                                    th {
                                        "price"
                                    }
                                    th {
                                        "qty."
                                    }
                                    th {
                                        "total"
                                    }
                                }
                                @for listing in &self.listings {
                                    tr {
                                        td {
                                            ((ItemIcon { item_id: listing.item_id, icon_size: IconSize::Small }))
                                            ((utils::get_item_name(listing.item_id)))
                                        }
                                        td {
                                            ((listing.price_per_unit))
                                        }
                                        td {
                                            ((listing.quantity))
                                        }
                                        td {
                                            ((listing.quantity * listing.price_per_unit))
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
