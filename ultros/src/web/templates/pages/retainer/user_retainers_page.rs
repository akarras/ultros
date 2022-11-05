use crate::web::oauth::AuthDiscordUser;
use crate::web::templates::components::gil::Gil;
use crate::web::templates::components::header::Header;
use crate::web::templates::components::item_icon::{IconSize, ItemIcon};
use crate::web::templates::page::Page;
use maud::html;
use ultros_db::entity::{active_listing, owned_retainers, retainer};
use ultros_db::retainers::ListingUndercutData;
use xiv_gen::ItemId;

pub(crate) enum RetainerViewType {
    Undercuts(
        Vec<(
            retainer::Model,
            Vec<(active_listing::Model, ListingUndercutData)>,
        )>,
    ),
    Listings(Vec<(retainer::Model, Vec<active_listing::Model>)>),
}

pub(crate) struct UserRetainersPage {
    pub(crate) character_names: Vec<(i32, String)>,
    pub(crate) view_type: RetainerViewType,
    pub(crate) owned_retainers: Vec<owned_retainers::Model>,
    pub(crate) current_user: AuthDiscordUser,
}

impl Page for UserRetainersPage {
    fn get_name(&'_ self) -> &'_ str {
        "Your Retainers"
    }

    fn draw_body(&self) -> maud::Markup {
        let items = &xiv_gen_db::decompress_data().items;
        let current_route_type = match &self.view_type {
            RetainerViewType::Undercuts(_) => "undercuts",
            RetainerViewType::Listings(_) => "listings",
        };

        html! {
            ((Header { user: Some(&self.current_user) }))
            div class="container" {
              div class="content-nav nav" {
                a href="/retainers/edit" class="btn-secondary" {
                  i class="fa-solid fa-pen-to-square" {}
                  "Edit Retainers"
                }
                @for (id, name) in &self.character_names {
                  a class="btn-secondary" href={"/retainers/" ((current_route_type)) "/" ((id)) } {
                    ((name))
                  }
                }
                a class={ "btn-secondary" @if current_route_type == "listings" { ((" active"))} } href="/retainers/listings" {
                  i class="fa-solid fa-sack-dollar" {}
                  "Listings"
                }
                a class={"btn-secondary" @if current_route_type == "undercuts" { ((" active"))}} href="/retainers/undercuts" {
                  i class="fa-solid fa-exclamation" {}
                  "Undercuts"
                };
              }
              div class="main-content" {
                @if let RetainerViewType::Undercuts(undercuts) = &self.view_type {
                  @for ((retainer, listings), _owned) in undercuts.iter().zip(self.owned_retainers.iter()) {
                    div class="content-well" {
                        span class="content-title" {
                          ((retainer.name))
                        }
                        table {
                          tr {
                            th {
                              "Item Name"
                            } th {
                              "Price Per Unit"
                            } th {
                              "Price to beat"
                            } th {
                              "Loss"
                            } th {
                              "# behind"
                            } th {
                              "Quantity"
                            } th {
                              "HQ"
                            }
                          }
                          @for (listing, undercut) in listings {
                            tr {
                              td {
                                ((ItemIcon { item_id: listing.item_id, icon_size: IconSize::Small }))
                                ((items.get(&ItemId(listing.item_id)).map(|i| i.name.as_str()).unwrap_or_default()))
                              } td {
                                ((Gil(listing.price_per_unit)))
                              } td {
                                ((Gil(undercut.price_to_beat)))
                              } td {
                                ((Gil(undercut.price_to_beat - listing.price_per_unit)))
                              }  td {
                                ((undercut.number_behind))
                              } td {
                                ((listing.quantity))
                              } td {
                                @if listing.hq {
                                  "✔️"
                                }
                              }
                            }
                          }
                        }
                      }
                    }
                  }
                @if let RetainerViewType::Listings(active) = &self.view_type {
                    @for ((retainer, listings), _owned) in active.iter().zip(self.owned_retainers.iter()) {
                      div class="content-well" {
                        span class="content-title" {
                          ((retainer.name))
                        }
                        @if listings.is_empty() {
                          "No listings"
                        } @ else {
                          table {
                            tr {
                              th {
                                "Item Name"
                              } th {
                                "Price Per Unit"
                              } th {
                                "Quantity"
                              } th {
                                "Total"
                              } th {
                                "HQ"
                              }
                            }
                            @for listing in listings {
                              tr {
                                td {
                                  ((ItemIcon { item_id: listing.item_id, icon_size: IconSize::Small }))
                                  ((items.get(&ItemId(listing.item_id)).map(|i| i.name.as_str()).unwrap_or_default()))
                                } td {
                                  ((Gil(listing.price_per_unit)))
                                } td {
                                  ((listing.quantity))
                                } td {
                                  ((Gil(listing.quantity * listing.price_per_unit)))
                                } td {
                                  @if listing.hq {
                                    "✔️"
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
