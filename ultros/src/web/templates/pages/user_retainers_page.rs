use maud::html;
use ultros_db::entity::{active_listing, retainer};
use ultros_db::retainers::ListingUndercutData;
use crate::web::oauth::AuthDiscordUser;
use crate::web::templates::components::header::Header;
use crate::web::templates::page::Page;
use xiv_gen::ItemId;


pub(crate) enum RetainerViewType {
  Undercuts(Vec<(retainer::Model, Vec<(active_listing::Model, ListingUndercutData)>)>),
  Listings(Vec<(retainer::Model, Vec<active_listing::Model>)>)
}

pub(crate) struct UserRetainersPage {
  pub(crate) character_names: Vec<(i32, String)>,
  pub(crate) view_type: RetainerViewType,
  pub(crate) current_user: AuthDiscordUser,
}

impl Page for UserRetainersPage {
    fn get_name<'a>(&self) -> &'a str {
        "Your Retainers"
    }

    fn draw_body(&self) -> maud::Markup {
      let items = &xiv_gen_db::decompress_data().items;
      let current_route_type = match &self.view_type {
        RetainerViewType::Undercuts(_) => "undercuts",
        RetainerViewType::Listings(_) => "listings",
      };
      
      html!{
        ((Header { user: Some(&self.current_user) }))
        div class="container" {
          div class="content-nav nav" {
            a href="/retainers/add" class="btn-secondary" {
              "Add Retainer"
            }
            @for (id, name) in &self.character_names {
              a class="btn-secondary" href={"/retainers/" ((current_route_type)) "/" ((id)) } {
                ((name))
              }
            }
            a class={ "btn-secondary" @if current_route_type == "listings" { ((" active"))} } href="/retainers/listings" {
              "Listings"
            }
            a class={"btn-secondary" @if current_route_type == "undercuts" { ((" active"))}} href="/retainers/undercuts" {
              "Undercuts"
            };
          }
          div class="main-content" {
            @if let RetainerViewType::Undercuts(undercuts) = &self.view_type {
              @for (retainer, listings) in undercuts {
                div {
                    table {
                      tr {
                        th {
                          "Item Name"
                        } th {
                          "Price Per Unit"
                        } th {
                          "Price to beat"
                        } th {
                          "# behind"
                        } th {
                          "Quantity"
                        } th {
                          "Total"
                        } th {
                          "Retainer"
                        }
                      }
                      @for (listing, undercut) in listings {
                        tr {
                          td {
                            ((items.get(&ItemId(listing.item_id)).map(|i| i.name.as_str()).unwrap_or_default()))
                          } td {
                            ((listing.price_per_unit))
                          } td {
                            ((undercut.price_to_beat))
                          } td {
                            ((undercut.number_behind))
                          } td {
                            ((listing.quantity))
                          } td {
                            ((listing.quantity * listing.price_per_unit))
                          } td {
                            ((retainer.name))
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
            @if let RetainerViewType::Listings(active) = &self.view_type {
                @for (retainer, listings) in active {
                  div {
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
                          "Retainer"
                        }
                      }
                      @for listing in listings {
                        tr {
                          td {
                            ((items.get(&ItemId(listing.item_id)).map(|i| i.name.as_str()).unwrap_or_default()))
                          } td {
                            ((listing.price_per_unit))
                          } td {
                            ((listing.quantity))
                          } td {
                            ((listing.quantity * listing.price_per_unit))
                          } td {
                            ((retainer.name))
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