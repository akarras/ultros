use maud::html;
use ultros_db::entity::active_listing;

use crate::{
    utils,
    web::{
        oauth::AuthDiscordUser,
        templates::{components::header::Header, page::Page},
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
    fn get_name<'a>(&'a self) -> &'a str {
        &self.retainer_name
    }

    fn draw_body(&self) -> maud::Markup {
        html! {
            ((Header { user: self.user.as_ref() }))

            div class="container" {
                div class="content-nav nav" {
                  a class="btn-secondary" href={"/retainers/add/" ((self.retainer_id)) } {
                    "Claim Retainer"
                  }
                }
                div class="main-content" {
                    span {
                        ((self.retainer_name))
                    }
                    span {
                        ((self.world_name))
                    }
                    hr {}
                    table {
                        tr {
                            th {
                                "Item"
                            }
                            th {
                                "Price Per Unit"
                            }
                            th {
                                "Quantity"
                            }
                            th {
                                "Total"
                            }
                        }
                        @for listing in &self.listings {
                            tr {
                                td {
                                    img class="small-icon" src=((utils::get_item_icon_url(listing.item_id))) {}
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
